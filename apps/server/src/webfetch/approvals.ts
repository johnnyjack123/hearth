import { randomInt } from 'node:crypto';
import type { ApprovalDecision, WebFetchAnswer } from '@harness/shared';

/**
 * How an open question ended for the tool that asked: an answer, or the
 * chat request was stopped while it waited.
 */
export type QuestionOutcome = WebFetchAnswer | 'aborted';

/**
 * Open web fetch questions, waiting for an answer from the chat. They live
 * in memory: a question only makes sense while its chat request runs, and
 * each one is removed when answered, timed out or aborted.
 */
export class FetchApprovals {
  readonly #open = new Map<string, Map<number, (decision: ApprovalDecision) => void>>();

  /**
   * Opens a question for a conversation.
   *
   * @param conversationId - Conversation whose request asks.
   * @param options - How long to wait, and the request's abort signal.
   * @returns The question id (random, so it cannot be guessed or confused
   *   with a gateway question) and the outcome once it is known.
   */
  ask(
    conversationId: string,
    options: { timeoutMs: number; signal?: AbortSignal },
  ): { id: number; outcome: Promise<QuestionOutcome> } {
    const id = randomInt(1, 2 ** 47);
    const questions = this.#open.get(conversationId) ?? new Map<number, (decision: ApprovalDecision) => void>();
    this.#open.set(conversationId, questions);
    const outcome = new Promise<QuestionOutcome>((resolve) => {
      const finish = (value: QuestionOutcome) => {
        clearTimeout(timer);
        options.signal?.removeEventListener('abort', onAbort);
        questions.delete(id);
        if (questions.size === 0 && this.#open.get(conversationId) === questions) {
          this.#open.delete(conversationId);
        }
        resolve(value);
      };
      const onAbort = () => finish('aborted');
      const timer = setTimeout(() => finish('timeout'), options.timeoutMs);
      timer.unref();
      questions.set(id, (decision) => finish(decision === 'once' || decision === 'session' ? decision : 'deny'));
      if (options.signal?.aborted) {
        finish('aborted');
      } else {
        options.signal?.addEventListener('abort', onAbort, { once: true });
      }
    });
    return { id, outcome };
  }

  /**
   * Whether a question is open.
   *
   * @param conversationId - Conversation.
   * @param id - Question id.
   * @returns True while it waits for an answer.
   */
  has(conversationId: string, id: number): boolean {
    return this.#open.get(conversationId)?.has(id) ?? false;
  }

  /**
   * Answers an open question.
   *
   * @param conversationId - Conversation, already checked for ownership.
   * @param id - Question id.
   * @param decision - The user's answer; anything but `once` and `session` means no.
   * @returns False if no such question is open.
   */
  decide(conversationId: string, id: number, decision: ApprovalDecision): boolean {
    const answer = this.#open.get(conversationId)?.get(id);
    answer?.(decision);
    return answer !== undefined;
  }

  /** Number of open questions over all conversations. */
  get size(): number {
    let count = 0;
    for (const questions of this.#open.values()) {
      count += questions.size;
    }
    return count;
  }
}
