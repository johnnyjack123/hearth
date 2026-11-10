import type { WebFetchErrorCode } from '@hearth/shared';

/**
 * A web fetch that did not happen or failed. The message goes to the model
 * as the tool result and to the chat as the error text, so it starts with a
 * code the web app translates (`[code]` or `[code:detail]`, see
 * `parseWebFetchError`) and continues in English for the model.
 */
export class WebFetchError extends Error {
  /**
   * @param code - Reason, translated by the web app.
   * @param explanation - English text for the model.
   * @param detail - Short value the web app shows, e.g. an HTTP status.
   */
  constructor(
    readonly code: WebFetchErrorCode,
    explanation: string,
    readonly detail?: string,
  ) {
    super(`[${code}${detail ? `:${detail.replace(/[^\w./+-]/g, '').slice(0, 100)}` : ''}] ${explanation}`);
    this.name = 'WebFetchError';
  }
}
