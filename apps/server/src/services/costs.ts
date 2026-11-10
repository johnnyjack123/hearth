/**
 * The daily cost limit: what each user spent today (`costDay`, counted
 * when a reply is stored, see `saveMessage`), plus what replies running
 * right now have cost so far, against the lower of the admin's limit for
 * every user and the user's own.
 *
 * Counted are the replies of the web chat (chat models, agents, helper
 * calls). `hearth run` in a terminal goes past the chat and is not counted;
 * once the limit is reached, its model calls are refused as well.
 */
import type { CostStatus } from '@hearth/shared';
import type { Kysely, Transaction } from 'kysely';
import type { DB } from '../db/schema.ts';
import { HttpError } from '../http/errors.ts';
import { getServerSettings } from './settings.ts';

/** Costs are kept in millionths of a dollar: whole numbers, the same in SQLite and Postgres. */
const MICRO = 1_000_000;

/**
 * The day costs count towards: the server's date, as for the chat's notes
 * (in Docker UTC unless `TZ` is set).
 *
 * @param now - The moment.
 * @returns YYYY-MM-DD.
 */
export function costDay(now: Date): string {
  return new Intl.DateTimeFormat('en-CA', { year: 'numeric', month: '2-digit', day: '2-digit' }).format(now);
}

/**
 * Adds to what a user spent today.
 *
 * @param db - Open database or the transaction storing the reply.
 * @param userId - The user.
 * @param dollars - The amount; nothing happens for 0 or less.
 * @param now - When it was spent.
 */
export async function addCost(db: Kysely<DB> | Transaction<DB>, userId: string, dollars: number, now = new Date()): Promise<void> {
  const micro = Math.round(dollars * MICRO);
  if (!Number.isFinite(micro) || micro <= 0) {
    return;
  }
  await db
    .insertInto('costDay')
    .values({ userId, day: costDay(now), microdollars: micro })
    .onConflict((oc) => oc.columns(['userId', 'day']).doUpdateSet((eb) => ({ microdollars: eb('costDay.microdollars', '+', eb.ref('excluded.microdollars')) })))
    .execute();
}

/**
 * @param db - Open database.
 * @param userId - The user.
 * @param now - Which day.
 * @returns Dollars spent on that day, stored replies only.
 */
export async function spentOn(db: Kysely<DB>, userId: string, now = new Date()): Promise<number> {
  const row = await db.selectFrom('costDay').select('microdollars').where('userId', '=', userId).where('day', '=', costDay(now)).executeTakeFirst();
  // Postgres returns bigint as a string.
  return row ? Number(row.microdollars) / MICRO : 0;
}

/**
 * @param db - Open database.
 * @param userId - The user.
 * @returns The user's own daily limit in dollars, or null for none.
 */
export async function ownLimit(db: Kysely<DB>, userId: string): Promise<number | null> {
  const row = await db.selectFrom('userLimit').select('dailyMicrodollars').where('userId', '=', userId).executeTakeFirst();
  return row?.dailyMicrodollars === null || row?.dailyMicrodollars === undefined ? null : Number(row.dailyMicrodollars) / MICRO;
}

/**
 * Sets the user's own daily limit.
 *
 * @param db - Open database.
 * @param userId - The user.
 * @param dollars - The limit, or null for none.
 */
export async function setOwnLimit(db: Kysely<DB>, userId: string, dollars: number | null): Promise<void> {
  const value = { dailyMicrodollars: dollars === null ? null : Math.round(dollars * MICRO), updatedAt: Date.now() };
  await db
    .insertInto('userLimit')
    .values({ userId, ...value })
    .onConflict((oc) => oc.column('userId').doUpdateSet(value))
    .execute();
}

/** Formats dollars for the English error message; the web app shows its own text. */
function usd(dollars: number): string {
  return `$${dollars.toFixed(dollars < 1 ? 4 : 2)}`;
}

/**
 * The limit and what counts against it. One per server: replies running
 * now report their cost as they go, so a long agent reply stops at the limit
 * and not only the next message.
 */
export class CostGuard {
  /** Running replies by chat: whose they are and what they cost so far. */
  readonly #running = new Map<string, { userId: string; dollars: number }>();

  /**
   * @param db - Open database.
   */
  constructor(private readonly db: Kysely<DB>) {}

  /**
   * Notes what a running reply has cost so far.
   *
   * @param conversationId - Its chat.
   * @param userId - Whose it is.
   * @param dollars - Its cost so far.
   */
  report(conversationId: string, userId: string, dollars: number): void {
    this.#running.set(conversationId, { userId, dollars });
  }

  /**
   * Forgets a reply once it is stored; its cost is in `costDay` then.
   *
   * @param conversationId - Its chat.
   */
  end(conversationId: string): void {
    this.#running.delete(conversationId);
  }

  /**
   * What a user spent today, running replies included, and the limit.
   *
   * @param userId - The user.
   * @param now - The moment.
   * @returns See {@link CostStatus}.
   */
  async status(userId: string, now = new Date()): Promise<CostStatus> {
    const [stored, own, settings] = await Promise.all([spentOn(this.db, userId, now), ownLimit(this.db, userId), getServerSettings(this.db)]);
    let running = 0;
    for (const reply of this.#running.values()) {
      if (reply.userId === userId) {
        running += reply.dollars;
      }
    }
    const server = settings.dailyCostLimit;
    const limits = [own, server].filter((limit): limit is number => limit !== null);
    return { day: costDay(now), spent: stored + running, limit: limits.length > 0 ? Math.min(...limits) : null, own, server };
  }

  /**
   * Refuses when the user reached the limit.
   *
   * @param userId - The user.
   * @returns The status, for a reply about to start: what is left of the limit.
   * @throws {HttpError} 429 `cost_limit` once today's spending reached the limit.
   */
  async assert(userId: string): Promise<CostStatus> {
    const status = await this.status(userId);
    if (status.limit !== null && status.spent >= status.limit) {
      throw new HttpError(429, 'cost_limit', `Daily cost limit of ${usd(status.limit)} reached (${usd(status.spent)} spent on ${status.day})`);
    }
    return status;
  }
}

const guards = new WeakMap<Kysely<DB>, CostGuard>();

/**
 * The cost guard of a database, one per server.
 *
 * @param db - Open database.
 * @returns The guard.
 */
export function costGuard(db: Kysely<DB>): CostGuard {
  let guard = guards.get(db);
  if (!guard) {
    guard = new CostGuard(db);
    guards.set(db, guard);
  }
  return guard;
}
