import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { serveStatic } from '@hono/node-server/serve-static';
import { Hono } from 'hono';
import { csrf } from 'hono/csrf';
import { logger } from 'hono/logger';
import { secureHeaders } from 'hono/secure-headers';
import { peerAddress, resolveClientIp } from './http/client-ip.ts';
import { type AppDeps, type AppEnv, requireUser } from './http/context.ts';
import { handleError, notFound } from './http/errors.ts';
import { withHeartbeat } from './http/heartbeat.ts';
import { appTemplateRoutes } from './routes/app-templates.ts';
import { authRoutes } from './routes/auth.ts';
import { conversationRoutes } from './routes/conversations.ts';
import { deviceRoutes, deviceSelfRoutes, pairingRoutes } from './routes/devices.ts';
import { fileRoutes } from './routes/files.ts';
import { llmRoutes } from './routes/llm.ts';
import { costRoutes } from './routes/costs.ts';
import { featureRoutes } from './routes/features.ts';
import { memoryRoutes } from './routes/memories.ts';
import { openRouterRoutes } from './routes/openrouter.ts';
import { tagRoutes } from './routes/tags.ts';
import { providerRoutes } from './routes/providers.ts';
import { adminSettingsRoutes, publicSettingsRoutes } from './routes/settings.ts';
import { OpenRouterRouting } from './openrouter.ts';
import { HostProber } from './openrouter-access.ts';
import { ActivityTracker } from './http/activity.ts';
import { adminActivityRoutes } from './routes/activity.ts';
import { ActiveStreams } from './streams.ts';
import { SERVER_COMMIT, SERVER_VERSION } from './version.ts';

// `/api/devices` (session) must not match, `/api/device` (token) must.
const TOKEN_ROUTES = /^\/api\/(pairing|device|llm)(\/|$)/;

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
 * @param given - Application dependencies.
 * @param options - See {@link CreateAppOptions}.
 * @returns The Hono app; pass `app.fetch` to a server or call `app.request` in tests.
 */
export function createApp(given: AppDeps, options: CreateAppOptions = {}) {
  // One routing for chats and the agents' pass-through, so both share its lists.
  const deps: AppDeps = {
    ...given,
    openRouter: given.openRouter ?? new OpenRouterRouting(),
    hostProber: given.hostProber ?? new HostProber(),
    streams: given.streams ?? new ActiveStreams(),
    activity: given.activity ?? new ActivityTracker(),
  };
  const app = new Hono<AppEnv>();
  app.onError(handleError);

  if (options.logRequests) {
    app.use(logger());
  }
  // nginx, and with it Nginx Proxy Manager, buffers responses by default: a
  // streamed answer would reach the browser only at its end. This header
  // switches that off per response, without settings in the proxy.
  // Long replies pause for longer than a proxy waits: comment lines keep
  // them open (`withHeartbeat`). The LLM pass-through stays byte for byte as
  // the provider sent it.
  app.use(async (c, next) => {
    await next();
    if (c.res.headers.get('content-type')?.startsWith('text/event-stream')) {
      const body = c.res.body && !c.req.path.startsWith('/api/llm/') ? withHeartbeat(c.res.body) : c.res.body;
      // A new response: headers of a fetched one are immutable.
      c.res = new Response(body, c.res);
      c.res.headers.set('X-Accel-Buffering', 'no');
    }
  });
  const proxies = deps.config.trustedProxies;
  app.use((c, next) => {
    const peer = peerAddress(c);
    c.set('clientIp', proxies ? resolveClientIp(peer, c.req.header('x-forwarded-for'), proxies) : peer);
    return next();
  });
  const appHeaders = secureHeaders({
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
  });
  // Uploaded files are the users' own bytes: nothing in them may run, also
  // when a browser opens one directly. secureHeaders overwrites the policy a
  // route sets, so these paths get their own.
  const fileHeaders = secureHeaders({ contentSecurityPolicy: { defaultSrc: ["'none'"], sandbox: [] } });
  app.use((c, next) => (c.req.path.startsWith('/api/files/') ? fileHeaders(c, next) : appHeaders(c, next)));
  const csrfProtection = csrf({ origin: deps.config.trustedOrigins });
  app.use('/api/*', (c, next) =>
    // Device routes authenticate with a bearer token or not at all, never
    // with the session cookie, so a cross-site request gains nothing there;
    // and CLI clients send no Origin header.
    TOKEN_ROUTES.test(c.req.path) ? next() : csrfProtection(c, next),
  );

  app.get('/api/health', (c) => c.json({ ok: true, version: SERVER_VERSION, commit: SERVER_COMMIT }));
  app.route('/api/auth', authRoutes(deps));
  app.route('/api/settings', publicSettingsRoutes(deps));
  // Device routes bring their own auth (pairing: none, device token) and
  // must be registered before the session-protected `api` router.
  app.route('/api/pairing', pairingRoutes(deps));
  app.route('/api/device', deviceSelfRoutes(deps));
  app.route('/api/llm', llmRoutes(deps));

  const api = new Hono<AppEnv>();
  api.use(requireUser(deps.auth, (user) => deps.activity?.touch(user.id)));
  const { providers, models } = providerRoutes(deps);
  api.route('/providers', providers);
  api.route('/models', models);
  api.route('/conversations', conversationRoutes(deps));
  api.route('/files', fileRoutes(deps));
  api.route('/devices', deviceRoutes(deps));
  api.route('/app-templates', appTemplateRoutes(deps));
  api.route('/memories', memoryRoutes(deps));
  api.route('/costs', costRoutes(deps));
  api.route('/openrouter', openRouterRoutes(deps));
  api.route('/tags', tagRoutes(deps));
  api.route('/features', featureRoutes(deps));
  api.route('/admin/settings', adminSettingsRoutes(deps));
  api.route('/admin/activity', adminActivityRoutes(deps));
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
