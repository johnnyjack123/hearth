import type { ChatFileKind } from '@harness/shared';

/** What a file is, decided from its first bytes and, for containers, its name. */
export interface DetectedFile {
  /** See {@link ChatFileKind}. */
  kind: ChatFileKind;
  /** Media type the server stands by. */
  mediaType: string;
}

const ZIP_DOCUMENTS: Record<string, string> = {
  '.docx': 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  '.xlsx': 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
  '.pptx': 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
  '.epub': 'application/epub+zip',
};

const HTML_EXTENSIONS = new Set(['.html', '.htm']);

function startsWith(bytes: Uint8Array, prefix: readonly number[], offset = 0): boolean {
  return prefix.every((byte, index) => bytes[offset + index] === byte);
}

function ascii(text: string): number[] {
  return [...text].map((char) => char.charCodeAt(0));
}

/**
 * The extension of a file name, lowercased, with the dot.
 *
 * @param name - File name.
 * @returns E.g. `.pdf`, or an empty string.
 */
export function extensionOf(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot).toLowerCase() : '';
}

/**
 * The text of a file, if it is UTF-8 without NUL bytes (a byte order mark is
 * dropped); binary data is not text, whatever its name says.
 *
 * @param bytes - The contents.
 * @returns The text, or null.
 */
export function decodeText(bytes: Uint8Array): string | null {
  if (bytes.includes(0)) {
    return null;
  }
  try {
    return new TextDecoder('utf-8', { fatal: true, ignoreBOM: false }).decode(bytes);
  } catch {
    return null;
  }
}

/**
 * Decides what an upload is by its contents: images by their signature,
 * PDF, office files and EPUB by theirs plus the extension (they are zip or
 * OLE containers), and anything that decodes as UTF-8 as text. The browser's
 * media type is never trusted; it comes from the user's machine.
 *
 * @param name - File name as uploaded.
 * @param bytes - The contents.
 * @returns What it is, or null if harness cannot use it.
 */
export function detectFile(name: string, bytes: Uint8Array): DetectedFile | null {
  const extension = extensionOf(name);
  if (startsWith(bytes, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])) {
    return { kind: 'image', mediaType: 'image/png' };
  }
  if (startsWith(bytes, [0xff, 0xd8, 0xff])) {
    return { kind: 'image', mediaType: 'image/jpeg' };
  }
  if (startsWith(bytes, ascii('GIF87a')) || startsWith(bytes, ascii('GIF89a'))) {
    return { kind: 'image', mediaType: 'image/gif' };
  }
  if (startsWith(bytes, ascii('RIFF')) && startsWith(bytes, ascii('WEBP'), 8)) {
    return { kind: 'image', mediaType: 'image/webp' };
  }
  if (startsWith(bytes, ascii('%PDF-'))) {
    return { kind: 'document', mediaType: 'application/pdf' };
  }
  if (startsWith(bytes, [0x50, 0x4b, 0x03, 0x04])) {
    const mediaType = ZIP_DOCUMENTS[extension];
    return mediaType ? { kind: 'document', mediaType } : null;
  }
  // Old Excel files are OLE containers; old Word and PowerPoint markitdown cannot read.
  if (startsWith(bytes, [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1])) {
    return extension === '.xls' ? { kind: 'document', mediaType: 'application/vnd.ms-excel' } : null;
  }
  if (decodeText(bytes) === null) {
    return null;
  }
  return HTML_EXTENSIONS.has(extension) ? { kind: 'document', mediaType: 'text/html' } : { kind: 'text', mediaType: 'text/plain' };
}

/**
 * A file name safe to store and show: no path, no control or direction
 * characters, at most 200 characters.
 *
 * @param raw - Name as the browser sent it (URL-encoded header).
 * @returns The cleaned name, never empty.
 */
export function cleanFileName(raw: string | undefined): string {
  let name = raw ?? '';
  try {
    name = decodeURIComponent(name);
  } catch {
    // Not encoded after all; use it as it is.
  }
  name = (name.split(/[\\/]/).at(-1) ?? '')
    .replace(/[\p{Cc}\p{Cf}]/gu, '')
    .trim();
  if (name.length > 200) {
    const extension = extensionOf(name);
    name = name.slice(0, 200 - extension.length) + extension;
  }
  return name || 'Datei';
}
