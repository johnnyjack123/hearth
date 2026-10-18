import { startupHint } from './config.ts';
import { startServer } from './server.ts';

startServer().catch((error: unknown) => {
  console.error('[server] failed to start', error);
  const hint = startupHint(error);
  if (hint) {
    console.error(`[server] ${hint}`);
  }
  process.exit(1);
});
