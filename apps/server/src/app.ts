import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { serveStatic } from '@hono/node-server/serve-static';
import { Hono } from 'hono';
import { csrf } from 'hono/csrf';
import { logger } from 'hono/logger';
import { secureHeaders } from 'hono/secure-headers';
import { type AppDeps, type AppEnv, requireUser } from './http/context.ts';
import { handleError, notFound } from './http/errors.ts';
import { conversationRoutes } from './routes/conversations.ts';
import { providerRoutes } from './routes/providers.ts';
import { adminSettingsRoutes, publicSettingsRoutes } from './routes/settings.ts';

/**
 * Options for {@link createApp}.
 */
export interface CreateAppOptions {
  /** Log every request to stdout. Off in tests. */
  logRequests?: boolean;
}

/**
 * Builds the HTTP application: auth, JSON API and, if a web build is
 * configured, the single-page app.
 *
 * @param deps - Application dependencies.
 * @param options - See {@link CreateAppOptions}.
 * @returns The Hono app; pass `app.fetch` to a server or call `app.request` in tests.
 */
export function createApp(deps: AppDeps, options: CreateAppOptions = {}) {
  const app = new Hono<AppEnv>();
  app.onError(handleError);

  if (options.logRequests) {
    app.use(logger());
  }
  app.use(
    secureHeaders({
      // The SPA is built with hashed assets and no inline scripts. Shiki's
      // highlighter compiles WebAssembly, which needs 'wasm-unsafe-eval'
      // (it does not allow JavaScript eval).
      contentSecurityPolicy: {
        defaultSrc: ["'self'"],
        scriptSrc: ["'self'", "'wasm-unsafe-eval'"],
        styleSrc: ["'self'", "'unsafe-inline'"],
        imgSrc: ["'self'", 'data:', 'blob:'],
        fontSrc: ["'self'", 'data:'],
        connectSrc: ["'self'"],
        frameAncestors: ["'none'"],
      },
    }),
  );
  app.use('/api/*', csrf({ origin: deps.config.trustedOrigins }));

  app.get('/api/health', (c) => c.json({ ok: true }));
  app.on(['GET', 'POST'], '/api/auth/*', (c) => deps.auth.handler(c.req.raw));
  app.route('/api/settings', publicSettingsRoutes(deps));

  const api = new Hono<AppEnv>();
  api.use(requireUser(deps.auth));
  const { providers, models } = providerRoutes(deps);
  api.route('/providers', providers);
  api.route('/models', models);
  api.route('/conversations', conversationRoutes(deps));
  api.route('/admin/settings', adminSettingsRoutes(deps));
  app.route('/api', api);

  app.all('/api/*', () => {
    throw notFound('Endpoint');
  });

  if (deps.config.webDist) {
    mountWebApp(app, deps.config.webDist);
  }
  return app;
}

function mountWebApp(app: Hono<AppEnv>, webDist: string): void {
  const indexFile = path.join(webDist, 'index.html');
  if (!existsSync(indexFile)) {
    console.warn(`[http] web build not found at ${webDist}; serving API only`);
    return;
  }
  const indexHtml = readFileSync(indexFile, 'utf8');
  // serveStatic resolves `root` against the working directory.
  const root = path.relative(process.cwd(), webDist) || '.';

  app.use(
    '/assets/*',
    serveStatic({
      root,
      onFound: (_path, c) => {
        c.header('Cache-Control', 'public, max-age=31536000, immutable');
      },
    }),
  );
  app.use('/*', serveStatic({ root }));
  // Client-side routes: every unknown GET path renders the SPA.
  app.get('*', (c) => {
    c.header('Cache-Control', 'no-cache');
    return c.html(indexHtml);
  });
}
