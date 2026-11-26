// End-to-end check of reading without a connection and of drafts, against
// the production build: only it registers the service worker. Starts the
// mock LLM (:8091) and a server (:8090) that serves `apps/web/dist`, chats
// once, keeps a draft, stops the server and reloads: the chat must still be
// there, read-only, with the draft. Then the server comes back.
// Usage: pnpm --filter @hearth/web build && pnpm --filter @hearth/web e2e:offline
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import puppeteer from 'puppeteer-core';

const here = path.dirname(fileURLToPath(import.meta.url));
const serverDir = path.resolve(here, '../../server');
const dist = path.resolve(here, '../dist');
const out = path.join(here, '.output');
const dataDir = mkdtempSync(path.join(tmpdir(), 'hearth-offline-'));
const BASE = 'http://127.0.0.1:8090';
mkdirSync(out, { recursive: true });
if (!existsSync(path.join(dist, 'sw.js'))) {
  console.error('No production build with sw.js: run `pnpm --filter @hearth/web build` first.');
  process.exit(1);
}

const children = [];
function start(name, args, options) {
  const child = spawn(process.execPath, args, { ...options, stdio: ['ignore', 'pipe', 'pipe'] });
  let log = '';
  child.stdout.on('data', (chunk) => (log += chunk));
  child.stderr.on('data', (chunk) => (log += chunk));
  const entry = { name, child, log: () => log };
  children.push(entry);
  return entry;
}
function stop(entry) {
  if (entry.child.exitCode !== null) {
    return;
  }
  if (process.platform === 'win32') {
    spawnSync('taskkill', ['/pid', String(entry.child.pid), '/T', '/F'], { stdio: 'ignore' });
  } else {
    entry.child.kill('SIGTERM');
  }
}
async function waitForLog(entry, pattern, timeoutMs = 60_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (pattern.test(entry.log())) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`${entry.name} did not start:\n${entry.log()}`);
}
function startServer() {
  return start('server', ['--import', 'tsx', 'src/index.ts'], {
    cwd: serverDir,
    env: {
      ...process.env,
      PORT: '8090',
      DATA_DIR: dataDir,
      PUBLIC_URL: BASE,
      WEB_DIST: dist,
      HEARTH_SETUP_CODE: 'OFFLINE-E2E',
    },
  });
}

function findBrowser() {
  const found = [
    process.env.E2E_BROWSER,
    'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/usr/bin/google-chrome',
    '/usr/bin/chromium',
  ].find((candidate) => candidate && existsSync(candidate));
  if (!found) {
    throw new Error('No Chromium-based browser found; set E2E_BROWSER to its executable');
  }
  return found;
}

const text = (value) => `::-p-text(${JSON.stringify(value)})`;
let failures = 0;
async function step(name, fn) {
  const started = Date.now();
  try {
    await fn();
    console.log(`ok   ${name} (${Date.now() - started} ms)`);
  } catch (error) {
    failures += 1;
    console.log(`FAIL ${name}: ${error instanceof Error ? error.message : error}`);
    await page?.screenshot({ path: path.join(out, `fail-offline-${name.replace(/\W+/g, '-')}.png`) }).catch(() => undefined);
    throw error;
  }
}

let browser;
let page;
const mock = start('mock', [path.join(here, 'mock-llm.mjs')], { env: { ...process.env, MOCK_PORT: '8091' } });
let server = startServer();
try {
  await waitForLog(mock, /listening/);
  await waitForLog(server, /listening/);
  browser = await puppeteer.launch({
    executablePath: findBrowser(),
    headless: true,
    args: ['--no-first-run', '--disable-extensions', '--lang=de-DE'],
    defaultViewport: { width: 1280, height: 820, deviceScaleFactor: 1 },
  });
  page = await browser.newPage();
  await page.evaluateOnNewDocument(() => {
    localStorage.setItem('hearth.locale', 'de');
    // An open live connection never lets the page become "network idle".
    localStorage.setItem('hearth.liveEvents', 'off');
  });
  const waitForText = (wanted, timeout = 10_000) =>
    page.waitForFunction((needle) => document.body.innerText.includes(needle), { timeout }, wanted);
  let chatPath = '';

  await step('an account chats once online', async () => {
    await page.goto(BASE, { waitUntil: 'networkidle0' });
    const status = await page.evaluate(async () => {
      const headers = { 'content-type': 'application/json', 'x-hearth-setup-code': 'OFFLINE-E2E' };
      const signUp = await fetch('/api/auth/sign-up/email', {
        method: 'POST',
        headers,
        body: JSON.stringify({ email: 'offline@example.com', name: 'Offline', password: 'sehr-sicheres-passwort' }),
      });
      const provider = await fetch('/api/providers', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ kind: 'openai-compatible', label: 'Mock', baseUrl: 'http://127.0.0.1:8091/v1', apiKey: 'sk-mock-0123456789' }),
      });
      return [signUp.status, provider.status];
    });
    if (status[0] !== 200 || status[1] > 201) {
      throw new Error(`sign-up/provider: ${status}`);
    }
    await page.goto(BASE, { waitUntil: 'networkidle0' });
    await page.waitForSelector(text('mock-model'), { timeout: 10_000 });
    await page.type('textarea', 'Hallo offline');
    await page.keyboard.press('Enter');
    await waitForText('Punkt zwei', 20_000);
    chatPath = new URL(page.url()).pathname;
  });

  await step('a draft survives reloading and switching chats', async () => {
    await page.type('textarea', 'Halber Gedanke');
    await page.reload({ waitUntil: 'networkidle0' });
    await page.waitForFunction(() => document.querySelector('textarea')?.value === 'Halber Gedanke', { timeout: 10_000 });
    // The start page has its own draft.
    await page.goto(BASE, { waitUntil: 'networkidle0' });
    await page.waitForSelector('textarea');
    if ((await page.$eval('textarea', (box) => box.value)) !== '') {
      throw new Error('the start page shows the chat’s draft');
    }
    await page.type('textarea', 'Neue Idee');
    await page.click(`nav a[href="${chatPath}"], a[href="${chatPath}"]`);
    await page.waitForFunction(() => document.querySelector('textarea')?.value === 'Halber Gedanke', { timeout: 10_000 });
    await page.goto(BASE, { waitUntil: 'networkidle0' });
    await page.waitForFunction(() => document.querySelector('textarea')?.value === 'Neue Idee', { timeout: 10_000 });
  });

  await step('the service worker takes over and keeps the chat', async () => {
    await page.goto(`${BASE}${chatPath}`, { waitUntil: 'networkidle0' });
    await page.evaluate(() => navigator.serviceWorker.ready);
    // The first page was loaded before the worker existed: load once more
    // under its control, so it keeps the page, the files and the chat.
    await page.reload({ waitUntil: 'networkidle0' });
    const controlled = await page.evaluate(() => Boolean(navigator.serviceWorker.controller));
    if (!controlled) {
      throw new Error('the page is not controlled by the service worker');
    }
    await waitForText('Punkt zwei');
  });

  await step('without the server the chat is still there, read-only', async () => {
    stop(server);
    await new Promise((resolve) => setTimeout(resolve, 1_000));
    await page.reload({ waitUntil: 'domcontentloaded' });
    await waitForText('Hallo offline', 15_000);
    await waitForText('Punkt zwei');
    await page.waitForSelector('[data-offline]', { timeout: 10_000 });
    await waitForText('Keine Verbindung zum Server');
    // The list in the sidebar came from the cache as well.
    await page.waitForSelector(`a[href="${chatPath}"]`, { timeout: 10_000 });
    await page.waitForFunction(() => document.querySelector('textarea')?.value === 'Halber Gedanke', { timeout: 10_000 });
    const blocked = await page.$eval('button[type=submit]', (button) => button.getAttribute('aria-disabled'));
    if (blocked !== 'true') {
      throw new Error('sending is not blocked while offline');
    }
    await page.screenshot({ path: path.join(out, '70-offline-chat.png') });
  });

  await step('the server comes back and the note goes', async () => {
    server = startServer();
    await waitForLog(server, /listening/);
    await page.waitForSelector('[data-offline]', { hidden: true, timeout: 25_000 });
    const blocked = await page.$eval('button[type=submit]', (button) => button.getAttribute('aria-disabled'));
    if (blocked === 'true') {
      throw new Error('sending is still blocked');
    }
  });

  await step('signing out forgets the kept chats and drafts', async () => {
    // Through the app's menu, so its clean-up runs.
    await page.click('button[aria-label="Benutzermenü"]');
    await page.click(`[role=menuitem]${text('Abmelden')}`);
    await page.waitForFunction(() => location.pathname === '/login', { timeout: 10_000 });
    // The login page loads the public settings again; no chat may be left.
    const state = await page.evaluate(async () => ({
      data: (await (await caches.open('hearth-data-v1')).keys()).filter((request) => request.url.includes('/api/conversations')).length,
      drafts: Object.keys(localStorage).filter((key) => key.startsWith('hearth.draft.')).length,
      session: localStorage.getItem('hearth.offlineSession') !== null,
    }));
    if (state.data > 0 || state.drafts > 0 || state.session) {
      throw new Error(`left behind after signing out: ${JSON.stringify(state)}`);
    }
  });
} catch {
  // Reported by step().
} finally {
  await browser?.close().catch(() => undefined);
  for (const entry of children) {
    stop(entry);
  }
  rmSync(dataDir, { recursive: true, force: true });
}
if (failures > 0) {
  console.log(`\n--- server log (tail)\n${server.log().split('\n').slice(-20).join('\n')}`);
}
console.log(failures === 0 ? 'Offline: alles grün.' : `${failures} Schritt(e) fehlgeschlagen.`);
process.exit(failures === 0 ? 0 : 1);
