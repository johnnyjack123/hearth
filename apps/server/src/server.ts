import { serve } from '@hono/node-server';
import { createApp } from './app.ts';
import { createAuth, migrateAuth } from './auth.ts';
import { loadConfig } from './config.ts';
import { createSecretBox } from './crypto.ts';
import { createDatabase, parseDatabaseUrl } from './db/index.ts';
import { migrateToLatest } from './db/migrations.ts';
import type { AppDeps } from './http/context.ts';
import { createLlmGateway } from './llm.ts';
import { PricingCatalog } from './pricing.ts';
import { saveDeviceProjects } from './services/devices.ts';
import { DeviceHub } from './tunnel/hub.ts';
import { attachTunnel } from './tunnel/socket.ts';

/**
 * Options of {@link startServer}. The server itself starts without any; the
 * browser E2E uses them to reach its local test page.
 */
export interface StartServerOptions {
  /** See `AppDeps.webFetch`. */
  webFetch?: AppDeps['webFetch'];
}

/**
 * Loads the configuration from the environment, migrates the database and
 * serves the app until SIGINT or SIGTERM.
 *
 * @param options - See {@link StartServerOptions}.
 * @returns Resolves once the server listens.
 */
export async function startServer(options: StartServerOptions = {}): Promise<void> {
  const config = loadConfig();
  const database = createDatabase(config.databaseUrl);
  await migrateAuth(config, database);
  await migrateToLatest(database.db);
  const auth = createAuth(config, database);
  const hub = new DeviceHub({ projects: (deviceId, projects) => saveDeviceProjects(database.db, deviceId, projects) });
  const pricing = new PricingCatalog();
  void pricing.refresh();

  const app = createApp(
    {
      config,
      database,
      auth,
      secretBox: createSecretBox(config.secret),
      llm: createLlmGateway(),
      hub,
      pricing,
      ...(options.webFetch ? { webFetch: options.webFetch } : {}),
    },
    { logRequests: true },
  );

  const server = serve({ fetch: app.fetch, hostname: config.host, port: config.port }, (info) => {
    const target = parseDatabaseUrl(config.databaseUrl);
    console.log(`[server] listening on http://${info.address}:${info.port} (public URL ${config.publicUrl})`);
    console.log(`[server] database: ${target.kind}${target.kind === 'sqlite' ? ` (${target.file})` : ''}`);
    console.log(`[server] web app: ${config.webDist ?? 'not served (API only)'}`);
  });
  attachTunnel(server, database.db, hub);

  const shutdown = (signal: string) => {
    console.log(`[server] ${signal} received, shutting down`);
    server.close(() => {
      void database.close().finally(() => process.exit(0));
    });
    // Streaming responses can keep connections open; do not wait forever.
    setTimeout(() => process.exit(0), 10_000).unref();
  };
  process.on('SIGINT', () => shutdown('SIGINT'));
  process.on('SIGTERM', () => shutdown('SIGTERM'));
}
