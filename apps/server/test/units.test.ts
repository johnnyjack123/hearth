import { describe, expect, it } from 'vitest';
import { createSecretBox, keyHint } from '../src/crypto.ts';
import { parseDatabaseUrl } from '../src/db/index.ts';
import { fallbackTitle } from '../src/routes/chat.ts';

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

describe('parseDatabaseUrl', () => {
  it.each([
    ['postgres://u:p@host:5432/db', { kind: 'postgres', url: 'postgres://u:p@host:5432/db' }],
    ['postgresql://host/db', { kind: 'postgres', url: 'postgresql://host/db' }],
    ['sqlite:./data/harness.db', { kind: 'sqlite', file: './data/harness.db' }],
    ['sqlite:/data/harness.db', { kind: 'sqlite', file: '/data/harness.db' }],
    ['file:C:/data/harness.db', { kind: 'sqlite', file: 'C:/data/harness.db' }],
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
