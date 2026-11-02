/** Largest file a user can attach, in bytes. */
export const MAX_FILE_BYTES = 20 * 1024 * 1024;

/** Files per message at most. */
export const MAX_FILES_PER_MESSAGE = 10;

/**
 * Characters of a document's text the server keeps; the rest is cut with a
 * note. Long documents reach the model as start and outline, and it reads
 * the parts it needs, so this only bounds the database and the agent's copy.
 */
export const MAX_FILE_TEXT_CHARS = 500_000;

/**
 * What an attached file is to the model: an image it looks at, a document
 * the server converts to Markdown, or text it reads as it is.
 */
export type ChatFileKind = 'image' | 'document' | 'text';

/**
 * An uploaded file as the server describes it. The bytes stay on the
 * server; messages refer to them by {@link chatFileUrl}.
 */
export interface ChatFile {
  /** File id (UUID). */
  id: string;
  /** Name as uploaded. */
  name: string;
  /** Media type the server determined from the contents. */
  mediaType: string;
  /** Size in bytes. */
  size: number;
  /** See {@link ChatFileKind}. */
  kind: ChatFileKind;
  /** Characters of text the model gets, for documents and text files. */
  chars?: number;
  /** Set when the text was longer than {@link MAX_FILE_TEXT_CHARS}. */
  truncated?: boolean;
}

/** Image types models read; others are refused. */
export const IMAGE_MEDIA_TYPES = ['image/png', 'image/jpeg', 'image/gif', 'image/webp'] as const;

/**
 * Extensions the file picker offers. The server decides by the contents;
 * text files with other extensions work when dropped or pasted.
 */
export const ACCEPTED_FILE_EXTENSIONS = [
  '.png',
  '.jpg',
  '.jpeg',
  '.gif',
  '.webp',
  '.pdf',
  '.docx',
  '.xlsx',
  '.xls',
  '.pptx',
  '.epub',
  '.html',
  '.htm',
  '.txt',
  '.md',
  '.csv',
  '.tsv',
  '.json',
  '.xml',
  '.yaml',
  '.yml',
  '.toml',
  '.ini',
  '.log',
  '.sql',
  '.js',
  '.mjs',
  '.ts',
  '.tsx',
  '.jsx',
  '.py',
  '.rs',
  '.go',
  '.java',
  '.kt',
  '.c',
  '.h',
  '.cpp',
  '.hpp',
  '.cs',
  '.rb',
  '.php',
  '.swift',
  '.sh',
  '.ps1',
  '.css',
  '.scss',
  '.vue',
  '.svelte',
] as const;

const FILE_URL = /^\/api\/files\/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$/;

/**
 * Where the browser loads an uploaded file; also how a message refers to it.
 *
 * @param id - File id.
 * @returns The path.
 */
export function chatFileUrl(id: string): string {
  return `/api/files/${id}`;
}

/**
 * The file id in a message's file part, if it points at an upload.
 *
 * @param url - The part's URL.
 * @returns The id, or null for anything else.
 */
export function chatFileId(url: string): string | null {
  return FILE_URL.exec(url)?.[1] ?? null;
}
