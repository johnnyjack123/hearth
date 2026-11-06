/**
 * The content behind a prompt injection warning, for reading the flagged
 * places in context. The chat shows short excerpts; the user can open the
 * whole page, the search hits or the tool output with every place marked.
 * For web pages the server keeps what the model did not read as well (the
 * whole Markdown and the HTML source, where hidden text and comments live),
 * but only for pages with warnings.
 */
import {
  type AgentToolOutput,
  type InspectedText,
  type Inspection,
  WEB_FETCH_TOOL,
  WEB_SEARCH_TOOL,
  type WebFetchOutput,
  type WebSearchOutput,
} from '@harness/shared';
import type { Kysely } from 'kysely';
import type { DB } from './db/schema.ts';
import type { ChatMessage } from './services/conversations.ts';
import { injectionMarks } from './webfetch/injection.ts';
import type { WarnedPage } from './webfetch/tool.ts';

/** Characters kept of each text; a page and its source stay below a few MB. */
const MAX_KEPT_CHARS = 2_000_000;

/** What `toolContent.content` holds. */
interface StoredToolContent {
  page: string;
  pageCut: boolean;
  source: string | null;
  sourceCut: boolean;
}

/**
 * Keeps the whole of a page with warnings for {@link inspection}.
 *
 * @param db - Open database.
 * @param conversationId - The chat.
 * @param toolCallId - The web fetch.
 * @param content - The page.
 */
export async function keepToolContent(db: Kysely<DB>, conversationId: string, toolCallId: string, content: WarnedPage): Promise<void> {
  const stored: StoredToolContent = {
    page: content.page.slice(0, MAX_KEPT_CHARS),
    pageCut: content.page.length > MAX_KEPT_CHARS,
    source: content.source?.slice(0, MAX_KEPT_CHARS) ?? null,
    sourceCut: (content.source?.length ?? 0) > MAX_KEPT_CHARS,
  };
  const row = { conversationId, toolCallId, content: JSON.stringify(stored), createdAt: Date.now() };
  await db
    .insertInto('toolContent')
    .values(row)
    .onConflict((conflict) => conflict.columns(['conversationId', 'toolCallId']).doUpdateSet({ content: row.content, createdAt: row.createdAt }))
    .execute();
}

async function keptContent(db: Kysely<DB>, conversationId: string, toolCallId: string): Promise<StoredToolContent | null> {
  const row = await db
    .selectFrom('toolContent')
    .select('content')
    .where('conversationId', '=', conversationId)
    .where('toolCallId', '=', toolCallId)
    .executeTakeFirst();
  return row ? (JSON.parse(row.content) as StoredToolContent) : null;
}

function inspected(kind: InspectedText['kind'], text: string, truncated: boolean): InspectedText {
  return { kind, text, truncated, marks: injectionMarks(text) };
}

/**
 * The content behind the warning of one tool call of a stored reply.
 *
 * @param db - Open database.
 * @param conversationId - The chat, already checked for ownership.
 * @param messages - Its messages.
 * @param messageId - The reply.
 * @param toolCallId - The tool call.
 * @returns The texts with the places marked, or null if the call is not
 *   there or has no output to show.
 */
export async function inspection(
  db: Kysely<DB>,
  conversationId: string,
  messages: ChatMessage[],
  messageId: string,
  toolCallId: string,
): Promise<Inspection | null> {
  const part = messages
    .find((message) => message.id === messageId)
    ?.parts.find((candidate) => 'toolCallId' in candidate && candidate.toolCallId === toolCallId);
  if (!part || !('state' in part) || part.state !== 'output-available') {
    return null;
  }
  if (part.type === `tool-${WEB_FETCH_TOOL}`) {
    const output = part.output as WebFetchOutput;
    const kept = await keptContent(db, conversationId, toolCallId);
    if (!kept) {
      // Loaded before pages were kept: only what the model read is left.
      return { location: output.url, texts: [inspected('page', output.text, output.truncated)], complete: false };
    }
    const page = inspected('page', kept.page, kept.pageCut);
    const source = kept.source === null ? null : inspected('source', kept.source, kept.sourceCut);
    return { location: output.url, texts: source && source.marks.length > 0 ? [page, source] : [page], complete: true };
  }
  if (part.type === `tool-${WEB_SEARCH_TOOL}`) {
    const output = part.output as WebSearchOutput;
    // Exactly the text the patterns read, so every finding has its place.
    const text = output.results.map((result) => `${result.title}\n${result.snippet}`).join('\n\n');
    return { location: output.query, texts: [inspected('results', text, false)], complete: true };
  }
  if (part.type === 'dynamic-tool') {
    const output = part.output as AgentToolOutput;
    const call = JSON.stringify(part.input ?? {}).slice(0, 300);
    return { location: `${part.toolName} ${call}`, texts: [inspected('output', output.text, false)], complete: true };
  }
  return null;
}
