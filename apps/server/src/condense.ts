/**
 * Condensing long chats: once the conversation has grown past a threshold,
 * or after a pause long enough for the provider's cache to have gone cold,
 * pages and search results from earlier turns reach the model only in short
 * form. The point where that happens is stored on the user message, so every
 * later request builds exactly the same prompt and the provider's cache
 * keeps working until the chat grows past the threshold again.
 */
import { type CondenseSettings, WEB_FETCH_TOOL, WEB_SEARCH_TOOL } from '@harness/shared';
import type { ChatMessage } from './services/conversations.ts';

/** Less text than this (~2,000 tokens) is not worth voiding the provider's cache for. */
const MIN_CONDENSABLE_CHARS = 8_000;

const FETCH_PART = `tool-${WEB_FETCH_TOOL}`;
const SEARCH_PART = `tool-${WEB_SEARCH_TOOL}`;

type Part = ChatMessage['parts'][number];

function outputOf(part: Part): Record<string, unknown> | null {
  if ((part.type !== FETCH_PART && part.type !== SEARCH_PART) || !('state' in part) || part.state !== 'output-available') {
    return null;
  }
  const { output } = part as { output?: unknown };
  return output && typeof output === 'object' ? (output as Record<string, unknown>) : null;
}

/** Characters a tool result adds to every later model call. */
function outputSize(part: Part): number {
  const output = outputOf(part);
  if (!output) {
    return 0;
  }
  if (part.type === FETCH_PART) {
    return typeof output.text === 'string' ? output.text.length : 0;
  }
  const results = Array.isArray(output.results) ? (output.results as { snippet?: unknown }[]) : [];
  return results.reduce((sum, result) => sum + (typeof result.snippet === 'string' ? result.snippet.length : 0), 0);
}

/** Index of the last user message that condenses what comes before it, or -1. */
function lastMark(messages: ChatMessage[]): number {
  return messages.findLastIndex((message) => message.role === 'user' && message.metadata?.condensed === true);
}

/**
 * Whether a new user message should condense the chat before it: the last
 * reply left a long conversation (see {@link CondenseSettings}) or is old
 * enough for the provider's cache to have gone cold, and enough tool
 * results since the last condensing would shrink.
 *
 * @param previous - The conversation before the new message.
 * @param settings - The thresholds of the server.
 * @param now - The current time, epoch milliseconds.
 * @returns True if the new message gets the mark.
 */
export function shouldCondense(previous: ChatMessage[], settings: CondenseSettings, now = Date.now()): boolean {
  if (!settings.enabled) {
    return false;
  }
  const reply = previous.findLast((message) => message.role === 'assistant');
  const usage = reply?.metadata?.usage;
  // Replies without usage (stopped early, old) are estimated at ~4 characters per token.
  const size = usage?.contextTokens ?? JSON.stringify(previous).length / 4;
  const limit = Math.min(settings.atTokens, usage?.contextWindow ? usage.contextWindow * settings.atWindowShare : Number.POSITIVE_INFINITY);
  const startedAt = reply?.metadata?.createdAt;
  const idle = settings.afterIdleHours > 0 && startedAt !== undefined && now - startedAt >= settings.afterIdleHours * 3_600_000;
  if (size < limit && !idle) {
    return false;
  }
  const since = previous.slice(lastMark(previous) + 1);
  const condensable = since.reduce((sum, message) => sum + message.parts.reduce((total, part) => total + outputSize(part), 0), 0);
  return condensable >= MIN_CONDENSABLE_CHARS;
}

/**
 * Marks the results of web_fetch and web_search before the last condensing
 * mark, so their model text comes out short (`modelText`,
 * `searchModelText`). The stored messages stay as they are; the chat still
 * shows everything.
 *
 * @param messages - The conversation as stored.
 * @returns Copies of the messages, with condensed outputs before the mark.
 */
export function condenseHistory(messages: ChatMessage[]): ChatMessage[] {
  const mark = lastMark(messages);
  if (mark <= 0) {
    return messages;
  }
  return messages.map((message, index) => {
    if (index >= mark || !message.parts.some((part) => outputOf(part))) {
      return message;
    }
    const parts = message.parts.map((part): Part => {
      const output = outputOf(part);
      return output ? ({ ...part, output: { ...output, condensed: true } } as Part) : part;
    });
    return { ...message, parts };
  });
}

/**
 * The reasoning of earlier replies, kept where it pays: DeepSeek expects the
 * reasoning of earlier turns back once a request has tools, and the
 * provider's prompt cache holds the conversation with it. Without it, the
 * first call of every new message read the previous reply at full price
 * (8.11.: 69 % instead of nearly all from the cache). Left out is the
 * reasoning of replies by another model or credential, whose formats and
 * signatures the current provider may reject, and of replies before the
 * last condensing mark, where the chat is long or its cache cold anyway.
 *
 * @param messages - The conversation, after the latest summary.
 * @param current - Model and credential of the request.
 * @returns Copies of the messages without the reasoning that goes.
 */
export function withReasoning(messages: ChatMessage[], current: { modelId: string; credentialId: string }): ChatMessage[] {
  const mark = lastMark(messages);
  return messages.map((message, index) => {
    if (message.role !== 'assistant' || !message.parts.some((part) => part.type === 'reasoning')) {
      return message;
    }
    const own = message.metadata?.modelId === current.modelId && message.metadata.credentialId === current.credentialId;
    return own && index > mark ? message : { ...message, parts: message.parts.filter((part) => part.type !== 'reasoning') };
  });
}

/**
 * URLs of pages whose content was condensed away: the model is told it can
 * load them again, and they do so without asking. The same URL carries
 * nothing new out of the chat.
 *
 * @param messages - The conversation as stored.
 * @returns The requested and the final URL of each condensed page.
 */
export function condensedPages(messages: ChatMessage[]): Set<string> {
  const urls = new Set<string>();
  const mark = lastMark(messages);
  for (const message of messages.slice(0, Math.max(0, mark))) {
    for (const part of message.parts) {
      const output = part.type === FETCH_PART ? outputOf(part) : null;
      if (!output) {
        continue;
      }
      const { input } = part as { input?: { url?: unknown } };
      for (const url of [input?.url, output.url]) {
        try {
          if (typeof url === 'string') {
            urls.add(new URL(url.trim()).href);
          }
        } catch {
          // Not a URL; nothing to load again.
        }
      }
    }
  }
  return urls;
}
