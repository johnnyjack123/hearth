// End-to-end check of the chat flow in a headless Chromium-based browser
// against an isolated server and Vite instance started by run-e2e.mjs.
// Uses an installed browser (puppeteer-core) so nothing large is downloaded.
import { existsSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import puppeteer from 'puppeteer-core';

const BASE = process.env.E2E_BASE ?? 'http://127.0.0.1:5174';
const OUT = path.resolve(process.env.E2E_OUT ?? './shots');
mkdirSync(OUT, { recursive: true });

function findBrowser() {
  const candidates = [
    process.env.E2E_BROWSER,
    'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
    '/usr/bin/google-chrome',
    '/usr/bin/chromium',
    '/usr/bin/microsoft-edge',
  ];
  const found = candidates.find((candidate) => candidate && existsSync(candidate));
  if (!found) {
    throw new Error('No Chromium-based browser found; set E2E_BROWSER to its executable');
  }
  return found;
}

const problems = [];
const browser = await puppeteer.launch({
  executablePath: findBrowser(),
  headless: true,
  args: ['--no-first-run', '--disable-extensions', '--lang=de-DE'],
  defaultViewport: { width: 1280, height: 820, deviceScaleFactor: 1 },
});
const page = await browser.newPage();
page.on('console', (message) => {
  if (message.type() === 'error' || message.type() === 'warn') {
    problems.push(`[console.${message.type()}] ${message.text()}`);
  }
});
page.on('pageerror', (error) => problems.push(`[pageerror] ${error.message}`));
page.on('requestfailed', (request) => {
  if (!request.url().includes('/@vite') && !request.url().includes('hot-update')) {
    problems.push(`[requestfailed] ${request.method()} ${request.url()} ${request.failure()?.errorText}`);
  }
});
await page.evaluateOnNewDocument(() => localStorage.setItem('harness.locale', 'de'));

const step = async (name, fn) => {
  const started = Date.now();
  try {
    await fn();
    console.log(`ok   ${name} (${Date.now() - started} ms)`);
  } catch (error) {
    console.log(`FAIL ${name}: ${error.message}`);
    await page.screenshot({ path: path.join(OUT, `fail-${name.replace(/\W+/g, '-')}.png`) });
    throw error;
  }
};
const shot = (name) => page.screenshot({ path: path.join(OUT, `${name}.png`) });
const text = (selectorText) => `::-p-text(${selectorText})`;

try {
  await step('register first user', async () => {
    await page.goto(BASE, { waitUntil: 'networkidle0' });
    await page.waitForFunction(() => location.pathname === '/register', { timeout: 10_000 });
    await shot('01-register');
    await page.type('input[autocomplete=name]', 'Erika Test');
    await page.type('input[autocomplete=email]', 'erika@example.com');
    await page.type('input[autocomplete=new-password]', 'sehr-sicheres-passwort');
    await page.click('button[type=submit]');
    await page.waitForFunction(() => location.pathname === '/', { timeout: 10_000 });
    await page.waitForSelector(text('Noch kein API-Key hinterlegt'), { timeout: 10_000 });
    await shot('02-home-no-provider');
  });

  await step('add openai-compatible provider', async () => {
    await page.click(text('API-Key hinzufügen'));
    await page.waitForFunction(() => location.pathname === '/settings', { timeout: 10_000 });
    await page.click(text('Anbieter hinzufügen'));
    await page.waitForSelector('[role=dialog]');
    await page.click(`[role=dialog] ${text('OpenAI-kompatibel')}`);
    const label = await page.$('[role=dialog] input:not([type=radio]):not([type=url]):not([type=password])');
    await label.click({ clickCount: 3 });
    await label.type('Mock');
    await page.type('[role=dialog] input[type=url]', 'http://127.0.0.1:8091/v1');
    await shot('03-provider-dialog');
    await page.click('[role=dialog] button[type=submit]');
    await page.waitForSelector(text('1 Modelle verfügbar'), { timeout: 10_000 });
    await shot('04-provider-added');
  });

  let conversationPath = '';
  await step('first message from the start page', async () => {
    await page.goto(BASE, { waitUntil: 'networkidle0' });
    await page.waitForSelector('textarea');
    await page.waitForSelector(text('mock-model'), { timeout: 10_000 });
    await shot('05-home');
    await page.type('textarea', 'Hallo, wer bist du?');
    await page.keyboard.press('Enter');
    await page.waitForFunction(() => location.pathname.startsWith('/c/'), { timeout: 10_000 });
    conversationPath = new URL(page.url()).pathname;
    await page.waitForSelector(text('Hallo, wer bist du?'), { timeout: 10_000 });
    await page.waitForSelector(text('Punkt zwei'), { timeout: 15_000 });
    await new Promise((resolve) => setTimeout(resolve, 800));
    await shot('06-first-reply');
  });

  await step('second message in the chat view', async () => {
    await page.type('textarea', 'Und noch eine Frage');
    await page.keyboard.press('Enter');
    await page.waitForSelector(text('Und noch eine Frage'), { timeout: 10_000 });
    await page.waitForFunction(() => document.body.innerText.split('Punkt zwei').length - 1 >= 2, { timeout: 15_000 });
    await new Promise((resolve) => setTimeout(resolve, 3_000));
    await shot('07-second-reply');
  });

  await step('history is persisted on the server', async () => {
    const id = conversationPath.split('/').at(-1);
    const detail = await page.evaluate(async (conversationId) => {
      const response = await fetch(`/api/conversations/${conversationId}`);
      return response.json();
    }, id);
    const roles = detail.messages.map((message) => message.role).join(',');
    if (roles !== 'user,assistant,user,assistant') {
      throw new Error(`unexpected roles ${roles}`);
    }
    console.log(`     title="${detail.title}"`);
  });

  await step('reload shows the stored chat', async () => {
    await page.reload({ waitUntil: 'networkidle0' });
    await page.waitForSelector(text('Und noch eine Frage'), { timeout: 10_000 });
  });

  await step('pair a device that then uses the LLM through the server', async () => {
    // What `harness login` does: start a pairing without any session.
    const json = { 'content-type': 'application/json' };
    const pairing = await fetch(`${BASE}/api/pairing`, {
      method: 'POST',
      headers: json,
      body: JSON.stringify({ name: 'harness auf E2E' }),
    }).then((response) => response.json());
    await page.goto(pairing.verificationUriComplete, { waitUntil: 'networkidle0' });
    await page.waitForSelector(text('„harness auf E2E“ möchte sich mit deinem Konto verbinden.'), { timeout: 10_000 });
    await page.waitForSelector(text(pairing.userCode));
    await shot('12-pairing');
    await page.click(text('Verbinden'));
    await page.waitForSelector(text('Verbunden. Du kannst zum Terminal zurückkehren.'), { timeout: 10_000 });
    await shot('13-paired');

    const poll = await fetch(`${BASE}/api/pairing/token`, {
      method: 'POST',
      headers: json,
      body: JSON.stringify({ deviceCode: pairing.deviceCode }),
    }).then((response) => response.json());
    if (poll.status !== 'approved') {
      throw new Error(`pairing poll returned ${poll.status}`);
    }
    const device = { authorization: `Bearer ${poll.token}` };
    const providers = await fetch(`${BASE}/api/llm/providers`, { headers: device }).then((response) => response.json());
    const mock = providers.find((provider) => provider.kind === 'openai-compatible');
    if (!mock) {
      throw new Error(`no provider for the device: ${JSON.stringify(providers)}`);
    }
    const completion = await fetch(`${BASE}/api/llm/proxy/${mock.id}/chat/completions`, {
      method: 'POST',
      headers: { ...device, ...json },
      body: JSON.stringify({ model: 'mock-model', messages: [{ role: 'user', content: 'Hallo' }] }),
    });
    const answer = await completion.json();
    if (!completion.ok || !answer.choices?.[0]?.message?.content) {
      throw new Error(`pass-through failed: ${completion.status} ${JSON.stringify(answer)}`);
    }
    console.log(`     device got: "${answer.choices[0].message.content}"`);

    await page.goto(`${BASE}/settings?tab=devices`, { waitUntil: 'networkidle0' });
    await page.waitForSelector(text('harness auf E2E'), { timeout: 10_000 });
    await shot('14-devices');
  });

  await step('pairing page after sign-in redirect', async () => {
    const context = await browser.createBrowserContext();
    const fresh = await context.newPage();
    await fresh.evaluateOnNewDocument(() => localStorage.setItem('harness.locale', 'de'));
    await fresh.goto(`${BASE}/device?code=ABCD-EFGH`, { waitUntil: 'networkidle0' });
    await fresh.waitForFunction(() => location.pathname === '/login', { timeout: 10_000 });
    await fresh.type('input[autocomplete=email]', 'erika@example.com');
    await fresh.type('input[autocomplete=current-password]', 'sehr-sicheres-passwort');
    await fresh.click('button[type=submit]');
    await fresh.waitForFunction(() => location.pathname === '/device' && location.search.includes('ABCD-EFGH'), {
      timeout: 10_000,
    });
    await fresh.waitForSelector(text('Diesen Code gibt es nicht. Prüfe die Eingabe.'), { timeout: 10_000 });
    await fresh.screenshot({ path: path.join(OUT, '15-pairing-unknown-code.png') });
    await context.close();
  });

  await step('dark mode and mobile screenshots', async () => {
    await page.evaluate(() => localStorage.setItem('harness.theme', 'dark'));
    await page.goto(`${BASE}${conversationPath}`, { waitUntil: 'networkidle0' });
    await page.waitForSelector(text('Und noch eine Frage'));
    await new Promise((resolve) => setTimeout(resolve, 500));
    await shot('08-chat-dark');
    await page.goto(`${BASE}/settings?tab=appearance`, { waitUntil: 'networkidle0' });
    await shot('09-settings-dark');
    await page.setViewport({ width: 390, height: 844, deviceScaleFactor: 1, isMobile: true, hasTouch: true });
    await page.goto(`${BASE}${conversationPath}`, { waitUntil: 'networkidle0' });
    await page.waitForSelector(text('Und noch eine Frage'));
    await shot('10-chat-mobile-dark');
    await page.evaluate(() => localStorage.setItem('harness.theme', 'light'));
    await page.goto(BASE, { waitUntil: 'networkidle0' });
    await shot('11-home-mobile-light');
  });
} finally {
  console.log(`\nconsole/page problems: ${problems.length}`);
  for (const problem of problems.slice(0, 30)) {
    console.log(`  ${problem}`);
  }
  await browser.close();
}
