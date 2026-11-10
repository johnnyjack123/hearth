/**
 * When each user last did something in the web app, in memory: what an
 * admin needs to see whether someone is on the server right now, e.g.
 * before an update. A restart forgets it.
 */
export class ActivityTracker {
  readonly #lastSeen = new Map<string, number>();
  readonly #now: () => number;
  /** When the tracker started; nothing before is known. */
  readonly since: number;

  /**
   * @param now - Clock, for tests.
   */
  constructor(now: () => number = Date.now) {
    this.#now = now;
    this.since = now();
  }

  /**
   * Notes a request of a signed-in user.
   *
   * @param userId - The user.
   */
  touch(userId: string): void {
    this.#lastSeen.set(userId, this.#now());
  }

  /**
   * @param userId - The user.
   * @returns Their last request since the start, epoch ms, or null.
   */
  lastSeen(userId: string): number | null {
    return this.#lastSeen.get(userId) ?? null;
  }

  /** Users seen since the start. */
  users(): string[] {
    return [...this.#lastSeen.keys()];
  }
}
