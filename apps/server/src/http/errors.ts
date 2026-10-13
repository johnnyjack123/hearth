import type { ApiErrorBody, ApiErrorCode } from '@harness/shared';
import type { Context } from 'hono';
import { HTTPException } from 'hono/http-exception';
import type { ContentfulStatusCode } from 'hono/utils/http-status';
import { ZodError } from 'zod';

/**
 * An error that maps directly to an HTTP response with an {@link ApiErrorBody}.
 */
export class HttpError extends Error {
  /** HTTP status to respond with. */
  readonly status: ContentfulStatusCode;
  /** Stable error code for the client. */
  readonly code: ApiErrorCode;

  /**
   * @param status - HTTP status to respond with.
   * @param code - Stable error code for the client.
   * @param message - English description, returned to the client and logged.
   */
  constructor(status: ContentfulStatusCode, code: ApiErrorCode, message: string) {
    super(message);
    this.name = 'HttpError';
    this.status = status;
    this.code = code;
  }
}

/**
 * Shorthand for a 404 response.
 *
 * @param what - Name of the missing resource, e.g. `Conversation`.
 * @returns The error to throw.
 */
export function notFound(what: string): HttpError {
  return new HttpError(404, 'not_found', `${what} not found`);
}

/**
 * Converts any thrown value into a JSON error response. Unexpected errors are
 * logged and reported without details, so internals never reach the client.
 *
 * @param error - The thrown value.
 * @param c - Request context.
 * @returns The error response.
 */
export function handleError(error: unknown, c: Context): Response {
  if (error instanceof HttpError) {
    return c.json<ApiErrorBody>({ error: { code: error.code, message: error.message } }, error.status);
  }
  if (error instanceof ZodError) {
    const message = error.issues.map((issue) => `${issue.path.join('.') || 'body'}: ${issue.message}`).join('; ');
    return c.json<ApiErrorBody>({ error: { code: 'validation', message } }, 400);
  }
  if (error instanceof SyntaxError) {
    return c.json<ApiErrorBody>({ error: { code: 'validation', message: 'Request body is not valid JSON' } }, 400);
  }
  // Raised by Hono's own middleware, e.g. the CSRF check.
  if (error instanceof HTTPException) {
    const code: ApiErrorCode = error.status === 403 ? 'forbidden' : error.status < 500 ? 'validation' : 'internal';
    return c.json<ApiErrorBody>({ error: { code, message: error.message || 'Request rejected' } }, error.status);
  }
  console.error('[http] unhandled error', error);
  return c.json<ApiErrorBody>({ error: { code: 'internal', message: 'Internal server error' } }, 500);
}
