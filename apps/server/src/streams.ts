/**
 * Replies that are still being written, so a page that reloads or reopens
 * the chat can pick them up again instead of losing them. Closing the page
 * no longer stops a reply; only the stop button does (`stop`).
 *
 * In memory, like the device hub: a restart of the server ends every reply
 * anyway. One reply per conversation; a new request replaces the old one.
 */

/** A reply being written: what was sent so far, and who waits for more. */
interface ActiveStream {
  /** SSE text as sent to the browser, in order. */
  chunks: string[];
  done: boolean;
  /** Called on every new chunk and at the end. */
  listeners: Set<() => void>;
  /** Ends the work behind the reply (the model call or the agent). */
  stop: () => void;
}

/**
 * Registry of the replies being written, by conversation id.
 */
export class ActiveStreams {
  readonly #streams = new Map<string, ActiveStream>();

  /**
   * Starts following a reply. Reads `stream` to its end in any case, which
   * keeps the reply going (and stored) when the browser goes away.
   *
   * @param conversationId - Conversation of the reply.
   * @param stream - Copy of the SSE stream sent to the browser.
   * @param stop - Ends the work behind the reply.
   */
  add(conversationId: string, stream: ReadableStream<string>, stop: () => void): void {
    this.#streams.get(conversationId)?.stop();
    const active: ActiveStream = { chunks: [], done: false, listeners: new Set(), stop };
    this.#streams.set(conversationId, active);
    const notify = () => {
      for (const listener of active.listeners) {
        listener();
      }
    };
    void (async () => {
      const reader = stream.getReader();
      try {
        for (;;) {
          const { done, value } = await reader.read();
          if (done) {
            break;
          }
          active.chunks.push(value);
          notify();
        }
      } catch {
        // The reply failed or was cancelled; its end is all that matters here.
      } finally {
        active.done = true;
        // A newer reply may have taken the place already.
        if (this.#streams.get(conversationId) === active) {
          this.#streams.delete(conversationId);
        }
        notify();
      }
    })();
  }

  /**
   * @returns Conversations whose reply is being written right now.
   */
  running(): string[] {
    return [...this.#streams].filter(([, active]) => !active.done).map(([id]) => id);
  }

  /**
   * Whether a reply is being written for the conversation.
   *
   * @param conversationId - Conversation id.
   * @returns True while the reply runs.
   */
  has(conversationId: string): boolean {
    return this.#streams.has(conversationId);
  }

  /**
   * The running reply from its start: first what was sent so far, then the
   * rest as it comes.
   *
   * @param conversationId - Conversation id.
   * @returns SSE text, or null if no reply is being written.
   */
  resume(conversationId: string): ReadableStream<string> | null {
    const active = this.#streams.get(conversationId);
    if (!active) {
      return null;
    }
    let next = 0;
    let wake: (() => void) | null = null;
    const listener = () => {
      wake?.();
    };
    active.listeners.add(listener);
    return new ReadableStream<string>({
      async pull(controller) {
        while (next >= active.chunks.length && !active.done) {
          await new Promise<void>((resolve) => {
            wake = resolve;
          });
          wake = null;
        }
        if (next < active.chunks.length) {
          controller.enqueue(active.chunks[next] as string);
          next += 1;
          return;
        }
        active.listeners.delete(listener);
        controller.close();
      },
      cancel() {
        active.listeners.delete(listener);
      },
    });
  }

  /**
   * Stops the reply of a conversation, as the stop button asks.
   *
   * @param conversationId - Conversation id.
   * @returns False if no reply was running.
   */
  stop(conversationId: string): boolean {
    const active = this.#streams.get(conversationId);
    if (!active) {
      return false;
    }
    active.stop();
    return true;
  }
}
