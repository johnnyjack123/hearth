import { type DeviceCredential, memoryRef, memoryTextSchema, projectNameSchema, PROVIDERS, type ProviderKind, SEARCH_KINDS } from '@hearth/shared';
import { createHash } from 'node:crypto';
import { type Context, Hono } from 'hono';
import { z } from 'zod';
import { type AppDeps, type DeviceEnv, requireDevice } from '../http/context.ts';
import { HttpError, notFound } from '../http/errors.ts';
import { DocumentError, readDocument, readDocumentSchema } from '../files/read.ts';
import { fileStore } from '../files/store.ts';
import { documentVectors } from '../files/vectors.ts';
import { ProviderRequestError } from '../llm.ts';
import { loadHelpers } from '../helpers/load.ts';
import { findCredential, listCredentialRows, resolveCredential } from '../services/credentials.ts';
import { chatFeatures } from '../services/features.ts';
import { loadSearchBackend } from '../services/settings.ts';
import { agentFetch, APPROVED_HOST_HEADER, URL_HEADER } from '../webfetch/agent.ts';
import { WebFetchError } from '../webfetch/errors.ts';
import { DEFAULT_WEB_FETCH_CONFIG, pageRequestSchema } from '../webfetch/tool.ts';
import { DEFAULT_SEARCH_ENDPOINTS, WebSearchError } from '../websearch/providers.ts';
import { runSearch, searchModelText } from '../websearch/tool.ts';
import { memoryText } from '../memory/tool.ts';
import { clientBetas, providerSideTool } from '../llm-tools.ts';
import { cacheKey, cheapestProvider, meterBill, pinnedProvider } from '../openrouter.ts';
import { blockedHosts } from '../services/host-access.ts';
import { pinnedHost } from '../services/hosts.ts';
import { costGuard } from '../services/costs.ts';
import { addMemory, removeMemory } from '../services/memories.ts';
import { assertEndpointAllowed } from './providers.ts';

/** What an agent's memory tools send (`pi/memory.ts`). */
const memoryRequestSchema = z.discriminatedUnion('action', [
  z.object({ action: z.literal('remember'), text: memoryTextSchema }).strict(),
  z.object({ action: z.literal('forget'), id: z.string().trim().min(1).max(64) }).strict(),
]);

/** What an agent sends to search: the same as the web chat's tool. */
const searchRequestSchema = z
  .object({ query: z.string().trim().min(1).max(400), kind: z.enum(SEARCH_KINDS).optional() })
  .strict();

/** Set by the gateway on every request: the agent's project (`PROJECT_HEADER` in `llm.rs`). */
const PROJECT_HEADER = 'x-hearth-project';

/**
 * The project an agent's request comes from. The gateway sets it and drops
 * any the agent sends; gateways before 0.1.17 send none, then the whole
 * device counts as before.
 *
 * @param c - The request.
 * @returns The project, or null for the device.
 * @throws {HttpError} 400 for a name no gateway sends.
 */
function projectOf(c: Context): string | null {
  const header = c.req.header(PROJECT_HEADER);
  if (header === undefined) {
    return null;
  }
  const parsed = projectNameSchema.safeParse(header);
  if (!parsed.success) {
    throw new HttpError(400, 'validation', `${PROJECT_HEADER} is no project name`);
  }
  return parsed.data;
}

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

// Model lists are read, everything else is a call: one method per path, so
// a device cannot `DELETE` a fine-tuned model under `models/…`.
const LIST_PATHS: Record<Protocol, RegExp> = {
  openai: /^models(\/[\w.:-]+)?$/,
  anthropic: /^v1\/models(\/[\w.:-]+)?$/,
  google: /^v1beta\/models(\/[\w.-]+)?$/,
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
  const searchEndpoints = { ...DEFAULT_SEARCH_ENDPOINTS, ...deps.webSearch };

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
    // gateway reaches (`http://llm.hearth/_web/fetch`); credential ids are
    // UUIDs, never `_web`. Only the gateway sets the approved host, see
    // `webfetch/agent.ts`.
    .post('/proxy/_web/fetch', async (c) => {
      const url = c.req.header(URL_HEADER);
      const approvedHost = c.req.header(APPROVED_HOST_HEADER);
      if (!url || !approvedHost) {
        throw new HttpError(400, 'validation', `${URL_HEADER} and ${APPROVED_HOST_HEADER} are required`);
      }
      if (!(await chatFeatures(db, c.get('owner').id)).webFetch) {
        return c.json({ error: 'Loading web pages is switched off in the settings of the hearth web app.' }, 403);
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
    // `web_search` of agents, only while a reply of the device runs in
    // research mode: the switch in the chat decides, not the agent.
    .post('/proxy/_web/search', async (c) => {
      const input = searchRequestSchema.parse(await c.req.json());
      const project = projectOf(c);
      const question = deps.hub.researchQuestion(c.get('device').id, project);
      if (question === null) {
        return c.json(
          { error: '[research_off] Research mode is off; the user can switch it on in the web chat. Answer without searching.' },
          403,
        );
      }
      const backend = await loadSearchBackend(db, secretBox, searchEndpoints);
      if (!backend) {
        return c.json({ error: '[not_configured] No search provider is set up on the hearth server.' }, 403);
      }
      const helpers = await loadHelpers(db, secretBox, c.get('owner').id, null, deps.helpers);
      try {
        const output = await runSearch(backend, input, { helpers, question }, c.req.raw.signal);
        // Reading the hits needs no question to the user (`_web/known`).
        deps.hub.researchFound(
          c.get('device').id,
          project,
          output.results.map((result) => result.url),
        );
        return c.json({ text: searchModelText(output), results: output.results.length, warnings: output.warnings });
      } catch (error) {
        if (error instanceof WebSearchError) {
          return c.json({ error: error.message }, 422);
        }
        throw error;
      }
    })
    // Asked by the gateway itself (agents cannot reach this path) before it
    // asks the user about the host of a `web_fetch`: a hit of a running
    // research reply loads without a question. 204 yes, 404 no.
    .post('/proxy/_web/known', async (c) => {
      const { url } = z.object({ url: z.string().max(4_000) }).parse(await c.req.json());
      return c.body(null, deps.hub.researchKnows(c.get('device').id, projectOf(c), url) ? 204 : 404);
    })
    // The memory tools of agents, under the proxy path like `_web/search`;
    // the memories belong to the device's owner.
    .post('/proxy/_memory', async (c) => {
      if (!(await chatFeatures(db, c.get('owner').id)).memory) {
        return c.json({ error: '[memory_off] The memory is switched off in the settings of the hearth web app.' }, 403);
      }
      const input = memoryRequestSchema.parse(await c.req.json());
      const owner = c.get('owner').id;
      try {
        if (input.action === 'remember') {
          const memory = await addMemory(db, owner, { text: input.text, source: 'agent' });
          const output = { ref: memoryRef(memory.id), text: memory.text, ...(memory.pending ? { pending: true } : {}) };
          return c.json({ ...output, modelText: memoryText('Remembered', output) });
        }
        const memory = await removeMemory(db, owner, input.id, 'agent');
        if (!memory) {
          return c.json({ error: `There is no memory [${input.id}].` }, 404);
        }
        const output = { ref: memoryRef(memory.id), text: memory.text };
        return c.json({ ...output, modelText: memoryText('Forgot', output) });
      } catch (error) {
        if (error instanceof HttpError) {
          return c.json({ error: error.message }, error.status);
        }
        throw error;
      }
    })
    // `read_document` of agents (`pi/documents.ts`): the documents of the
    // chats whose replies run on this device right now, searched like in the
    // web chat, with the vectors kept on the server.
    .post('/proxy/_documents', async (c) => {
      const request = readDocumentSchema.strict().parse(await c.req.json());
      const owner = c.get('owner').id;
      // Only the chats of the agent's own project (security review M6).
      const sessions = deps.hub.runningSessions(c.get('device').id, projectOf(c));
      const rows = await fileStore(db, deps.config.dataDir).ofConversations(owner, sessions);
      const helpers = await loadHelpers(db, secretBox, owner, null, deps.helpers);
      try {
        return c.json(await readDocument(rows, request, { helpers, vectors: documentVectors(db), signal: c.req.raw.signal }));
      } catch (error) {
        if (error instanceof DocumentError) {
          return c.json({ error: error.message }, 404);
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
      if (c.req.method !== (LIST_PATHS[protocol].test(path) ? 'GET' : 'POST')) {
        return c.json({ error: { type: 'method_not_allowed', message: `${c.req.method} is not allowed on ${path}.` } }, 405);
      }
      await assertEndpointAllowed(db, owner, row.kind, row.baseUrl);
      // Model calls stop at the owner's daily cost limit, also those of
      // `hearth run`, whose cost the limit cannot see; listing models is free.
      if (c.req.method !== 'GET') {
        try {
          await costGuard(db).assert(owner.id);
        } catch (error) {
          if (error instanceof HttpError) {
            return c.json({ error: { type: error.code, message: error.message } }, error.status);
          }
          throw error;
        }
      }
      const credential = resolveCredential(secretBox, row);

      const headers = new Headers(authHeaders(protocol, credential.apiKey));
      for (const name of FORWARDED_REQUEST_HEADERS) {
        const value = c.req.header(name);
        // Flags for provider-side tools are dropped with the tools below.
        const forwarded = value && name === 'anthropic-beta' ? clientBetas(value) : value;
        if (forwarded) {
          headers.set(name, forwarded);
        }
      }
      const method = c.req.method;
      const hasBody = method === 'POST';
      const target = `${proxyRoot(row.kind, row.baseUrl)}/${path}${new URL(c.req.url).search}`;
      const device = c.get('device').id;
      // Every call is read before it goes out: tools the provider runs itself
      // would reach the network around the project's rules (see `llm-tools.ts`).
      const text = hasBody ? await c.req.text() : null;
      const request = text === null ? null : parseJson(text);
      const serverTool = providerSideTool(protocol, request);
      if (serverTool) {
        return c.json(
          {
            error: {
              type: 'tool_not_allowed',
              message: `Tools the provider runs itself (${serverTool.slice(0, 100)}) are not available to agents: they would reach the network around the project's rules.`,
            },
          },
          400,
        );
      }
      // Chat requests to OpenRouter go to the pinned host or the cheapest first.
      const routed = text !== null && row.kind === 'openrouter' && path === 'chat/completions' ? await route(text, request, device, owner.id, row.id) : null;
      const body = routed?.body ?? text;

      let upstream: Response;
      try {
        upstream = await fetch(target, {
          method,
          headers,
          ...(body !== null ? { body } : {}),
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
      // What the provider billed (OpenRouter says it in every answer) waits
      // for the agent's report of the call; see `agentCallCost`.
      const type = upstream.headers.get('content-type') ?? '';
      const stream = type.includes('text/event-stream');
      const metered =
        upstream.ok && upstream.body && protocol === 'openai' && (stream || type.includes('application/json'))
          ? meterBill(upstream.body, stream, (bill) => {
              deps.hub.bills.record(device, bill);
              // The conversation's next call goes back to the host that has it cached.
              if (routed?.conversation && bill.cost.host) {
                deps.openRouter?.stick(routed.conversation.model, routed.conversation.key, bill.cost.host);
              }
            })
          : upstream.body;
      return new Response(metered, { status: upstream.status, headers: responseHeaders });
    });

  /**
   * An OpenRouter chat request with the conversation's cache key and the
   * host its owner pinned the model to. Without a pin, the cheapest hosts
   * go first, the host of the conversation's last call before them, unless
   * the admin turned that off or the request routes itself.
   */
  async function route(
    text: string,
    request: unknown,
    device: string,
    owner: string,
    credentialId: string,
  ): Promise<{ body: string; conversation?: { model: string; key: string } }> {
    if (!request || typeof request !== 'object' || Array.isArray(request)) {
      return { body: text };
    }
    const fields = request as Record<string, unknown>;
    if (typeof fields.model !== 'string') {
      return { body: text };
    }
    const key = conversationKey(device, fields.messages);
    const changes: Record<string, unknown> = fields.prompt_cache_key === undefined ? { prompt_cache_key: cacheKey(key) } : {};
    const pinned = await pinnedHost(db, owner, fields.model);
    if (pinned !== null) {
      return { body: JSON.stringify({ ...fields, ...changes, provider: pinnedProvider(fields.provider, pinned) }) };
    }
    const features = await chatFeatures(db, owner);
    if (!features.openRouterCheapest) {
      return { body: JSON.stringify({ ...fields, ...changes }) };
    }
    const conversation = { model: fields.model, key };
    const exclude = await blockedHosts(db, owner, credentialId, fields.model);
    const order = (await deps.openRouter?.order(fields.model, { noFourBit: features.openRouterNoFourBit, exclude }, key)) ?? null;
    const provider = cheapestProvider(fields.provider, order);
    return { body: JSON.stringify({ ...fields, ...changes, ...(provider ? { provider } : {}) }), conversation };
  }
}

/** The body as JSON, or null if it is none (the provider answers that). */
function parseJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

/**
 * Names an agent's conversation by its first user message: the request
 * carries no session, but every call of a conversation starts the same way,
 * and two conversations rarely do.
 *
 * @param device - The device the agent runs on.
 * @param messages - The request's messages.
 * @returns The name.
 */
export function conversationKey(device: string, messages: unknown): string {
  const first = Array.isArray(messages) ? messages.find((message) => (message as { role?: unknown } | null)?.role === 'user') : undefined;
  return `${device}:${createHash('sha256').update(JSON.stringify(first ?? null)).digest('hex').slice(0, 32)}`;
}
