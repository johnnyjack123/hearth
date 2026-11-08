import { createHash, timingSafeEqual } from 'node:crypto';
import { Hono } from 'hono';
import { bodyLimit } from 'hono/body-limit';
import { createMiddleware } from 'hono/factory';
import { CLIENT_IP_HEADER } from '../auth.ts';
import { rateKey } from '../http/client-ip.ts';
import type { AppDeps, AppEnv } from '../http/context.ts';
import { RateLimiter } from '../http/rate-limit.ts';
import { countUsers, getServerSettings } from '../services/settings.ts';

/** Header with the setup code when the first account signs up. */
export const SETUP_CODE_HEADER = 'x-harness-setup-code';

/** Sign-up and sign-in forms are small; nothing else of Better Auth takes more. */
const MAX_AUTH_BODY = 64 * 1024;

function digest(code: string): Buffer {
  return createHash('sha256').update(code.toUpperCase().replace(/[\s-]/g, '')).digest();
}

/**
 * Compares a setup code as typed with the server's, ignoring case, dashes
 * and spaces, in constant time.
 *
 * @param given - Code from the request.
 * @param expected - The server's code.
 * @returns True if they match.
 */
export function sameSetupCode(given: string | undefined, expected: string): boolean {
  return given !== undefined && timingSafeEqual(digest(given), digest(expected));
}

/**
 * Better Auth sees the client address only in a header it is told to read.
 * The header is set here from the address the server determined; whatever
 * a client sent under that name is dropped.
 *
 * @param request - The incoming request.
 * @param ip - The client's address, or null if unknown.
 * @returns The request to hand to Better Auth.
 */
function withClientIp(request: Request, ip: string | null): Request {
  const headers = new Headers(request.headers);
  headers.delete(CLIENT_IP_HEADER);
  if (ip) {
    headers.set(CLIENT_IP_HEADER, ip);
  }
  return new Request(request, { headers });
}

/**
 * Better Auth under `/api/auth`, with the checks it does not do itself:
 * a body limit, the setup code of the first account, and closed
 * registration rejected before Better Auth looks up the address (which
 * would reveal whether an account exists) or hashes a password.
 *
 * @param deps - Application dependencies.
 * @returns Router to mount under `/api/auth`.
 */
export function authRoutes(deps: AppDeps) {
  const { db } = deps.database;
  // Wrong setup codes per address; the code has 40 bits, this only keeps
  // the log quiet.
  const wrongCodes = new RateLimiter(10, 10 * 60_000);

  const signUpGuard = createMiddleware<AppEnv>(async (c, next) => {
    if ((await countUsers(db)) === 0) {
      const expected = deps.config.setupCode;
      if (expected && !sameSetupCode(c.req.header(SETUP_CODE_HEADER), expected)) {
        if (wrongCodes.take(rateKey(c.get('clientIp'))) > 0) {
          return c.json({ code: 'TOO_MANY_REQUESTS', message: 'Too many attempts; try again later' }, 429);
        }
        return c.json({ code: 'SETUP_CODE_INVALID', message: 'The first account needs the setup code from the server log' }, 403);
      }
    } else if ((await getServerSettings(db)).registration === 'closed') {
      return c.json({ code: 'REGISTRATION_CLOSED', message: 'Registration is closed' }, 403);
    }
    await next();
  });

  return new Hono<AppEnv>()
    .use(
      bodyLimit({
        maxSize: MAX_AUTH_BODY,
        onError: (c) => c.json({ code: 'PAYLOAD_TOO_LARGE', message: 'Request body too large' }, 413),
      }),
    )
    .post('/sign-up/email', signUpGuard)
    .on(['GET', 'POST'], '/*', (c) => deps.auth.handler(withClientIp(c.req.raw, c.get('clientIp'))));
}
