import { type DeviceCredential, PROVIDERS, type ProviderKind } from '@harness/shared';
import { Hono } from 'hono';
import { type AppDeps, type DeviceEnv, requireDevice } from '../http/context.ts';
import { HttpError, notFound } from '../http/errors.ts';
import { ProviderRequestError } from '../llm.ts';
import { loadHelpers } from '../helpers/load.ts';
import { findCredential, listCredentialRows, resolveCredential } from '../services/credentials.ts';
import { getServerSettings } from '../services/settings.ts';
import { agentFetch, APPROVED_HOST_HEADER, URL_HEADER } from '../webfetch/agent.ts';
import { WebFetchError } from '../webfetch/errors.ts';
import { DEFAULT_WEB_FETCH_CONFIG, pageRequestSchema } from '../webfetch/tool.ts';
import { assertEndpointAllowed } from './providers.ts';

/**
 * Wire protocol a provider kind speaks; decides allowed paths and how the key
 * is sent.
 */
type Protocol = 'openai' | 'anthropic' | 'google';

function protocolOf(kind: ProviderKind): Protocol {
  switch (kind) {
    case 'anthropic':
      return 'anthropic';
    case 'google':
      return 'google';
    default:
      return 'openai';
  }
}

// Only the endpoints harnesses need for chatting. Anything else (files,
// fine-tuning, batches, admin APIs) stays out of reach of a paired device.
const ALLOWED_PATHS: Record<Protocol, RegExp> = {
  openai: /^(chat\/completions|completions|responses|embeddings|models(\/[\w.:-]+)?)$/,
  anthropic: /^v1\/(messages(\/count_tokens)?|models(\/[\w.:-]+)?)$/,
  google: /^v1beta\/models(\/[\w.-]+(:(generateContent|streamGenerateContent|countTokens))?)?$/,
};

// Request headers a client may pass on; credentials are set by the server.
const FORWARDED_REQUEST_HEADERS = ['content-type', 'accept', 'anthropic-version', 'anthropic-beta', 'openai-beta'];

// Content-Encoding and Content-Length are dropped on purpose: fetch has
// already decompressed the body.
const FORWARDED_RESPONSE_HEADERS = ['content-type', 'cache-control', 'retry-after', 'request-id', 'x-request-id'];
const FORWARDED_RESPONSE_PREFIXES = ['x-ratelimit-', 'anthropic-ratelimit-'];

/**
 * Root the client's native paths are appended to. Clients of the Anthropic
 * and Google APIs include the version in their paths (`/v1/messages`,
 * `/v1beta/models/…`), OpenAI-style clients put it into the base URL, which
 * matches how the provider defaults are stored.
 */
function proxyRoot(kind: ProviderKind, baseUrl: string | null): string {
  const url = (baseUrl ?? PROVIDERS[kind].defaultBaseUrl ?? '').replace(/\/+$/, '');
  if (!url) {
    throw new HttpError(409, 'provider_error', 'No endpoint configured for this credential');
  }
  switch (protocolOf(kind)) {
    case 'anthropic':
      return url.replace(/\/v1$/, '');
    case 'google':
      return url.replace(/\/v1beta$/, '');
    case 'openai':
      return url;
  }
}

function authHeaders(protocol: Protocol, apiKey: string | null): Record<string, string> {
  if (!apiKey) {
    return {};
  }
  switch (protocol) {
    case 'anthropic':
      return { 'x-api-key': apiKey };
    case 'google':
      return { 'x-goog-api-key': apiKey };
    case 'openai':
      return { authorization: `Bearer ${apiKey}` };
  }
}

/**
 * LLM access for paired devices (`/api/llm`): which credentials there are,
 * and a pass-through to the providers that adds the stored key. Harnesses
 * speak their provider's native API, so features like streaming, reasoning
 * and prompt caching work unchanged; the key never leaves the server.
 *
 * @param deps - Application dependencies.
 * @returns Router to mount under `/api/llm`.
 */
export function llmRoutes(deps: AppDeps) {
  const { db } = deps.database;
  const { secretBox, llm } = deps;
  const webFetchConfig = { ...DEFAULT_WEB_FETCH_CONFIG, ...deps.webFetch };

  return new Hono<DeviceEnv>()
    .use(requireDevice(db))
    .get('/providers', async (c) => {
      const owner = c.get('owner');
      const rows = await listCredentialRows(db, owner.id);
      const credentials = await Promise.all(
        rows.map(async (row): Promise<DeviceCredential> => {
          let models: DeviceCredential['models'] = null;
          // Harnesses know the catalogs of the big providers; only user-run
          // endpoints need their model list from here.
          if (PROVIDERS[row.kind].userDefinedEndpoint) {
            try {
              await assertEndpointAllowed(db, owner, row.kind, row.baseUrl);
              models = await llm.listModels(resolveCredential(secretBox, row));
            } catch (error) {
              if (!(error instanceof ProviderRequestError || error instanceof HttpError)) {
                throw error;
              }
            }
          }
          return { id: row.id, kind: row.kind, label: row.label, baseUrl: row.baseUrl, models };
        }),
      );
      return c.json(credentials);
    })
    // `web_fetch` of agents. Under the proxy path because that is all the
    // gateway reaches (`http://llm.harness/_web/fetch`); credential ids are
    // UUIDs, never `_web`. Only the gateway sets the approved host, see
    // `webfetch/agent.ts`.
    .post('/proxy/_web/fetch', async (c) => {
      const url = c.req.header(URL_HEADER);
      const approvedHost = c.req.header(APPROVED_HOST_HEADER);
      if (!url || !approvedHost) {
        throw new HttpError(400, 'validation', `${URL_HEADER} and ${APPROVED_HOST_HEADER} are required`);
      }
      if (!(await getServerSettings(db)).webFetch) {
        return c.json({ error: 'Loading web pages is switched off on the harness server.' }, 403);
      }
      const request = pageRequestSchema.strict().parse(await c.req.json());
      const helpers = await loadHelpers(db, secretBox, c.get('owner').id, null, deps.helpers);
      try {
        return c.json(await agentFetch(url, approvedHost, request, { config: webFetchConfig, helpers, signal: c.req.raw.signal }));
      } catch (error) {
        if (error instanceof WebFetchError) {
          return c.json({ error: error.message }, 422);
        }
        throw error;
      }
    })
    .all('/proxy/:credentialId/*', async (c) => {
      const owner = c.get('owner');
      const credentialId = c.req.param('credentialId');
      const row = await findCredential(db, owner.id, credentialId);
      if (!row) {
        throw notFound('Credential');
      }
      const protocol = protocolOf(row.kind);
      const prefix = `/api/llm/proxy/${credentialId}/`;
      const path = c.req.path.startsWith(prefix) ? c.req.path.slice(prefix.length) : '';
      if (!ALLOWED_PATHS[protocol].test(path)) {
        throw notFound('LLM endpoint');
      }
      await assertEndpointAllowed(db, owner, row.kind, row.baseUrl);
      const credential = resolveCredential(secretBox, row);

      const headers = new Headers(authHeaders(protocol, credential.apiKey));
      for (const name of FORWARDED_REQUEST_HEADERS) {
        const value = c.req.header(name);
        if (value) {
          headers.set(name, value);
        }
      }
      const method = c.req.method;
      const hasBody = method !== 'GET' && method !== 'HEAD';
      const target = `${proxyRoot(row.kind, row.baseUrl)}/${path}${new URL(c.req.url).search}`;

      let upstream: Response;
      try {
        upstream = await fetch(target, {
          method,
          headers,
          body: hasBody ? c.req.raw.body : undefined,
          // Streams the request body instead of buffering it.
          ...(hasBody ? { duplex: 'half' } : {}),
          redirect: 'error',
          signal: c.req.raw.signal,
        } as RequestInit);
      } catch (error) {
        const reason = error instanceof Error ? error.message : String(error);
        throw new HttpError(502, 'provider_error', `Provider not reachable: ${reason}`);
      }

      const responseHeaders = new Headers();
      upstream.headers.forEach((value, name) => {
        const lower = name.toLowerCase();
        if (
          FORWARDED_RESPONSE_HEADERS.includes(lower) ||
          FORWARDED_RESPONSE_PREFIXES.some((p) => lower.startsWith(p))
        ) {
          responseHeaders.set(name, value);
        }
      });
      return new Response(upstream.body, { status: upstream.status, headers: responseHeaders });
    });
}
