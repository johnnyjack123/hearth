import type { ChatFile, ConversationDetail, ConversationSummary, ProviderCredential } from '@harness/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import type { Kysely } from 'kysely';
import type { DB } from '../src/db/schema.ts';
import { cleanFileName, detectFile } from '../src/files/detect.ts';
import { FileStore } from '../src/files/store.ts';
import { HttpError } from '../src/http/errors.ts';
import type { ChatMessage } from '../src/services/conversations.ts';
import { createScriptedLlm, createTestServer, ORIGIN, readChunks, type TestServer } from './helpers.ts';

const PNG = Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52]);
const PDF = new TextEncoder().encode('%PDF-1.4\n1 0 obj\n<< >>\nendobj\n%%EOF\n');
const MARKDOWN = '# Bericht\n\nKaffee wird bei 200 °C geröstet.';

describe('detectFile', () => {
  it('goes by the contents, not the name', () => {
    expect(detectFile('bild.txt', PNG)).toEqual({ kind: 'image', mediaType: 'image/png' });
    expect(detectFile('x', Uint8Array.from([0xff, 0xd8, 0xff, 0xe0]))).toEqual({ kind: 'image', mediaType: 'image/jpeg' });
    expect(detectFile('bericht.pdf', PDF)).toEqual({ kind: 'document', mediaType: 'application/pdf' });
    expect(detectFile('notizen.md', new TextEncoder().encode('# Hallo\nÄpfel'))).toEqual({ kind: 'text', mediaType: 'text/plain' });
    expect(detectFile('seite.html', new TextEncoder().encode('<p>x</p>'))).toEqual({ kind: 'document', mediaType: 'text/html' });
    // Office files are zips; the extension says which.
    const zip = Uint8Array.from([0x50, 0x4b, 0x03, 0x04, 1, 2, 3]);
    expect(detectFile('a.docx', zip)?.kind).toBe('document');
    expect(detectFile('a.zip', zip)).toBeNull();
    // Programs, binary data and broken UTF-8 are no text.
    expect(detectFile('tool.exe', Uint8Array.from([0x4d, 0x5a, 0x90, 0]))).toBeNull();
    expect(detectFile('kaputt.txt', Uint8Array.from([0x61, 0xff, 0xfe, 0x62]))).toBeNull();
  });

  it('keeps names short and plain', () => {
    expect(cleanFileName(encodeURIComponent('C:\\Users\\x\\Bericht ä.pdf'))).toBe('Bericht ä.pdf');
    expect(cleanFileName('a\u{202E}fdp.exe')).toBe('afdp.exe');
    expect(cleanFileName('')).toBe('Datei');
    expect(cleanFileName(`${'x'.repeat(300)}.pdf`)).toHaveLength(200);
  });
});

describe('FileStore', () => {
  it('lets the server start without a writable data directory and says so on upload', async () => {
    // A file where the directory should be: it cannot be created.
    const dataDir = mkdtempSync(path.join(tmpdir(), 'harness-files-'));
    writeFileSync(path.join(dataDir, 'files'), 'in the way');
    const store = new FileStore({} as Kysely<DB>, dataDir);
    const upload = { userId: 'u1', name: 'a.txt', mediaType: 'text/plain', kind: 'text' as const, text: 'a', truncated: false };
    const failure = await store.save(upload, new Uint8Array([97])).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(HttpError);
    expect(failure).toMatchObject({ status: 503, code: 'storage_unavailable' });
  });
});

describe('files in chats', () => {
  let server: TestServer;
  let llm: ReturnType<typeof createScriptedLlm>;
  let cookie: string;
  let credential: ProviderCredential;
  let converted: string[];

  beforeEach(async () => {
    llm = createScriptedLlm(() => [{ text: 'Gesehen.' }]);
    converted = [];
    server = await createTestServer('sqlite', {
      llm,
      converter: {
        async convert(name) {
          converted.push(name);
          if (name.startsWith('leer')) {
            return '  ';
          }
          if (name.startsWith('kaputt')) {
            throw new HttpError(422, 'convert_failed', 'The document could not be read: broken');
          }
          return MARKDOWN;
        },
      },
    });
    cookie = await server.signUp('alice@example.com');
    credential = (await (
      await server.request('POST', '/api/providers', { cookie, body: { kind: 'openai', label: 'Work', apiKey: 'sk-test-0123456789abcdef' } })
    ).json()) as ProviderCredential;
  });

  afterEach(async () => {
    await server.close();
  });

  async function upload(name: string, bytes: Uint8Array, as = cookie): Promise<Response> {
    return server.app.request('/api/files', {
      method: 'POST',
      headers: { origin: ORIGIN, cookie: as, 'content-type': 'application/octet-stream', 'x-file-name': encodeURIComponent(name) },
      body: bytes,
    });
  }

  async function uploaded(name: string, bytes: Uint8Array): Promise<ChatFile> {
    const response = await upload(name, bytes);
    expect(response.status).toBe(201);
    return (await response.json()) as ChatFile;
  }

  async function chat(conversationId: string, parts: unknown[], modelId = 'mock-1') {
    return server.app.request(`/api/conversations/${conversationId}/chat`, {
      method: 'POST',
      headers: { origin: ORIGIN, cookie, 'content-type': 'application/json' },
      body: JSON.stringify({ message: { id: `m-${Math.random().toString(36).slice(2)}`, role: 'user', parts }, credentialId: credential.id, modelId }),
    });
  }

  async function newConversation(): Promise<string> {
    return ((await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary).id;
  }

  it('takes images, documents and text files, and refuses the rest', async () => {
    const image = await uploaded('foto.png', PNG);
    expect(image).toMatchObject({ name: 'foto.png', kind: 'image', mediaType: 'image/png', size: PNG.length });
    const document = await uploaded('bericht.pdf', PDF);
    expect(document).toMatchObject({ kind: 'document', mediaType: 'application/pdf', chars: MARKDOWN.length });
    expect(converted).toEqual(['bericht.pdf']);
    const text = await uploaded('notiz.txt', new TextEncoder().encode('Einkaufen: Bohnen'));
    expect(text).toMatchObject({ kind: 'text', chars: 17 });

    const bytes = await server.app.request(`/api/files/${image.id}`, { headers: { cookie } });
    expect(bytes.headers.get('content-type')).toBe('image/png');
    expect(new Uint8Array(await bytes.arrayBuffer())).toEqual(PNG);
    const pdf = await server.app.request(`/api/files/${document.id}`, { headers: { cookie } });
    expect(pdf.headers.get('content-type')).toBe('application/octet-stream');
    expect(pdf.headers.get('content-disposition')).toMatch(/^attachment/);
    expect(await (await server.app.request(`/api/files/${document.id}/text`, { headers: { cookie } })).text()).toContain('200 °C');

    expect((await upload('tool.exe', Uint8Array.from([0x4d, 0x5a, 0, 1]))).status).toBe(415);
    expect((await upload('leer.txt', new Uint8Array())).status).toBe(422);
    expect(((await (await upload('leer.pdf', PDF)).json()) as { error: { code: string } }).error.code).toBe('no_text');
    expect(((await (await upload('kaputt.pdf', PDF)).json()) as { error: { code: string } }).error.code).toBe('convert_failed');
    expect((await upload('gross.txt', new Uint8Array(20 * 1024 * 1024 + 1).fill(0x61))).status).toBe(413);

    // Only the owner reads a file.
    const bob = await server.signUp('bob@example.com');
    expect((await server.app.request(`/api/files/${image.id}`, { headers: { cookie: bob } })).status).toBe(404);
  });

  it('gives the model images as data and documents as text, stored as references', async () => {
    const image = await uploaded('foto.png', PNG);
    const document = await uploaded('bericht.pdf', PDF);
    const id = await newConversation();
    const response = await chat(id, [
      { type: 'text', text: 'Was siehst du?' },
      // The browser's name and type do not count; the stored ones do.
      { type: 'file', url: `/api/files/${image.id}`, mediaType: 'text/html', filename: 'anders.html' },
      { type: 'file', url: `/api/files/${document.id}`, mediaType: 'application/pdf', filename: 'bericht.pdf' },
    ]);
    expect(response.status).toBe(200);
    await readChunks(response);

    const user = llm.calls[0]?.prompt.findLast((message) => message.role === 'user');
    const content = user?.content as { type: string; mediaType?: string; text?: string; data?: unknown }[];
    expect(content.find((part) => part.type === 'file')).toMatchObject({ mediaType: 'image/png' });
    expect(content.some((part) => part.type === 'text' && part.text?.includes('<file>\n# Bericht'))).toBe(true);

    const detail = (await (await server.request('GET', `/api/conversations/${id}`, { cookie })).json()) as ConversationDetail<ChatMessage>;
    expect(detail.messages[0]?.parts).toEqual([
      { type: 'text', text: 'Was siehst du?' },
      { type: 'file', url: `/api/files/${image.id}`, mediaType: 'image/png', filename: 'foto.png' },
      { type: 'file', url: `/api/files/${document.id}`, mediaType: 'application/pdf', filename: 'bericht.pdf' },
    ]);

    // A model the catalog calls blind gets a note instead of the image.
    await readChunks(await chat(id, [{ type: 'text', text: 'Und jetzt?' }], 'mock-blind'));
    expect(JSON.stringify(llm.calls.at(-1)?.prompt)).toContain('the current model cannot see images');

    // Deleting the chat removes its files.
    expect((await server.request('DELETE', `/api/conversations/${id}`, { cookie })).status).toBe(204);
    expect((await server.app.request(`/api/files/${image.id}`, { headers: { cookie } })).status).toBe(404);
  });

  it('refuses files that are not the sender’s uploads, and too many', async () => {
    const id = await newConversation();
    expect((await chat(id, [{ type: 'file', url: 'https://example.com/x.png', mediaType: 'image/png' }])).status).toBe(400);

    const bob = await server.signUp('bob@example.com');
    const theirs = (await (await upload('foto.png', PNG, bob)).json()) as ChatFile;
    expect((await chat(id, [{ type: 'file', url: `/api/files/${theirs.id}`, mediaType: 'image/png' }])).status).toBe(400);

    const mine = await uploaded('foto.png', PNG);
    const eleven = Array.from({ length: 11 }, () => ({ type: 'file', url: `/api/files/${mine.id}`, mediaType: 'image/png' }));
    expect((await chat(id, eleven)).status).toBe(400);

    // A file alone is a message; its name becomes the title.
    expect((await chat(id, [{ type: 'file', url: `/api/files/${mine.id}`, mediaType: 'image/png' }])).status).toBe(200);
  });
});
