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

/** Probes at a time; a model has up to some forty hosts. */
const CONCURRENCY = 6;

/** One probe waits at most this long. */
const PROBE_TIMEOUT_MS = 20_000;

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
 * Probes the hosts of a model for a credential, a few at a time; a second
 * request for the same credential and model while one runs waits for it.
 * What the probes found is stored by the caller (`services/host-access.ts`).
 */
export class HostProber {
  readonly #running = new Map<string, Promise<Map<string, HostAccessState>>>();

  /**
   * @param probe - Asks about one host; tests pass a stand-in.
   */
  constructor(private readonly probe: HostProbe = probeHost) {}

  /**
   * The state of each host for a credential.
   *
   * @param credential - The OpenRouter credential: its id, key and API URL.
   * @param model - OpenRouter's model id.
   * @param hosts - Slugs of the model's hosts.
   * @returns State per slug.
   */
  async check(credential: { id: string; apiKey: string; baseUrl: string }, model: string, hosts: string[]): Promise<Map<string, HostAccessState>> {
    const key = `${credential.id}\n${model}`;
    let running = this.#running.get(key);
    if (!running) {
      running = this.#probeAll(credential, model, hosts).finally(() => this.#running.delete(key));
      this.#running.set(key, running);
    }
    return running;
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
