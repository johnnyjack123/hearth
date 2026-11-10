import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { checkDatabaseUrl, loadConfig, startupHint } from '../src/config.ts';
import { createSecretBox, keyHint } from '../src/crypto.ts';
import { parseDatabaseUrl } from '../src/db/index.ts';
import { agentToolsAsText, fallbackTitle } from '../src/routes/chat.ts';

describe('secret box', () => {
  const box = createSecretBox('unit-test-secret-0123456789-0123456789');

  it('round-trips a value', () => {
    const token = box.encrypt('sk-secret', 'user:cred');
    expect(token).not.toContain('sk-secret');
    expect(box.decrypt(token, 'user:cred')).toBe('sk-secret');
  });

  it('rejects a token used with a different context', () => {
    const token = box.encrypt('sk-secret', 'user-a:cred');
    expect(() => box.decrypt(token, 'user-b:cred')).toThrow();
  });

  it('rejects a tampered token', () => {
    const token = box.encrypt('sk-secret', 'ctx');
    const parts = token.split('.');
    const ciphertext = Buffer.from(parts[3] ?? '', 'base64url');
    ciphertext[0] = (ciphertext[0] ?? 0) ^ 0xff;
    parts[3] = ciphertext.toString('base64url');
    expect(() => box.decrypt(parts.join('.'), 'ctx')).toThrow();
  });

  it('cannot decrypt with another server secret', () => {
    const token = box.encrypt('sk-secret', 'ctx');
    expect(() => createSecretBox('another-secret-0123456789-0123456789').decrypt(token, 'ctx')).toThrow();
  });

  it('shows only the last four characters as hint', () => {
    expect(keyHint('sk-1234567890abcd')).toBe('…abcd');
    expect(keyHint('short')).toBe('…');
  });
});

describe('agentToolsAsText', () => {
  it('turns agent tool calls into text and drops display-only parts', () => {
    const [reply] = agentToolsAsText([
      {
        id: 'a1',
        role: 'assistant',
        parts: [
          { type: 'text', text: 'Ich schaue nach.' },
          {
            type: 'dynamic-tool',
            toolName: 'bash',
            toolCallId: 'c1',
            state: 'output-available',
            input: { command: 'ls' },
            output: { text: 'README.md' },
          },
          { type: 'data-notice', data: { level: 'info', text: 'x' } },
        ],
      },
    ]);
    expect(reply?.parts).toEqual([
      { type: 'text', text: 'Ich schaue nach.' },
      { type: 'text', text: '[The agent in the linked environment ran bash {"command":"ls"}; output: README.md]' },
    ]);
  });
});

describe('loadConfig', () => {
  it('treats empty variables as unset, as Docker Compose passes them', () => {
    const dataDir = mkdtempSync(path.join(tmpdir(), 'hearth-config-'));
    const config = loadConfig({
      DATA_DIR: dataDir,
      DATABASE_URL: '',
      HEARTH_SECRET: '',
      TRUSTED_ORIGINS: '',
      WEB_DIST: '',
      NODE_ENV: 'production',
    });
    expect(config.databaseUrl).toBe(`sqlite:${path.join(dataDir, 'hearth.db')}`);
    expect(config.secret.length).toBeGreaterThanOrEqual(32);
    expect(config.trustedOrigins).toEqual(['http://localhost:8080']);
  });

  it('reads the variables and the SQLite file of a server from before the rename', () => {
    const dataDir = mkdtempSync(path.join(tmpdir(), 'harness-config-'));
    writeFileSync(path.join(dataDir, 'harness.db'), '');
    const secret = 'an-old-secret-that-is-long-enough-0123456789';
    const config = loadConfig({ DATA_DIR: dataDir, HARNESS_SECRET: secret, HARNESS_SETUP_CODE: 'OLDC-ODE1' });
    expect(config.secret).toBe(secret);
    expect(config.setupCode).toBe('OLDC-ODE1');
    expect(config.databaseUrl).toBe(`sqlite:${path.join(dataDir, 'harness.db')}`);
    // The new name wins when both are set.
    expect(loadConfig({ DATA_DIR: dataDir, HARNESS_SECRET: secret, HEARTH_SECRET: `${secret}-new` }).secret).toBe(`${secret}-new`);
  });

  it('explains a Postgres URL with an unencoded password without repeating it', () => {
    const base64Password = 'postgres://hearth:Ab/cd+ef=@postgres:5432/hearth';
    expect(() => checkDatabaseUrl(base64Password)).toThrow(/percent-encoded/);
    expect(() => checkDatabaseUrl(base64Password)).not.toThrow(/Ab\/cd/);
    expect(() => checkDatabaseUrl('postgres://hearth:Ab%2Fcd%2Bef%3D@postgres:5432/hearth')).not.toThrow();
    expect(() => checkDatabaseUrl('postgres://hearth:0a1b2c3d@postgres:5432/hearth')).not.toThrow();
    expect(() => checkDatabaseUrl('sqlite:./data/hearth.db')).not.toThrow();
  });

  it('points at the compose profile when the Postgres service is missing', () => {
    const missing = Object.assign(new Error('getaddrinfo ENOTFOUND postgres'), { code: 'ENOTFOUND', hostname: 'postgres' });
    expect(startupHint(missing)).toMatch(/COMPOSE_PROFILES=postgres/);
    expect(startupHint(new Error('other'))).toBeNull();
    expect(startupHint(undefined)).toBeNull();
  });
});

describe('parseDatabaseUrl', () => {
  it.each([
    ['postgres://u:p@host:5432/db', { kind: 'postgres', url: 'postgres://u:p@host:5432/db' }],
    ['postgresql://host/db', { kind: 'postgres', url: 'postgresql://host/db' }],
    ['sqlite:./data/hearth.db', { kind: 'sqlite', file: './data/hearth.db' }],
    ['sqlite:/data/hearth.db', { kind: 'sqlite', file: '/data/hearth.db' }],
    ['file:C:/data/hearth.db', { kind: 'sqlite', file: 'C:/data/hearth.db' }],
    ['sqlite::memory:', { kind: 'sqlite', file: ':memory:' }],
  ])('parses %s', (url, expected) => {
    expect(parseDatabaseUrl(url)).toEqual(expected);
  });

  it('rejects unknown schemes', () => {
    expect(() => parseDatabaseUrl('mysql://host/db')).toThrow(/Unsupported/);
  });
});

describe('fallbackTitle', () => {
  it('keeps short messages', () => {
    expect(fallbackTitle('  Hallo\nWelt ')).toBe('Hallo Welt');
  });

  it('cuts long messages at a word boundary', () => {
    const title = fallbackTitle('Wie kann ich einen Container unter Windows starten, ohne dass Docker Desktop installiert ist?');
    expect(title.length).toBeLessThanOrEqual(61);
    expect(title.endsWith('…')).toBe(true);
    expect(title).not.toMatch(/\s…$/);
  });
});
