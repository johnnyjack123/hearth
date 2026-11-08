import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import type { ModelInfo } from '@harness/shared';
import { simulateReadableStream } from 'ai';
import { MockLanguageModelV4 } from 'ai/test';
import { sql } from 'kysely';
import { createApp } from '../src/app.ts';
import { createAuth, migrateAuth } from '../src/auth.ts';
import type { ServerConfig } from '../src/config.ts';
import { createSecretBox } from '../src/crypto.ts';
import { parseTrustedProxies } from '../src/http/client-ip.ts';
import { SETUP_CODE_HEADER } from '../src/routes/auth.ts';
import { createDatabase, type Database, type DatabaseKind } from '../src/db/index.ts';
import { migrateToLatest } from '../src/db/migrations.ts';
import type { DocumentConverter } from '../src/files/convert.ts';
import { type LlmGateway, ProviderRequestError } from '../src/llm.ts';
import { PricingCatalog } from '../src/pricing.ts';
import { createDeviceHub } from '../src/server.ts';
import { OpenRouterRouting } from '../src/openrouter.ts';
import { HostAccess } from '../src/openrouter-access.ts';
import type { DeviceHub } from '../src/tunnel/hub.ts';
import type { WebFetchConfig } from '../src/webfetch/tool.ts';
import type { SearchEndpoints } from '../src/websearch/providers.ts';
import type { HelperOptions } from '../src/helpers/load.ts';

/**
 * Dialects to run integration tests against. Postgres is included when
 * `TEST_POSTGRES_URL` points at a disposable database.
 */
export const DIALECTS: DatabaseKind[] = process.env.TEST_POSTGRES_URL ? ['sqlite', 'postgres'] : ['sqlite'];

/**
 * Origin used for all test requests; must be trusted for Better Auth and CSRF.
 */
export const ORIGIN = 'http://localhost:8080';

/** Setup code of every test server; {@link TestServer.signUp} sends it. */
export const SETUP_CODE = 'TEST-CODE';

const usage = {
  inputTokens: { total: 5, noCache: 5, cacheRead: undefined, cacheWrite: undefined },
  outputTokens: { total: 7, text: 7, reasoning: undefined },
};

/**
 * LLM gateway that answers with fixed text and records which models were used.
 */
export interface MockLlm extends LlmGateway {
  /** Model ids passed to `createModel`, in call order. */
  createdModels: string[];
  /** When set, `listModels` throws this message. */
  listError: string | null;
}

/**
 * Creates a {@link MockLlm}.
 *
 * @param reply - Text the model streams, split into words.
 * @param title - Text returned for title generation.
 * @returns The mock gateway.
 */
export function createMockLlm(reply = 'Hello from the mock model', title = 'Mock Title'): MockLlm {
  const mock: MockLlm = {
    createdModels: [],
    listError: null,
    createModel(_credential, modelId) {
      mock.createdModels.push(modelId);
      return new MockLanguageModelV4({
        doGenerate: async () => ({
          content: [{ type: 'text', text: title }],
          finishReason: { unified: 'stop', raw: undefined },
          usage,
          warnings: [],
        }),
        doStream: async () => ({
          stream: simulateReadableStream({
            chunks: [
              { type: 'text-start', id: 't1' },
              ...reply.split(' ').map((word, index) => ({
                type: 'text-delta' as const,
                id: 't1',
                delta: index === 0 ? word : ` ${word}`,
              })),
              { type: 'text-end', id: 't1' },
              { type: 'finish', finishReason: { unified: 'stop', raw: undefined }, usage },
            ],
          }),
        }),
      });
    },
    async listModels(): Promise<ModelInfo[]> {
      if (mock.listError) {
        throw new ProviderRequestError(mock.listError);
      }
      return [{ id: 'mock-1', name: 'Mock One' }];
    },
    invalidate() {},
  };
  return mock;
}

/**
 * A running app instance backed by a fresh database.
 */
export interface TestServer {
  /** The open database, for asserting on stored rows. */
  database: Database;
  /** The mock LLM used by the app. */
  llm: MockLlm;
  /** Device tunnels; tests attach fake sockets. */
  hub: DeviceHub;
  /** The app itself, e.g. to serve it on a real port. */
  app: ReturnType<typeof createApp>;
  /**
   * Sends a request to the app with the test origin and an optional session.
   *
   * @param method - HTTP method.
   * @param url - Path, e.g. `/api/conversations`.
   * @param options - JSON body, session cookie, extra headers (an empty
   *   value removes a default header) and the address of the connection
   *   (none by default, as `app.request` has no socket).
   * @returns The response.
   */
  request(
    method: string,
    url: string,
    options?: { body?: unknown; cookie?: string; headers?: Record<string, string>; peer?: string },
  ): Promise<Response>;
  /**
   * Registers an account and returns its session cookie.
   *
   * @param email - Account email.
   * @returns The `Cookie` header value for subsequent requests.
   * @throws {Error} If sign-up fails.
   */
  signUp(email: string): Promise<string>;
  /** Closes the database. */
  close(): Promise<void>;
}

async function resetPostgres(database: Database): Promise<void> {
  await sql`drop schema if exists public cascade`.execute(database.db);
  await sql`create schema public`.execute(database.db);
}

/**
 * Starts the app on a fresh database of the given dialect.
 *
 * @param kind - Database dialect.
 * @param options - Another LLM gateway, web fetch overrides (e.g. an
 *   address filter that lets a local test server through) and search
 *   endpoints (local stand-ins of Brave and Tavily).
 * @returns The running test server.
 */
export async function createTestServer(
  kind: DatabaseKind,
  options: {
    llm?: MockLlm;
    webFetch?: Partial<WebFetchConfig>;
    webSearch?: Partial<SearchEndpoints>;
    helpers?: HelperOptions;
    converter?: DocumentConverter | null;
    openRouter?: OpenRouterRouting;
    hostAccess?: HostAccess;
  } = {},
): Promise<TestServer> {
  const dataDir = mkdtempSync(path.join(tmpdir(), 'harness-test-'));
  const databaseUrl = kind === 'postgres' ? (process.env.TEST_POSTGRES_URL as string) : 'sqlite::memory:';
  const config: ServerConfig = {
    host: '127.0.0.1',
    port: 0,
    publicUrl: ORIGIN,
    dataDir,
    databaseUrl,
    secret: 'test-secret-that-is-long-enough-for-hkdf-0123456789',
    trustedOrigins: [ORIGIN],
    webDist: null,
    trustedProxies: parseTrustedProxies('10.0.0.0/8'),
    setupCode: SETUP_CODE,
  };

  const database = createDatabase(databaseUrl);
  if (kind === 'postgres') {
    await resetPostgres(database);
  }
  await migrateAuth(config, database);
  await migrateToLatest(database.db);
  const auth = createAuth(config, database);

  const llm = options.llm ?? createMockLlm();
  const hub = createDeviceHub(database.db);
  // A tiny catalog instead of models.dev: tests never go online.
  const pricing = new PricingCatalog(async () => ({
    openai: {
      models: {
        'mock-1': { limit: { context: 1000 }, cost: { input: 2, output: 8 } },
        // A model the catalog says cannot see images.
        'mock-blind': { modalities: { input: ['text'] } },
      },
    },
  }));
  const app = createApp({
    config,
    database,
    auth,
    secretBox: createSecretBox(config.secret),
    llm,
    hub,
    pricing,
    ...(options.webFetch ? { webFetch: options.webFetch } : {}),
    ...(options.webSearch ? { webSearch: options.webSearch } : {}),
    ...(options.helpers ? { helpers: options.helpers } : {}),
    // Documents need a converter; tests without one say so explicitly.
    converter: options.converter ?? null,
    // No endpoint lists unless a test brings some: tests never go online.
    openRouter: options.openRouter ?? new OpenRouterRouting(async () => null, Date.now, async () => null),
    // Nor probes OpenRouter for the account's hosts.
    hostAccess: options.hostAccess ?? new HostAccess(async () => 'unknown'),
  });

  const request: TestServer['request'] = (method, url, options = {}) => {
    const headers: Record<string, string> = { origin: ORIGIN };
    if (options.cookie) {
      headers.cookie = options.cookie;
    }
    if (options.body !== undefined) {
      headers['content-type'] = 'application/json';
    }
    // An empty value removes a default header, e.g. `origin` for CLI clients.
    for (const [name, value] of Object.entries(options.headers ?? {})) {
      if (value) {
        headers[name] = value;
      } else {
        delete headers[name];
      }
    }
    // Hono hands the third argument to routes as `c.env`, like the Node
    // server does with its socket.
    const env = options.peer ? { incoming: { socket: { remoteAddress: options.peer } } } : undefined;
    return Promise.resolve(
      app.request(
        url,
        {
          method,
          headers,
          ...(options.body !== undefined ? { body: JSON.stringify(options.body) } : {}),
        },
        env,
      ),
    );
  };

  return {
    database,
    llm,
    hub,
    app,
    request,
    async signUp(email) {
      const response = await request('POST', '/api/auth/sign-up/email', {
        body: { email, name: email.split('@')[0], password: 'correct-horse-battery' },
        headers: { [SETUP_CODE_HEADER]: SETUP_CODE },
      });
      if (!response.ok) {
        throw new Error(`sign-up failed: ${response.status} ${await response.text()}`);
      }
      return response.headers
        .getSetCookie()
        .map((cookie) => cookie.split(';')[0])
        .join('; ');
    },
    close: () => database.close(),
  };
}

/**
 * Waits until a condition holds, for effects that finish after the response
 * (message persistence in `onEnd`, background title generation).
 *
 * @param condition - Check to repeat.
 * @param timeoutMs - Maximum wait.
 * @throws {Error} If the condition does not hold in time.
 */
export async function eventually(condition: () => Promise<boolean>, timeoutMs = 3_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await condition()) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error('condition not met in time');
}

const scriptedUsage = {
  inputTokens: { total: 5, noCache: 5, cacheRead: undefined, cacheWrite: undefined },
  outputTokens: { total: 7, text: 7, reasoning: undefined },
};

export type Prompt = { role: string; content: unknown }[];
/** One model call: a tool call, or the answer, optionally with a large prompt. */
export type Step = { tool: string; input: Record<string, unknown> } | { text: string; inputTokens?: number };

/**
 * A model that follows a script after each user message: one step per
 * model call, counted by the tool results since that message.
 */
export function createScriptedLlm(script: () => Step[]): MockLlm & { calls: { prompt: Prompt; tools: string[]; system: string }[] } {
  const base = createMockLlm();
  const calls: { prompt: Prompt; tools: string[]; system: string }[] = [];
  return Object.assign(base, {
    calls,
    createModel(_credential: unknown, modelId: string) {
      base.createdModels.push(modelId);
      return new MockLanguageModelV4({
        doGenerate: async () => ({
          content: [{ type: 'text', text: 'Titel' }],
          finishReason: { unified: 'stop', raw: undefined },
          usage: scriptedUsage,
          warnings: [],
        }),
        doStream: async (options) => {
          const prompt = options.prompt as Prompt;
          const tools = (options.tools ?? []).map((tool) => tool.name);
          const system = prompt.filter((message) => message.role === 'system').map((message) => String(message.content)).join(' ');
          calls.push({ prompt, tools, system });
          const lastUser = prompt.map((message) => message.role).lastIndexOf('user');
          const done = prompt.slice(lastUser).filter((message) => message.role === 'tool').length;
          const step = script()[done] ?? { text: 'fertig' };
          const chunks =
            'tool' in step
              ? [
                  { type: 'tool-call', toolCallId: `call-${calls.length}`, toolName: step.tool, input: JSON.stringify(step.input) },
                  { type: 'finish', finishReason: { unified: 'tool-calls', raw: undefined }, usage: scriptedUsage },
                ]
              : [
                  { type: 'text-start', id: 't1' },
                  { type: 'text-delta', id: 't1', delta: step.text },
                  { type: 'text-end', id: 't1' },
                  {
                    type: 'finish',
                    finishReason: { unified: 'stop', raw: undefined },
                    usage: step.inputTokens
                      ? { ...scriptedUsage, inputTokens: { total: step.inputTokens, noCache: step.inputTokens, cacheRead: undefined, cacheWrite: undefined } }
                      : scriptedUsage,
                  },
                ];
          return { stream: simulateReadableStream({ chunks: chunks as never[] }) };
        },
      });
    },
  });
}

/** Reads a streamed reply to its end and returns the UI message chunks. */
export async function readChunks(response: Response, onChunk?: (chunk: { type: string; [key: string]: unknown }) => void) {
  const chunks: { type: string; [key: string]: unknown }[] = [];
  const reader = response.body?.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  for (;;) {
    const { done, value } = (await reader?.read()) ?? { done: true, value: undefined };
    if (done) {
      return chunks;
    }
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop() ?? '';
    for (const line of lines) {
      if (line.startsWith('data: ') && line !== 'data: [DONE]') {
        const chunk = JSON.parse(line.slice('data: '.length)) as { type: string };
        chunks.push(chunk);
        onChunk?.(chunk);
      }
    }
  }
}
