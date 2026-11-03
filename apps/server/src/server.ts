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
import { forDevice, listAppTemplates } from './services/app-templates.ts';
import { saveDeviceApps, saveDeviceProjects, saveDeviceVersion } from './services/devices.ts';
import { DeviceHub } from './tunnel/hub.ts';
import { attachTunnel } from './tunnel/socket.ts';

/**
 * Options of {@link startServer}. The server itself starts without any; the
 * browser E2E uses them to reach its local test page.
 */
export interface StartServerOptions {
  /** See `AppDeps.webFetch`. */
  webFetch?: AppDeps['webFetch'];
  /** See `AppDeps.helpers`. */
  helpers?: AppDeps['helpers'];
}

/**
 * The device hub with what it keeps in the database: reported projects and
 * apps stay visible while a device is offline. Tests build theirs here too,
 * so nothing is wired only in production.
 *
 * @param db - Open database.
 * @returns The hub.
 */
export function createDeviceHub(db: AppDeps['database']['db']): DeviceHub {
  return new DeviceHub({
    projects: (deviceId, projects) => saveDeviceProjects(db, deviceId, projects),
    apps: (deviceId, report) => saveDeviceApps(db, deviceId, report),
    version: (deviceId, harness) => saveDeviceVersion(db, deviceId, harness),
    templates: async (userId) => (await listAppTemplates(db, userId)).map(forDevice),
  });
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
  const hub = createDeviceHub(database.db);
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
      ...(options.helpers ? { helpers: options.helpers } : {}),
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
