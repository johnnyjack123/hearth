import { HttpError } from '../http/errors.ts';

/** Turns a document into Markdown for the model. */
export interface DocumentConverter {
  /**
   * @param name - File name; its extension tells the converter the format.
   * @param bytes - The document.
   * @returns Its text as Markdown.
   * @throws {HttpError} 422 `convert_failed` or 503 `converter_unavailable`.
   */
  convert(name: string, bytes: Uint8Array): Promise<string>;
}

/** A large PDF takes markitdown a while on a Raspberry Pi. */
const CONVERT_TIMEOUT_MS = 300_000;

/**
 * The converter of apps/markitdown, reached over HTTP. Its answer is text
 * from a parser that read an untrusted file, so it is treated as data.
 *
 * @param baseUrl - E.g. `http://markitdown:8080`.
 * @returns The converter.
 */
export function markitdownConverter(baseUrl: string): DocumentConverter {
  return {
    async convert(name, bytes) {
      let response: Response;
      try {
        response = await fetch(`${baseUrl}/convert`, {
          method: 'POST',
          headers: { 'content-type': 'application/octet-stream', 'x-file-name': encodeURIComponent(name) },
          body: bytes,
          signal: AbortSignal.timeout(CONVERT_TIMEOUT_MS),
        });
      } catch (error) {
        console.warn('[files] converter not reachable:', error instanceof Error ? error.message : error);
        throw new HttpError(503, 'converter_unavailable', 'The document converter is not reachable');
      }
      const body = (await response.json().catch(() => null)) as { markdown?: unknown; error?: unknown } | null;
      if (response.ok && typeof body?.markdown === 'string') {
        return body.markdown;
      }
      const reason = typeof body?.error === 'string' ? body.error.slice(0, 300) : `HTTP ${response.status}`;
      throw new HttpError(422, 'convert_failed', `The document could not be read: ${reason}`);
    },
  };
}
