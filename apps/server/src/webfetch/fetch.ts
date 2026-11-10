/**
 * The HTTP part of the web fetch: one GET with redirects followed by hand,
 * DNS answers checked before connecting, and limits on time and size.
 */
import { lookup as dnsLookup } from 'node:dns/promises';
import http, { type IncomingMessage } from 'node:http';
import https from 'node:https';
import type { LookupFunction } from 'node:net';
import type { Readable, Transform } from 'node:stream';
import { createBrotliDecompress, createGunzip, createInflate } from 'node:zlib';
import { WebFetchError } from './errors.ts';
import { parseTarget, type TargetPolicy } from './target.ts';

/** An address a host name resolved to. */
export interface ResolvedAddress {
  /** IPv4 or IPv6 address. */
  address: string;
  /** 4 or 6. */
  family: number;
}

/**
 * Limits and checks of a fetch. Production code uses the defaults of
 * `DEFAULT_WEB_FETCH_CONFIG`; tests swap the address filter and the resolver
 * to reach a local server.
 */
export interface FetchPolicy extends TargetPolicy {
  /** Resolves a host name to all its addresses. */
  resolve: (hostname: string) => Promise<ResolvedAddress[]>;
  /** Time for the whole fetch including redirects, in milliseconds. */
  timeoutMs: number;
  /** Most bytes read from a response body (after decompression). */
  maxBytes: number;
  /** Most redirects followed. */
  maxRedirects: number;
}

/**
 * Resolves with the operating system's resolver, like a browser would.
 *
 * @param hostname - Host name.
 * @returns All addresses.
 */
export async function systemResolve(hostname: string): Promise<ResolvedAddress[]> {
  return dnsLookup(hostname, { all: true, order: 'verbatim' });
}

/**
 * A response body read within the limits.
 */
export interface FetchedPage {
  /** URL of the final response. */
  url: URL;
  /** HTTP status. */
  status: number;
  /** Media type without parameters, lowercase. */
  contentType: string;
  /** Charset from the Content-Type header, if any. */
  charset: string | null;
  /** The body, at most `maxBytes`. */
  body: Buffer;
  /** True if the body was longer than `maxBytes`. */
  truncated: boolean;
}

const TEXT_TYPES = new Set([
  'text/html',
  'application/xhtml+xml',
  'text/plain',
  'text/markdown',
  'text/x-markdown',
  'application/json',
  'text/xml',
  'application/xml',
]);

/**
 * Whether the web fetch reads this media type: HTML, plain text, Markdown,
 * JSON and XML, including `+json` and `+xml` types (feeds, JSON-LD).
 *
 * @param type - Media type without parameters, lowercase.
 * @returns True for textual types.
 */
export function isTextual(type: string): boolean {
  return TEXT_TYPES.has(type) || /^application\/[\w.-]+\+(json|xml)$/.test(type);
}

const HEADERS = {
  'user-agent': 'Hearth-WebFetch/1.0 (self-hosted chat; fetches pages a user asked about)',
  accept: 'text/html,application/xhtml+xml,text/markdown,text/plain;q=0.9,application/json;q=0.8,application/xml;q=0.8,*/*;q=0.1',
  'accept-encoding': 'gzip, deflate, br',
};

/**
 * A DNS lookup for `http.request` that refuses a host as soon as one of its
 * addresses is not allowed, and hands the socket exactly the addresses it
 * checked. There is no second lookup an attacker could answer differently
 * (DNS rebinding).
 */
function guardedLookup(policy: FetchPolicy): LookupFunction {
  return (hostname, options, callback) => {
    policy
      .resolve(hostname)
      .then((addresses) => {
        const blocked = addresses.find((entry) => !policy.isAllowedAddress(entry.address));
        if (blocked) {
          throw new WebFetchError('blocked_address', `${hostname} resolves to a non-public address and cannot be loaded.`);
        }
        const family = options.family === 4 || options.family === 6 ? options.family : 0;
        const usable = family ? addresses.filter((entry) => entry.family === family) : addresses;
        const first = usable[0];
        if (!first) {
          throw new WebFetchError('network', `${hostname} could not be resolved.`);
        }
        if (options.all) {
          callback(null, usable);
        } else {
          callback(null, first.address, first.family);
        }
      })
      .catch((error: unknown) => {
        callback(error as NodeJS.ErrnoException, '', 0);
      });
  };
}

function networkError(error: unknown, url: URL): WebFetchError {
  if (error instanceof WebFetchError) {
    return error;
  }
  const code = (error as NodeJS.ErrnoException | undefined)?.code ?? '';
  if (code === 'ENOTFOUND' || code === 'EAI_AGAIN' || code === 'ENODATA') {
    return new WebFetchError('network', `${url.hostname} could not be resolved.`);
  }
  return new WebFetchError('network', `Connecting to ${url.host} failed (${code || (error as Error).message}).`, code || undefined);
}

function get(url: URL, policy: FetchPolicy, signal: AbortSignal): Promise<IncomingMessage> {
  return new Promise((resolve, reject) => {
    const client = url.protocol === 'https:' ? https : http;
    const request = client.request(
      url,
      {
        method: 'GET',
        headers: HEADERS,
        // A fresh connection per request: a pooled socket would skip the
        // lookup above.
        agent: false,
        lookup: guardedLookup(policy),
        signal,
      },
      resolve,
    );
    request.on('error', reject);
    request.end();
  });
}

function parseContentType(header: string | undefined): { type: string; charset: string | null } {
  const [type = '', ...params] = (header ?? '').split(';');
  const charset = params
    .map((param) => /^\s*charset\s*=\s*"?([\w.:-]+)"?\s*$/i.exec(param)?.[1])
    .find((value) => value !== undefined);
  return { type: type.trim().toLowerCase(), charset: charset ?? null };
}

function decoderFor(encoding: string): Transform | null {
  switch (encoding) {
    case '':
    case 'identity':
      return null;
    case 'gzip':
    case 'x-gzip':
      return createGunzip();
    case 'deflate':
      return createInflate();
    case 'br':
      return createBrotliDecompress();
    default:
      throw new WebFetchError('unsupported_encoding', `The server sent an unsupported encoding (${encoding}).`, encoding);
  }
}

async function readBody(response: IncomingMessage, maxBytes: number): Promise<{ body: Buffer; truncated: boolean }> {
  const decoder = decoderFor((response.headers['content-encoding'] ?? '').trim().toLowerCase());
  // The limit counts decompressed bytes, so a compression bomb stops early.
  const source: Readable = decoder ? response.pipe(decoder) : response;
  if (decoder) {
    response.once('error', (error) => decoder.destroy(error));
  }
  const chunks: Buffer[] = [];
  let size = 0;
  let truncated = false;
  try {
    for await (const chunk of source) {
      const buffer = chunk as Buffer;
      if (size + buffer.length > maxBytes) {
        chunks.push(buffer.subarray(0, maxBytes - size));
        size = maxBytes;
        truncated = true;
        break;
      }
      chunks.push(buffer);
      size += buffer.length;
    }
  } finally {
    response.destroy();
    decoder?.destroy();
  }
  return { body: Buffer.concat(chunks, size), truncated };
}

const REDIRECTS = new Set([301, 302, 303, 307, 308]);

/**
 * Options of {@link fetchPage}.
 */
export interface FetchPageOptions {
  /** Aborts the fetch, e.g. when the chat request ends. */
  signal?: AbortSignal;
  /**
   * Called before a redirect is followed, with the already validated target.
   * Throws to stop, e.g. when the user does not allow the new host.
   */
  onRedirect?: (next: URL, from: URL) => Promise<void>;
}

/**
 * Loads a URL that {@link parseTarget} accepted. Redirects are followed by
 * hand, and every hop is validated and passed to `onRedirect` again.
 *
 * @param start - Validated URL.
 * @param policy - See {@link FetchPolicy}.
 * @param options - See {@link FetchPageOptions}.
 * @returns The final response with its body.
 * @throws {WebFetchError} On blocked targets, HTTP errors, unsupported types,
 *   timeouts and network errors. Aborts via `signal` reject with the abort reason.
 */
export async function fetchPage(start: URL, policy: FetchPolicy, options: FetchPageOptions = {}): Promise<FetchedPage> {
  // The time limit covers the network only: while `onRedirect` waits for the
  // user, the clock stops.
  let remaining = policy.timeoutMs;
  let deadline: AbortSignal | undefined;
  let url = start;
  try {
    for (let hop = 0; ; hop += 1) {
      const hopStarted = Date.now();
      deadline = AbortSignal.timeout(Math.max(remaining, 1));
      const signal = options.signal ? AbortSignal.any([options.signal, deadline]) : deadline;
      let response: IncomingMessage;
      try {
        response = await get(url, policy, signal);
      } catch (error) {
        if (signal.aborted) {
          throw error;
        }
        throw networkError(error, url);
      }
      const status = response.statusCode ?? 0;
      const location = response.headers.location;
      if (REDIRECTS.has(status) && location) {
        response.destroy();
        if (hop >= policy.maxRedirects) {
          throw new WebFetchError('too_many_redirects', `Gave up after ${policy.maxRedirects} redirects.`);
        }
        let next: URL;
        try {
          next = new URL(location, url);
        } catch {
          throw new WebFetchError('invalid_url', 'The server redirected to an invalid URL.');
        }
        const target = parseTarget(next.href, policy);
        remaining -= Date.now() - hopStarted;
        await options.onRedirect?.(target, url);
        url = target;
        continue;
      }
      if (status < 200 || status >= 300) {
        response.destroy();
        throw new WebFetchError('http_error', `The server answered with HTTP ${status}.`, String(status));
      }
      const { type, charset } = parseContentType(response.headers['content-type']);
      if (!isTextual(type)) {
        response.destroy();
        throw new WebFetchError(
          'unsupported_type',
          `The content type ${type || '(none)'} is not supported; only HTML, text, Markdown, JSON and XML are.`,
          type || 'none',
        );
      }
      let read: { body: Buffer; truncated: boolean };
      try {
        read = await readBody(response, policy.maxBytes);
      } catch (error) {
        if (signal.aborted) {
          throw error;
        }
        throw networkError(error, url);
      }
      const { body, truncated } = read;
      return { url, status, contentType: type, charset, body, truncated };
    }
  } catch (error) {
    if (deadline?.aborted && !options.signal?.aborted) {
      throw new WebFetchError('timeout', `The page did not load within ${Math.round(policy.timeoutMs / 1000)} seconds.`);
    }
    throw error;
  }
}
