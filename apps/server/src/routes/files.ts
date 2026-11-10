import { MAX_FILE_BYTES, MAX_FILE_TEXT_CHARS } from '@hearth/shared';
import { Hono } from 'hono';
import { bodyLimit } from 'hono/body-limit';
import { type DocumentConverter, markitdownConverter } from '../files/convert.ts';
import { cleanFileName, decodeText, detectFile } from '../files/detect.ts';
import { fileStore } from '../files/store.ts';
import type { AppDeps, AppEnv } from '../http/context.ts';
import { HttpError, notFound } from '../http/errors.ts';

/**
 * The converter the server uses for documents, if any.
 *
 * @param deps - Application dependencies.
 * @returns The converter, or null without one.
 */
export function documentConverter(deps: AppDeps): DocumentConverter | null {
  if (deps.converter !== undefined) {
    return deps.converter;
  }
  return deps.config.markitdownUrl ? markitdownConverter(deps.config.markitdownUrl) : null;
}

/**
 * Routes for uploads (`/api/files`): upload one file, read it back, read
 * the text the model gets.
 *
 * @param deps - Application dependencies.
 * @returns The router.
 */
export function fileRoutes(deps: AppDeps) {
  const store = fileStore(deps.database.db, deps.config.dataDir);
  const converter = documentConverter(deps);

  return (
    new Hono<AppEnv>()
      // The raw file as the body, its name in `X-File-Name` (URL-encoded).
      .post(
        '/',
        bodyLimit({
          maxSize: MAX_FILE_BYTES,
          onError: () => {
            throw new HttpError(413, 'file_too_large', `Files may have at most ${MAX_FILE_BYTES} bytes`);
          },
        }),
        async (c) => {
          const user = c.get('user');
          const name = cleanFileName(c.req.header('x-file-name'));
          const bytes = new Uint8Array(await c.req.arrayBuffer());
          if (bytes.length === 0) {
            throw new HttpError(422, 'file_empty', 'The file is empty');
          }
          const detected = detectFile(name, bytes);
          if (!detected) {
            throw new HttpError(415, 'file_unsupported', 'This kind of file is not supported');
          }
          let text: string | null = null;
          if (detected.kind === 'text') {
            text = decodeText(bytes);
          } else if (detected.kind === 'document') {
            if (!converter) {
              throw new HttpError(503, 'converter_unavailable', 'No document converter is set up (MARKITDOWN_URL)');
            }
            text = await converter.convert(name, bytes);
          }
          if (text !== null && !text.trim()) {
            // A scanned PDF has pages but no text layer.
            throw new HttpError(422, detected.kind === 'document' ? 'no_text' : 'file_empty', 'The file has no readable text');
          }
          const truncated = text !== null && text.length > MAX_FILE_TEXT_CHARS;
          const file = await store.save(
            {
              userId: user.id,
              name,
              mediaType: detected.mediaType,
              kind: detected.kind,
              text: truncated ? (text as string).slice(0, MAX_FILE_TEXT_CHARS) : text,
              truncated,
            },
            bytes,
          );
          return c.json(file, 201);
        },
      )
      .get('/:id', async (c) => {
        const file = await store.find(c.get('user').id, c.req.param('id'));
        if (!file) {
          throw notFound('File');
        }
        const bytes = await store.read(file.id);
        const image = file.kind === 'image';
        // Images show in the chat; anything else only downloads, under a
        // type the browser does not interpret, sandboxed as well.
        return c.body(new Uint8Array(bytes), 200, {
          'content-type': image ? file.mediaType : 'application/octet-stream',
          'content-disposition': `${image ? 'inline' : 'attachment'}; filename*=UTF-8''${encodeURIComponent(file.name)}`,
          'content-security-policy': "default-src 'none'; sandbox",
          'x-content-type-options': 'nosniff',
          // Ids are never reused, so the contents never change.
          'cache-control': 'private, max-age=86400, immutable',
        });
      })
      .get('/:id/text', async (c) => {
        const file = await store.find(c.get('user').id, c.req.param('id'));
        if (!file || file.text === null) {
          throw notFound('File text');
        }
        return c.body(file.text, 200, {
          'content-type': 'text/plain; charset=utf-8',
          'content-security-policy': "default-src 'none'; sandbox",
          'x-content-type-options': 'nosniff',
        });
      })
  );
}
