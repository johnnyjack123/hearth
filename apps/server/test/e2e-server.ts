// Server for the browser E2E (apps/web/e2e/run-e2e.mjs): the real server,
// except that web fetch may also reach the mock LLM's test page on
// 127.0.0.1:8091. Every other non-public address stays blocked.
import { startServer } from '../src/server.ts';
import { isPublicAddress } from '../src/webfetch/address.ts';

const MOCK_PORT = 8091;

startServer({
  webFetch: {
    isAllowedAddress: (ip) => ip === '127.0.0.1' || isPublicAddress(ip),
    ports: [80, 443, MOCK_PORT],
  },
}).catch((error: unknown) => {
  console.error('[server] failed to start', error);
  process.exit(1);
});
