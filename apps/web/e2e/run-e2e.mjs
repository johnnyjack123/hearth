// Starts an isolated stack and runs the browser end-to-end test:
//   mock LLM (OpenAI format) :8091, API server :8090 with a throwaway data
//   directory, Vite dev server :5174 proxying to it (dev mode = React
//   StrictMode, which is where lifecycle bugs show up).
// Usage: pnpm --filter @harness/web e2e    Screenshots: apps/web/e2e/.output/
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const webDir = path.resolve(here, '..');
const serverDir = path.resolve(here, '../../server');
const outDir = path.join(here, '.output');
const dataDir = mkdtempSync(path.join(tmpdir(), 'harness-e2e-'));
rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

const children = [];
// Everything runs through the current Node binary without a shell: spawning
// pnpm on Windows would need shell mode, which breaks on paths with spaces.
function start(name, args, options) {
  const child = spawn(process.execPath, args, { ...options, stdio: ['ignore', 'pipe', 'pipe'] });
  let log = '';
  child.stdout.on('data', (chunk) => (log += chunk));
  child.stderr.on('data', (chunk) => (log += chunk));
  children.push({ name, child, log: () => log });
  return () => log;
}

function stopAll() {
  for (const { child } of children) {
    if (child.exitCode !== null) {
      continue;
    }
    // pnpm and tsx spawn grandchildren; on Windows only taskkill /T reaches them.
    if (process.platform === 'win32') {
      spawnSync('taskkill', ['/pid', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
    } else {
      child.kill('SIGTERM');
    }
  }
  rmSync(dataDir, { recursive: true, force: true });
}

async function waitFor(name, logOf, pattern, timeoutMs = 60_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (pattern.test(logOf())) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`${name} did not start in time:\n${logOf()}`);
}

let exitCode = 1;
try {
  const mockLog = start('mock', [path.join(here, 'mock-llm.mjs')], { env: { ...process.env, MOCK_PORT: '8091' } });
  const serverLog = start('server', ['--import', 'tsx', 'src/index.ts'], {
    cwd: serverDir,
    env: {
      ...process.env,
      PORT: '8090',
      DATA_DIR: dataDir,
      PUBLIC_URL: 'http://127.0.0.1:5174',
      TRUSTED_ORIGINS: 'http://127.0.0.1:5174',
      WEB_DIST: path.join(dataDir, 'no-web-build'),
    },
  });
  const viteLog = start('vite', [path.join(webDir, 'node_modules/vite/bin/vite.js'), '--host', '127.0.0.1', '--port', '5174'], {
    cwd: webDir,
    env: { ...process.env, HARNESS_API_URL: 'http://127.0.0.1:8090' },
  });
  await waitFor('mock', mockLog, /listening/);
  await waitFor('server', serverLog, /listening/);
  await waitFor('vite', viteLog, /Local/);

  const test = spawnSync(process.execPath, [path.join(here, 'chat.e2e.mjs')], {
    stdio: 'inherit',
    env: { ...process.env, E2E_BASE: 'http://127.0.0.1:5174', E2E_OUT: outDir },
  });
  exitCode = test.status ?? 1;
  if (exitCode !== 0) {
    console.log('\n--- server log (tail)\n' + serverLog().split('\n').slice(-30).join('\n'));
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
} finally {
  stopAll();
}
process.exit(exitCode);
