/**
 * Which of a model's OpenRouter hosts an account may use. Users can allow or
 * ignore providers in their OpenRouter settings (and data policies or
 * guardrails rule out more), but no API with an ordinary key says which. A
 * tiny request pinned to one host answers it: OpenRouter refuses with 404
 * before any host sees it when the account rules the host out.
 */

/** Whether an account may use a host. */
export type HostAccessState = 'allowed' | 'blocked' | 'unknown';

/** Asks OpenRouter whether a key may use one host for a model. */
export type HostProbe = (request: { apiKey: string; baseUrl: string; model: string; host: string }) => Promise<HostAccessState>;

/** A result is shown again without asking for this long. */
const CHECK_TTL_MS = 15 * 60_000;

/** Routing leaves out blocked hosts while the result is younger than this. */
const KEEP_MS = 6 * 60 * 60_000;

/** Probes at a time; a model has up to some forty hosts. */
const CONCURRENCY = 6;

/** One probe waits at most this long. */
const PROBE_TIMEOUT_MS = 20_000;

/** Results kept at most (credential and model). */
const MAX_RESULTS = 1_000;

/**
 * One probe: a one-token request that only this host may serve. Hosts the
 * account allows cost a few tokens (fractions of a cent); the others are
 * refused by OpenRouter and cost nothing.
 *
 * @param request - Key, OpenRouter's API URL, model and host slug.
 * @returns What the answer says about the host.
 */
export const probeHost: HostProbe = async ({ apiKey, baseUrl, model, host }) => {
  let response: Response;
  try {
    response = await fetch(`${baseUrl}/chat/completions`, {
      method: 'POST',
      headers: { authorization: `Bearer ${apiKey}`, 'content-type': 'application/json' },
      body: JSON.stringify({
        model,
        messages: [{ role: 'user', content: 'Hi' }],
        max_tokens: 1,
        stream: false,
        provider: { only: [host], allow_fallbacks: false },
      }),
      signal: AbortSignal.timeout(PROBE_TIMEOUT_MS),
      redirect: 'error',
    });
  } catch {
    return 'unknown';
  }
  return accessFromAnswer(response.status, await response.json().catch(() => null));
};

/**
 * Reads a probe's answer. 404 is OpenRouter finding no endpoint the account
 * may use ("No allowed providers are available for the selected model");
 * an error the host itself raised (it rejected a parameter, it is
 * overloaded) still means the request got there.
 *
 * @param status - HTTP status.
 * @param body - The answer as JSON, null if it was none.
 * @returns The host's state for the account.
 */
export function accessFromAnswer(status: number, body: unknown): HostAccessState {
  if (status >= 200 && status < 300) {
    return 'allowed';
  }
  if (status === 404) {
    return 'blocked';
  }
  const metadata = (body as { error?: { metadata?: { provider_name?: unknown } } } | null)?.error?.metadata;
  return typeof metadata?.provider_name === 'string' ? 'allowed' : 'unknown';
}

/**
 * Probes and remembers which hosts each OpenRouter credential may use.
 */
export class HostAccess {
  readonly #results = new Map<string, { at: number; states: Map<string, HostAccessState> }>();
  readonly #running = new Map<string, Promise<Map<string, HostAccessState>>>();

  /**
   * @param probe - Asks about one host; tests pass a stand-in.
   * @param now - The clock, for tests.
   */
  constructor(
    private readonly probe: HostProbe = probeHost,
    private readonly now: () => number = Date.now,
  ) {}

  /**
   * The state of each host for a credential, probed unless a recent result
   * covers them all.
   *
   * @param credential - The OpenRouter credential: its id, key and API URL.
   * @param model - OpenRouter's model id.
   * @param hosts - Slugs of the model's hosts.
   * @returns State per slug.
   */
  async check(credential: { id: string; apiKey: string; baseUrl: string }, model: string, hosts: string[]): Promise<Map<string, HostAccessState>> {
    const key = `${credential.id}\n${model}`;
    const known = this.#results.get(key);
    if (known && this.now() - known.at < CHECK_TTL_MS && hosts.every((host) => known.states.has(host))) {
      return known.states;
    }
    let running = this.#running.get(key);
    if (!running) {
      running = this.#probeAll(credential, model, hosts)
        .then((states) => {
          this.#results.delete(key);
          this.#results.set(key, { at: this.now(), states });
          for (const oldest of this.#results.keys()) {
            if (this.#results.size <= MAX_RESULTS) {
              break;
            }
            this.#results.delete(oldest);
          }
          return states;
        })
        .finally(() => this.#running.delete(key));
      this.#running.set(key, running);
    }
    return running;
  }

  /**
   * Hosts a credential may not use, from a recent result only; never probes,
   * so requests do not wait for it.
   *
   * @param credentialId - The OpenRouter credential.
   * @param model - OpenRouter's model id.
   * @returns Their slugs; empty when nothing is known.
   */
  blocked(credentialId: string, model: string): Set<string> {
    const known = this.#results.get(`${credentialId}\n${model}`);
    if (!known || this.now() - known.at >= KEEP_MS) {
      return new Set();
    }
    return new Set([...known.states].filter(([, state]) => state === 'blocked').map(([host]) => host));
  }

  async #probeAll(credential: { apiKey: string; baseUrl: string }, model: string, hosts: string[]): Promise<Map<string, HostAccessState>> {
    const states = new Map<string, HostAccessState>();
    const queue = [...hosts];
    const worker = async () => {
      for (let host = queue.shift(); host !== undefined; host = queue.shift()) {
        states.set(host, await this.probe({ apiKey: credential.apiKey, baseUrl: credential.baseUrl, model, host }).catch(() => 'unknown' as const));
      }
    };
    await Promise.all(Array.from({ length: Math.min(CONCURRENCY, hosts.length) }, worker));
    return states;
  }
}
