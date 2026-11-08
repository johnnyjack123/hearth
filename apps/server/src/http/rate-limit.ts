import { HttpError } from './errors.ts';

/** Keys kept at most; beyond this the oldest windows are dropped. */
const MAX_KEYS = 10_000;

/**
 * Counts requests per key in fixed windows, in memory. Enough for one server
 * process; a restart forgets the counts.
 */
export class RateLimiter {
  readonly #windowMs: number;
  readonly #max: number;
  readonly #now: () => number;
  readonly #windows = new Map<string, { count: number; resetAt: number }>();

  /**
   * @param max - Requests allowed per window.
   * @param windowMs - Length of a window.
   * @param now - Clock, for tests.
   */
  constructor(max: number, windowMs: number, now: () => number = Date.now) {
    this.#max = max;
    this.#windowMs = windowMs;
    this.#now = now;
  }

  /**
   * Counts one request.
   *
   * @param key - Who makes it, usually {@link rateKey} of the client address.
   * @returns Seconds until the next request is allowed, or 0 if this one is.
   */
  take(key: string): number {
    const now = this.#now();
    const window = this.#windows.get(key);
    if (!window || window.resetAt <= now) {
      this.#prune(now);
      this.#windows.set(key, { count: 1, resetAt: now + this.#windowMs });
      return 0;
    }
    if (window.count >= this.#max) {
      return Math.max(1, Math.ceil((window.resetAt - now) / 1000));
    }
    window.count += 1;
    return 0;
  }

  /**
   * Counts one request and rejects it when the key is over its limit.
   *
   * @param key - Who makes it.
   * @throws {HttpError} 429 when over the limit.
   */
  check(key: string): void {
    const wait = this.take(key);
    if (wait > 0) {
      throw new HttpError(429, 'too_many_requests', `Too many requests; try again in ${wait} s`);
    }
  }

  #prune(now: number): void {
    if (this.#windows.size < MAX_KEYS) {
      return;
    }
    for (const [key, window] of this.#windows) {
      if (window.resetAt <= now) {
        this.#windows.delete(key);
      }
    }
    // Map order is insertion order: what is left over the limit is oldest.
    for (const key of this.#windows.keys()) {
      if (this.#windows.size < MAX_KEYS) {
        break;
      }
      this.#windows.delete(key);
    }
  }
}
