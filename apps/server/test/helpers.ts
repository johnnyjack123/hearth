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
import { createDatabase, type Database, type DatabaseKind } from '../src/db/index.ts';
import { migrateToLatest } from '../src/db/migrations.ts';
import { type LlmGateway, ProviderRequestError } from '../src/llm.ts';
import { DeviceHub } from '../src/tunnel/hub.ts';

/**
 * Dialects to run integration tests against. Postgres is included when
 * `TEST_POSTGRES_URL` points at a disposable database.
 */
export const DIALECTS: DatabaseKind[] = process.env.TEST_POSTGRES_URL ? ['sqlite', 'postgres'] : ['sqlite'];

/**
 * Origin used for all test requests; must be trusted for Better Auth and CSRF.
 */
export const ORIGIN = 'http://localhost:8080';

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
   * @param options - JSON body, session cookie and extra headers (an empty
   *   value removes a default header).
   * @returns The response.
   */
  request(
    method: string,
    url: string,
    options?: { body?: unknown; cookie?: string; headers?: Record<string, string> },
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
 * @returns The running test server.
 */
export async function createTestServer(kind: DatabaseKind): Promise<TestServer> {
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
  };

  const database = createDatabase(databaseUrl);
  if (kind === 'postgres') {
    await resetPostgres(database);
  }
  await migrateAuth(config, database);
  await migrateToLatest(database.db);
  const auth = createAuth(config, database);

  const llm = createMockLlm();
  const hub = new DeviceHub();
  const app = createApp({ config, database, auth, secretBox: createSecretBox(config.secret), llm, hub });

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
    return Promise.resolve(
      app.request(url, {
        method,
        headers,
        ...(options.body !== undefined ? { body: JSON.stringify(options.body) } : {}),
      }),
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
