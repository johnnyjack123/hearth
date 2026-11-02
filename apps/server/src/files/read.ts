/**
 * Long documents in ordinary chats. Sent whole, a long PDF would fill the
 * context and cost its tokens again with every message; instead the model
 * gets the start and the outline, and reads the parts it needs with the
 * tool `read_document`, by focus, words or section numbers, the same way
 * `web_fetch` reads long pages.
 */
import { chatFileId } from '@harness/shared';
import { tool } from 'ai';
import { z } from 'zod';
import type { Helpers } from '../helpers/openrouter.ts';
import type { ChatMessage } from '../services/conversations.ts';
import { grepPage } from '../webfetch/grep.ts';
import { type OutlineGroup, outlineOf, selectSections, splitSections } from '../webfetch/sections.ts';
import type { FileRow, FileStore } from './store.ts';

/** Name of the tool that reads parts of a long document. */
export const READ_DOCUMENT_TOOL = 'read_document';

/** Documents longer than this (~4,000 tokens) reach the model as start and outline. */
export const LONG_DOCUMENT_CHARS = 16_000;

/** Characters of a long document the model gets right away. */
const PREVIEW_CHARS = 3_000;

/** Characters one call of the tool returns at most. */
const READ_CHARS = 8_000;

/** What reading a document failed with; reaches the model and the chat as it is. */
export class DocumentError extends Error {}

/**
 * The short name the model uses for a file: the end of its UUID.
 *
 * @param id - File id.
 * @returns Eight hex digits.
 */
export function documentRef(id: string): string {
  return id.slice(-8);
}

/**
 * Whether a stored file is too long to send whole.
 *
 * @param row - The file.
 * @returns True for long documents and text files.
 */
export function isLongDocument(row: FileRow): boolean {
  return row.kind !== 'image' && (row.text?.length ?? 0) > LONG_DOCUMENT_CHARS;
}

function outlineLines(outline: OutlineGroup[], total: number, shown: number[] = []): string[] {
  return [
    `Outline (${total} sections${shown.length > 0 ? ', shown ones marked *' : ''}):`,
    ...outline.map(({ from, to, heading }) => {
      const mark = shown.some((index) => index >= from && index <= to) ? '*' : ' ';
      return `${mark} §${from === to ? from : `${from}-${to}`} ${heading || '(start)'}`;
    }),
    ...(outline.at(-1)?.to !== total ? [`  … up to §${total}`] : []),
  ];
}

/**
 * What the model reads of a long document in the chat: its start and its
 * outline. The same text in every request, so the prompt cache holds.
 *
 * @param row - The file.
 * @returns The text part.
 */
export function longDocumentText(row: FileRow): string {
  const text = row.text ?? '';
  const sections = splitSections(text);
  const ref = documentRef(row.id);
  return [
    `[Attached file ${JSON.stringify(row.name)} (${row.mediaType}), ${text.length} characters: too long to send whole. ` +
      `Here are its start and its outline; read the parts you need with the ${READ_DOCUMENT_TOOL} tool, file "${ref}".]`,
    '<file>',
    `${text.slice(0, PREVIEW_CHARS)}…`,
    '',
    ...outlineLines(outlineOf(sections), sections.length),
    '</file>',
  ].join('\n');
}

/**
 * The ids of the files a conversation refers to.
 *
 * @param messages - The conversation.
 * @returns File ids.
 */
export function attachedFileIds(messages: ChatMessage[]): string[] {
  return messages.flatMap((message) => message.parts.flatMap((part) => (part.type === 'file' ? [chatFileId(part.url) ?? ''] : []))).filter(Boolean);
}

/** What one call of {@link READ_DOCUMENT_TOOL} returns. */
export interface ReadDocumentOutput {
  /** The document's name. */
  file: string;
  /** What the model reads: the parts and the outline. */
  text: string;
}

/**
 * The tool that reads parts of the chat's long documents.
 *
 * @param store - Where the files are.
 * @param ids - The files of the conversation.
 * @param helpers - The user's helper models (embeddings for a focus); keywords decide without them.
 * @param signal - Ends embedding calls with the request.
 * @returns The tool.
 */
export function createReadDocumentTool(store: FileStore, ids: string[], helpers: Helpers | null, signal?: AbortSignal) {
  return tool({
    description:
      'Reads parts of a long document attached to this chat. Name the file by the id given with it (or its name). ' +
      'Set focus to what you look for to get the matching sections, grep to words next to an exact figure to get those lines, ' +
      'or sections to numbers from the outline. Ask again for other parts instead of guessing what the document says.',
    inputSchema: z.object({
      file: z.string().trim().min(1).max(300).describe('The id given with the document, or its name'),
      focus: z.string().trim().max(300).optional().describe('What you look for, as a short question or a few words'),
      grep: z.string().trim().max(300).optional().describe('Words or phrases to find, separated by |'),
      sections: z.array(z.number().int().min(1)).max(30).optional().describe('Section numbers from the outline'),
    }),
    execute: async ({ file, focus, grep, sections }): Promise<ReadDocumentOutput> => {
      const rows = (await store.rows(ids)).filter((row) => row.kind !== 'image' && row.text);
      const row = rows.find((candidate) => documentRef(candidate.id) === file.toLowerCase()) ?? rows.find((candidate) => candidate.name === file);
      if (!row?.text) {
        const names = rows.map((candidate) => `"${documentRef(candidate.id)}" (${candidate.name})`).join(', ');
        throw new DocumentError(`No document "${file}" in this chat.${names ? ` Its documents: ${names}.` : ''}`);
      }
      const header = `From ${JSON.stringify(row.name)}:`;
      if (grep) {
        const found = grepPage(row.text, grep);
        return { file: row.name, text: found ? `${header}\n${found.text}` : `${header} no line contains ${JSON.stringify(grep)}.` };
      }
      const selection = await selectSections(row.text, { focus: focus || null, sections: sections ?? null, maxChars: READ_CHARS }, helpers, signal);
      if (!selection) {
        return { file: row.name, text: `${header}\n${row.text}` };
      }
      const note = focus && !selection.matched ? ' (nothing matched the focus; this is the start)' : '';
      return {
        file: row.name,
        text: [`${header} sections ${selection.shown.join(', ')} of ${selection.total}${note}`, selection.text, '', ...outlineLines(selection.outline, selection.total, selection.shown)].join(
          '\n',
        ),
      };
    },
    toModelOutput: ({ output }) => ({ type: 'text', value: output.text }),
  });
}
