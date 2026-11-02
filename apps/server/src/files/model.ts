import { chatFileId, MAX_FILE_TEXT_CHARS } from '@harness/shared';
import type { ChatMessage } from '../services/conversations.ts';
import { isLongDocument, longDocumentText } from './read.ts';
import type { FileRow, FileStore } from './store.ts';

type Part = ChatMessage['parts'][number];

/**
 * What the model reads of a document or text file. The same text in every
 * request, so the provider's prompt cache keeps working.
 *
 * @param row - The file.
 * @returns The text part.
 */
export function fileText(row: FileRow): string {
  if (isLongDocument(row)) {
    return longDocumentText(row);
  }
  const cut = row.truncated ? `, cut to the first ${MAX_FILE_TEXT_CHARS} characters` : '';
  return `[Attached file ${JSON.stringify(row.name)} (${row.mediaType}${cut})]\n<file>\n${row.text ?? ''}\n</file>`;
}

/**
 * Replaces the chat's file references (`/api/files/<id>`) with what the
 * model reads: an image as inline data, a document or text file as its
 * text. Images go to a model that cannot see them as a short note instead,
 * so switching to such a model mid-chat still works.
 *
 * @param messages - Messages as stored.
 * @param store - Where the files are.
 * @param images - Whether the model reads images; undefined when unknown,
 *   then they are sent and the provider decides.
 * @returns The messages for `convertToModelMessages`.
 */
export async function withFileContents(messages: ChatMessage[], store: FileStore, images: boolean | undefined): Promise<ChatMessage[]> {
  const ids = messages.flatMap((message) => message.parts.flatMap((part) => (part.type === 'file' ? [chatFileId(part.url)] : [])));
  const wanted = ids.filter((id): id is string => id !== null);
  if (wanted.length === 0 && ids.length === 0) {
    return messages;
  }
  const rows = new Map((await store.rows(wanted)).map((row) => [row.id, row]));
  const data = new Map<string, string>();
  for (const row of rows.values()) {
    if (row.kind === 'image' && images !== false) {
      data.set(row.id, `data:${row.mediaType};base64,${(await store.read(row.id)).toString('base64')}`);
    }
  }
  return messages.map((message) => {
    if (!message.parts.some((part) => part.type === 'file')) {
      return message;
    }
    const parts = message.parts.map((part): Part => {
      if (part.type !== 'file') {
        return part;
      }
      const id = chatFileId(part.url);
      const row = id ? rows.get(id) : undefined;
      const name = JSON.stringify(row?.name ?? part.filename ?? 'file');
      if (!row) {
        return { type: 'text', text: `[Attached file ${name}: no longer available.]` };
      }
      if (row.kind !== 'image') {
        return { type: 'text', text: fileText(row) };
      }
      const url = data.get(row.id);
      return url
        ? { type: 'file', mediaType: row.mediaType, filename: row.name, url }
        : { type: 'text', text: `[Attached image ${name}: the current model cannot see images.]` };
    });
    return { ...message, parts };
  });
}
