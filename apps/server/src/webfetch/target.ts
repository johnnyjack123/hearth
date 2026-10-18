/**
 * Which URLs the web fetch accepts, and which hosts it may load without
 * asking the user.
 */
import { isIP } from 'node:net';
import type { ChatMessage } from '../services/conversations.ts';
import { WebFetchError } from './errors.ts';

/** Longest URL the web fetch accepts; longer ones smell of encoded data. */
export const MAX_URL_LENGTH = 2_048;

/**
 * What {@link parseTarget} checks besides the syntax.
 */
export interface TargetPolicy {
  /** Whether a resolved or literal address may be contacted. */
  isAllowedAddress: (ip: string) => boolean;
  /** Ports the fetch may connect to. */
  ports: readonly number[];
}

/**
 * Host names as the gateway allows them (`Host::parse`): letters, digits,
 * `-` and `_`, and a last label that starts with a letter. That rules out
 * numeric forms a resolver would turn into addresses.
 */
const LABEL = /^[a-z0-9_-]{1,63}$/;

function bareHost(hostname: string): string {
  return hostname.startsWith('[') && hostname.endsWith(']') ? hostname.slice(1, -1) : hostname;
}

/**
 * The host as written in the URL, before the WHATWG parser normalized it:
 * that parser turns `127.1`, `0x7f000001`, `2130706433` or `%31%32%37.0.0.1`
 * into `127.0.0.1`. Comparing both catches such tricks.
 */
function rawHost(raw: string): string | null {
  const authority = /^[a-z][a-z0-9+.-]*:\/\/([^/?#\\]*)/i.exec(raw)?.[1];
  if (authority === undefined || authority.includes('@')) {
    return null;
  }
  if (authority.startsWith('[')) {
    const end = authority.indexOf(']');
    return end === -1 ? null : authority.slice(0, end + 1);
  }
  const colon = authority.lastIndexOf(':');
  return colon === -1 ? authority : authority.slice(0, colon);
}

function invalid(message: string): WebFetchError {
  return new WebFetchError('invalid_url', `${message} Only absolute http:// and https:// URLs to public hosts work.`);
}

/**
 * Validates a URL the model wants to load: scheme, credentials, host syntax,
 * literal addresses and port. Host names are only checked for syntax here;
 * their addresses are checked when connecting.
 *
 * @param raw - URL from the model or a redirect.
 * @param policy - See {@link TargetPolicy}.
 * @returns The parsed URL without fragment.
 * @throws {WebFetchError} `invalid_url`, `blocked_address` or `blocked_port`.
 */
export function parseTarget(raw: string, policy: TargetPolicy): URL {
  if (raw.length > MAX_URL_LENGTH) {
    throw invalid(`The URL is longer than ${MAX_URL_LENGTH} characters.`);
  }
  // Whitespace and control characters would be dropped or encoded by the
  // parser, so the user would approve something else than what is loaded.
  if (/[\s\p{Cc}\p{Cf}]/u.test(raw)) {
    throw invalid('The URL contains whitespace or control characters.');
  }
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    throw invalid('This is not a valid URL.');
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    throw invalid(`The scheme ${url.protocol} is not supported.`);
  }
  if (url.username || url.password) {
    throw invalid('URLs with credentials are not allowed.');
  }
  const written = rawHost(raw);
  if (written === null) {
    throw invalid('The URL has no valid host.');
  }
  const host = bareHost(url.hostname);
  if (isIP(host)) {
    if (written.toLowerCase() !== url.hostname.toLowerCase()) {
      throw invalid('Addresses must be written in their usual form.');
    }
    if (!policy.isAllowedAddress(host)) {
      throw new WebFetchError('blocked_address', `${url.hostname} is not a public address and cannot be loaded.`);
    }
  } else {
    const name = host.endsWith('.') ? host.slice(0, -1) : host;
    const labels = name.split('.');
    if (name.length > 253 || !labels.every((label) => LABEL.test(label)) || !/^[a-z]/.test(labels.at(-1) ?? '')) {
      throw invalid(`${url.hostname} is not a valid host name.`);
    }
  }
  const port = url.port ? Number(url.port) : url.protocol === 'https:' ? 443 : 80;
  if (!policy.ports.includes(port)) {
    throw new WebFetchError('blocked_port', `Port ${port} is not allowed; only ${policy.ports.join(' and ')} are.`, String(port));
  }
  url.hash = '';
  return url;
}

/**
 * Normalizes a host for comparisons: lowercase, without trailing dot and
 * without a leading `www.`, because sites redirect between both forms and
 * the same party controls them.
 *
 * @param hostname - Host as in `URL.hostname`.
 * @returns The comparison key.
 */
export function hostKey(hostname: string): string {
  const lower = hostname.toLowerCase().replace(/\.$/, '');
  return lower.startsWith('www.') ? lower.slice(4) : lower;
}

const URL_IN_TEXT = /\bhttps?:\/\/[^\s<>"'`]+/giu;
// A bare domain like `tasks.org`: labels of letters and digits, a last label
// of letters (or punycode), not part of an e-mail address or a longer word.
const DOMAIN_IN_TEXT =
  /(?<![\p{L}\p{N}@._/-])((?:[\p{L}\p{N}](?:[\p{L}\p{N}-]{0,61}[\p{L}\p{N}])?\.)+(?:\p{L}{2,63}|xn--[a-z0-9-]{1,59}))(?![\p{L}\p{N}_-]|\.[\p{L}\p{N}])/giu;
// File names look like domains (`README.md`, `main.rs`); some of these
// extensions are real top-level domains, which an attacker could register.
const FILE_EXTENSIONS = new Set(
  'md rs py sh pl rb cc so zip mov ps sql js ts tsx jsx mjs cjs json yml yaml toml txt html htm css lock log csv exe dll png jpg jpeg gif svg pdf java kt php cpp xml ini cfg conf bat env gz tar bak'.split(
    ' ',
  ),
);

function hostOf(text: string): string | null {
  try {
    const { hostname } = new URL(text.includes('://') ? text : `http://${text}`);
    return hostname ? hostKey(hostname) : null;
  } catch {
    return null;
  }
}

/**
 * Hosts the user named in their own messages, as full URLs or bare domains.
 * The model may load these without asking: the user asked about them.
 *
 * @param messages - The conversation, including the new message.
 * @returns Host keys (see {@link hostKey}).
 */
export function mentionedHosts(messages: ChatMessage[]): Set<string> {
  const hosts = new Set<string>();
  for (const message of messages) {
    if (message.role !== 'user') {
      continue;
    }
    for (const part of message.parts) {
      if (part.type !== 'text') {
        continue;
      }
      for (const match of part.text.matchAll(URL_IN_TEXT)) {
        const host = hostOf(match[0]);
        if (host) {
          hosts.add(host);
        }
      }
      // Paths of full URLs are no domains (`…/repo.git`).
      for (const match of part.text.replace(URL_IN_TEXT, ' ').matchAll(DOMAIN_IN_TEXT)) {
        const domain = match[1] ?? '';
        const tld = domain.split('.').at(-1)?.toLowerCase() ?? '';
        const host = FILE_EXTENSIONS.has(tld) ? null : hostOf(domain);
        if (host) {
          hosts.add(host);
        }
      }
    }
  }
  return hosts;
}

/**
 * Hosts the user allowed for the rest of the chat in an earlier answer. The
 * answers are stored in assistant messages, which only the server writes.
 *
 * @param messages - The conversation.
 * @returns Host keys (see {@link hostKey}).
 */
export function grantedHosts(messages: ChatMessage[]): Set<string> {
  const hosts = new Set<string>();
  for (const message of messages) {
    if (message.role !== 'assistant') {
      continue;
    }
    for (const part of message.parts) {
      if (part.type === 'data-approval' && part.data.url && part.data.answer === 'session') {
        hosts.add(hostKey(part.data.host));
      }
    }
  }
  return hosts;
}
