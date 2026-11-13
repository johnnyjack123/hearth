import { type ChatChange, CLIENT_ID_HEADER, type LiveEvent } from '@hearth/shared';
import type { Context } from 'hono';

type Listener = (event: LiveEvent) => void;

/** What a page may call itself; anything else counts as no name. */
const CLIENT_ID = /^[\w-]{1,64}$/;

/**
 * Tells every open page of a user what changed, so a chat open on the phone
 * and on the laptop shows the same. In memory like the device hub: a restart
 * drops the connections, and the pages reconnect and reload what they show.
 */
export class LiveEvents {
  readonly #listeners = new Map<string, Set<Listener>>();

  /**
   * Starts sending a user's events to a listener.
   *
   * @param userId - Whose events.
   * @param listener - Called with each event.
   * @returns A function that stops it.
   */
  subscribe(userId: string, listener: Listener): () => void {
    let listeners = this.#listeners.get(userId);
    if (!listeners) {
      listeners = new Set();
      this.#listeners.set(userId, listeners);
    }
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
      if (listeners.size === 0 && this.#listeners.get(userId) === listeners) {
        this.#listeners.delete(userId);
      }
    };
  }

  /**
   * Sends an event to all open pages of a user.
   *
   * @param userId - Whose pages.
   * @param event - What changed.
   */
  publish(userId: string, event: LiveEvent): void {
    for (const listener of this.#listeners.get(userId) ?? []) {
      listener(event);
    }
  }

  /**
   * Says that the messages of a chat changed.
   *
   * @param userId - Owner of the chat.
   * @param conversationId - The chat.
   * @param change - What happened.
   * @param source - The page that caused it, if it said (see {@link clientSource}).
   */
  chat(userId: string, conversationId: string, change: ChatChange, source?: string): void {
    this.publish(userId, { type: 'chat', id: conversationId, change, ...(source ? { source } : {}) });
  }

  /**
   * Says that the chat list changed: a new chat, a title, tags.
   *
   * @param userId - Owner of the chats.
   * @param source - The page that caused it, if it said.
   */
  list(userId: string, source?: string): void {
    this.publish(userId, { type: 'list', ...(source ? { source } : {}) });
  }

  /**
   * How many pages are connected, for tests.
   *
   * @param userId - Whose pages.
   * @returns The number of listeners.
   */
  connections(userId: string): number {
    return this.#listeners.get(userId)?.size ?? 0;
  }
}

/**
 * The page a request comes from, as it named itself; its own events it
 * already shows.
 *
 * @param c - The request.
 * @returns The page's id, or undefined.
 */
export function clientSource(c: Context): string | undefined {
  const id = c.req.header(CLIENT_ID_HEADER);
  return id && CLIENT_ID.test(id) ? id : undefined;
}
