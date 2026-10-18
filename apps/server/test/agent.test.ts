import type { AddressInfo } from 'node:net';
import { serve } from '@hono/node-server';
import {
  type AgentEvent,
  type ConversationDetail,
  type ConversationSummary,
  type DeviceSummary,
  type PairingPoll,
  type PairingStart,
  type ProviderCredential,
  type ServerMessage,
  TUNNEL_PATH,
  TUNNEL_VERSION,
} from '@harness/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { WebSocket } from 'ws';
import { ChunkMapper, transcript } from '../src/routes/agent.ts';
import type { ChatMessage } from '../src/services/conversations.ts';
import { DeviceHub, type TunnelConnection, type TunnelSocket } from '../src/tunnel/hub.ts';
import { attachTunnel } from '../src/tunnel/socket.ts';
import { createTestServer, DIALECTS, eventually, type TestServer } from './helpers.ts';

const CLI = { origin: '' };
const PROJECT = { name: 'demo', harnesses: ['pi'] };

/** A device on the other end of the tunnel, scripted by the test. */
class FakeDevice implements TunnelSocket {
  readonly received: ServerMessage[] = [];
  closedWith: number | undefined;
  connection: TunnelConnection | undefined;
  onPrompt: ((prompt: Extract<ServerMessage, { type: 'session.prompt' }>) => void) | undefined;

  send(data: string): void {
    const message = JSON.parse(data) as ServerMessage;
    this.received.push(message);
    if (message.type === 'session.prompt') {
      // Answers after startRun returned, like a real device would.
      setTimeout(() => this.onPrompt?.(message), 0);
    }
  }

  close(code?: number): void {
    this.closedWith = code;
  }

  say(message: unknown): void {
    this.connection?.receive(JSON.stringify(message));
  }

  hello(projects = [PROJECT]): void {
    this.say({ type: 'hello', version: TUNNEL_VERSION, projects });
  }

  emit(session: string, ...events: AgentEvent[]): void {
    for (const event of events) {
      this.say({ type: 'session.event', session, event });
    }
  }
}

function connect(hub: DeviceHub, deviceId: string, userId = 'user-1'): FakeDevice {
  const device = new FakeDevice();
  device.connection = hub.attach(deviceId, userId, device);
  return device;
}

describe('ChunkMapper', () => {
  it('turns agent events into UI chunks and sums usage', () => {
    const mapper = new ChunkMapper();
    const chunks = [
      { type: 'turn.start' },
      { type: 'text.start', id: 't1' },
      { type: 'text.delta', id: 't1', delta: 'Hallo' },
      { type: 'text.end', id: 't1' },
      { type: 'tool.start', id: 'c1', name: 'bash', input: { command: 'ls' } },
      { type: 'tool.update', id: 'c1', output: 'a' },
      { type: 'tool.end', id: 'c1', output: 'a\nb', isError: false },
      { type: 'usage', inputTokens: 10, outputTokens: 2, cost: 0.01, contextWindow: 1000 },
      { type: 'usage', inputTokens: 5, outputTokens: 1, cost: 0.02, cachedInputTokens: 4 },
      { type: 'approval', id: 3, host: 'example.com', port: 443, state: 'pending' },
      { type: 'progress', id: 'vm', label: 'VM starten', state: 'done', detail: '21.0 s' },
      { type: 'turn.end' },
    ].flatMap((event) => mapper.map(event as AgentEvent));
    expect(chunks.map((chunk) => chunk.type)).toEqual([
      'start-step',
      'text-start',
      'text-delta',
      'text-end',
      'tool-input-available',
      'tool-output-available',
      'tool-output-available',
      'data-approval',
      'data-progress',
      'finish-step',
    ]);
    expect(chunks[8]).toEqual({
      type: 'data-progress',
      id: 'progress-vm',
      data: { id: 'vm', label: 'VM starten', state: 'done', detail: '21.0 s' },
    });
    expect(chunks[5]).toMatchObject({ preliminary: true, output: { text: 'a' } });
    expect(chunks[7]).toMatchObject({ id: 'approval-3', data: { host: 'example.com', state: 'pending' } });
    // Tokens and cost add up; the context is what the last call saw.
    expect(mapper.usage).toEqual({
      inputTokens: 15,
      outputTokens: 3,
      cachedInputTokens: 4,
      contextTokens: 6,
      contextWindow: 1000,
      cost: 0.03,
    });
  });

  it('drops output for unknown tool calls and closes calls left open', () => {
    const mapper = new ChunkMapper();
    expect(mapper.map({ type: 'tool.end', id: 'nope', output: 'x', isError: false })).toEqual([]);
    mapper.map({ type: 'tool.start', id: 'c1', name: 'bash', input: {} });
    expect(mapper.closeOpenTools()).toEqual([
      { type: 'tool-output-error', toolCallId: 'c1', errorText: 'interrupted', dynamic: true },
    ]);
    expect(mapper.closeOpenTools()).toEqual([]);
  });
});

describe('transcript', () => {
  it('passes earlier text on and keeps the newest part', () => {
    const messages: ChatMessage[] = [
      { id: '1', role: 'user', parts: [{ type: 'text', text: 'Frage' }] },
      { id: '2', role: 'assistant', parts: [{ type: 'reasoning', text: 'hm' }, { type: 'text', text: 'Antwort' }] },
    ];
    expect(transcript(messages)).toBe('User: Frage\n\nAssistant: Antwort');
    expect(transcript([])).toBeNull();
    const long: ChatMessage[] = [{ id: '1', role: 'user', parts: [{ type: 'text', text: 'x'.repeat(30_000) }] }];
    expect(transcript(long)?.length).toBe(20_001);
  });
});

describe('DeviceHub', () => {
  it('greets, expects a hello first and checks the protocol version', () => {
    const hub = new DeviceHub();
    const early = connect(hub, 'd1');
    expect(early.received).toEqual([{ type: 'welcome', version: TUNNEL_VERSION }]);
    early.say({ type: 'projects', projects: [] });
    expect(early.closedWith).toBe(4002);
    expect(hub.status('d1').online).toBe(false);

    const old = connect(hub, 'd2');
    old.say({ type: 'hello', version: TUNNEL_VERSION + 1, projects: [] });
    expect(old.closedWith).toBe(4001);

    const good = connect(hub, 'd3');
    good.hello();
    expect(hub.status('d3')).toEqual({ online: true, projects: [PROJECT], vm: 'unknown' });
    good.say({ type: 'status', vm: 'stopped' });
    expect(hub.status('d3').vm).toBe('stopped');
    good.say({ type: 'session.event', session: 's', event: { type: 'nope' } });
    expect(good.closedWith).toBe(4002);
    expect(hub.status('d3').online).toBe(false);
  });

  it('only accepts events for its own sessions and survives a replaced socket', async () => {
    const hub = new DeviceHub();
    const device = connect(hub, 'd1');
    device.hello();
    const other = connect(hub, 'd2');
    other.hello();

    const prompt = {
      type: 'session.prompt' as const,
      session: 's1',
      project: 'demo',
      harness: 'pi',
      conversation: { id: 's1', title: null },
      text: 'hi',
      history: null,
      model: null,
    };
    // Whether a project may run is the device's call, not the hub's.
    expect(() => hub.startRun('offline', prompt)).toThrow(/not connected/);
    const run = hub.startRun('d1', prompt);
    expect(() => hub.startRun('d1', prompt)).toThrow(/still answering/);

    // Another device cannot write into this session.
    other.emit('s1', { type: 'text.start', id: 'x' });
    // A reconnect of the same device ends the run on the old socket ...
    const fresh = connect(hub, 'd1');
    expect(device.closedWith).toBe(4000);
    const events: AgentEvent[] = [];
    for await (const event of run) {
      events.push(event);
    }
    expect(events).toEqual([{ type: 'error', message: 'device_replaced' }]);

    // ... and the old socket's late close does not touch the new one.
    fresh.hello();
    const second = hub.startRun('d1', prompt);
    device.connection?.closed();
    expect(hub.status('d1').online).toBe(true);
    fresh.emit('s1', { type: 'text.start', id: 't' }, { type: 'done', aborted: false });
    const types: string[] = [];
    for await (const event of second) {
      types.push(event.type);
    }
    expect(types).toEqual(['text.start', 'done']);
  });
});

describe.each(DIALECTS)('linked chats on %s', (kind) => {
  let server: TestServer;
  let cookie: string;
  let credential: ProviderCredential;

  beforeEach(async () => {
    server = await createTestServer(kind);
    cookie = await server.signUp('owner@example.com');
    const response = await server.request('POST', '/api/providers', {
      cookie,
      body: { kind: 'openai', label: 'Work', apiKey: 'sk-test-0123456789abcdef' },
    });
    credential = (await response.json()) as ProviderCredential;
  });

  afterEach(async () => {
    await server.close();
  });

  async function pair(as = cookie): Promise<{ token: string; deviceId: string; userId: string }> {
    const start = (await (
      await server.request('POST', '/api/pairing', { body: { name: 'harness auf TEST' }, headers: CLI })
    ).json()) as PairingStart;
    await server.request('POST', `/api/devices/pairing/${start.userCode}`, { cookie: as, body: { approve: true } });
    const poll = (await (
      await server.request('POST', '/api/pairing/token', { body: { deviceCode: start.deviceCode }, headers: CLI })
    ).json()) as PairingPoll;
    if (poll.status !== 'approved') {
      throw new Error('not approved');
    }
    const row = await server.database.db
      .selectFrom('device')
      .select('userId')
      .where('id', '=', poll.deviceId)
      .executeTakeFirstOrThrow();
    return { token: poll.token, deviceId: poll.deviceId, userId: row.userId };
  }

  async function newConversation(): Promise<ConversationSummary> {
    return (await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary;
  }

  function chat(conversationId: string, text: string, id = `m-${Math.random().toString(36).slice(2)}`) {
    return server.request('POST', `/api/conversations/${conversationId}/chat`, {
      cookie,
      body: {
        message: { id, role: 'user', parts: [{ type: 'text', text }] },
        credentialId: credential.id,
        modelId: 'mock-1',
      },
    });
  }

  async function load(conversationId: string): Promise<ConversationDetail<ChatMessage>> {
    return (await (
      await server.request('GET', `/api/conversations/${conversationId}`, { cookie })
    ).json()) as ConversationDetail<ChatMessage>;
  }

  it('shows the device online and links a chat to one of its projects', async () => {
    const { deviceId, userId } = await pair();
    const conversation = await newConversation();
    const link = { deviceId, project: 'demo', harness: 'pi' };

    const offline = await server.request('PUT', `/api/conversations/${conversation.id}/link`, { cookie, body: link });
    expect(offline.status).toBe(409);

    const device = connect(server.hub, deviceId, userId);
    device.hello();
    const devices = (await (await server.request('GET', '/api/devices', { cookie })).json()) as DeviceSummary[];
    expect(devices[0]).toMatchObject({ online: true, projects: [PROJECT] });

    const wrong = await server.request('PUT', `/api/conversations/${conversation.id}/link`, {
      cookie,
      body: { ...link, project: 'secret' },
    });
    expect(wrong.status).toBe(409);
    const linked = await server.request('PUT', `/api/conversations/${conversation.id}/link`, { cookie, body: link });
    expect(linked.status).toBe(200);
    expect(((await linked.json()) as ConversationSummary).link).toEqual({
      ...link,
      deviceName: 'harness auf TEST',
    });

    // Someone else's device cannot be linked.
    const mallory = await server.signUp('mallory@example.com');
    const foreign = (await (await server.request('POST', '/api/conversations', { cookie: mallory, body: {} })).json()) as ConversationSummary;
    const stolen = await server.request('PUT', `/api/conversations/${foreign.id}/link`, { cookie: mallory, body: link });
    expect(stolen.status).toBe(404);

    expect((await server.request('DELETE', `/api/conversations/${conversation.id}/link`, { cookie })).status).toBe(204);
    expect(device.received.at(-1)).toEqual({ type: 'session.close', session: conversation.id });
    expect((await load(conversation.id)).link).toBeNull();
  });

  it('streams the agent reply into the chat and stores it', async () => {
    const { deviceId, userId } = await pair();
    const device = connect(server.hub, deviceId, userId);
    device.hello();
    const conversation = await newConversation();
    await server.request('PUT', `/api/conversations/${conversation.id}/link`, {
      cookie,
      body: { deviceId, project: 'demo', harness: 'pi' },
    });

    device.onPrompt = (prompt) => {
      device.emit(
        prompt.session,
        { type: 'turn.start' },
        { type: 'tool.start', id: 'call-1', name: 'bash', input: { command: 'ls' } },
        { type: 'approval', id: 7, host: 'example.com', port: 443, state: 'pending' },
        { type: 'tool.end', id: 'call-1', output: 'README.md', isError: false },
        { type: 'text.start', id: 't1' },
        { type: 'text.delta', id: 't1', delta: 'Fertig.' },
        { type: 'text.end', id: 't1' },
        { type: 'usage', inputTokens: 3, outputTokens: 4 },
        { type: 'turn.end' },
        { type: 'done', aborted: false },
      );
    };
    const response = await chat(conversation.id, 'Liste die Dateien');
    expect(response.status).toBe(200);
    const body = await response.text();
    expect(body).toContain('"type":"tool-input-available"');
    expect(body).toContain('"type":"data-approval"');
    expect(body).toContain('"delta":"Fertig."');

    const prompt = device.received.find((message) => message.type === 'session.prompt');
    expect(prompt).toMatchObject({
      session: conversation.id,
      project: 'demo',
      harness: 'pi',
      text: 'Liste die Dateien',
      history: null,
      model: { credentialId: credential.id, modelId: 'mock-1' },
    });

    await eventually(async () => (await load(conversation.id)).messages.length === 2);
    const reply = (await load(conversation.id)).messages[1];
    expect(reply?.metadata).toMatchObject({ agent: { project: 'demo', harness: 'pi' }, usage: { inputTokens: 3, outputTokens: 4 } });
    expect(reply?.parts.map((part) => part.type)).toEqual(['step-start', 'dynamic-tool', 'data-approval', 'text']);

    // The next prompt carries the history for agents that join later.
    device.onPrompt = (next) => device.emit(next.session, { type: 'done', aborted: false });
    await (await chat(conversation.id, 'Und jetzt?')).text();
    const second = device.received.filter((message) => message.type === 'session.prompt').at(-1);
    expect(second).toMatchObject({ history: 'User: Liste die Dateien\n\nAssistant: Fertig.' });

    const answer = await server.request('POST', `/api/conversations/${conversation.id}/approvals/7`, {
      cookie,
      body: { decision: 'session' },
    });
    expect(answer.status).toBe(204);
    expect(device.received.at(-1)).toEqual({ type: 'approval.decide', session: conversation.id, id: 7, decision: 'session' });
  });

  it('keeps reported projects while the device is offline and links to them', async () => {
    const { token, deviceId } = await pair();
    // What `harness init` does without a running service.
    const reported = await server.request('PUT', '/api/device/projects', {
      headers: { ...CLI, authorization: `Bearer ${token}` },
      body: { projects: [{ name: 'neu', harnesses: ['pi'] }] },
    });
    expect(reported.status).toBe(204);
    const invalid = await server.request('PUT', '/api/device/projects', {
      headers: { ...CLI, authorization: `Bearer ${token}` },
      body: { projects: [{ name: '../etc', harnesses: ['pi'] }] },
    });
    expect(invalid.status).toBe(400);

    const devices = (await (await server.request('GET', '/api/devices', { cookie })).json()) as DeviceSummary[];
    expect(devices[0]).toMatchObject({ online: false, vm: null, projects: [{ name: 'neu', harnesses: ['pi'] }] });

    const conversation = await newConversation();
    const linked = await server.request('PUT', `/api/conversations/${conversation.id}/link`, {
      cookie,
      body: { deviceId, project: 'neu', harness: 'pi' },
    });
    expect(linked.status).toBe(200);
    const offline = await chat(conversation.id, 'Hallo?');
    expect(offline.status).toBe(409);
  });

  it('refuses to chat while the device is offline and stores nothing', async () => {
    const { deviceId, userId } = await pair();
    const device = connect(server.hub, deviceId, userId);
    device.hello();
    const conversation = await newConversation();
    await server.request('PUT', `/api/conversations/${conversation.id}/link`, {
      cookie,
      body: { deviceId, project: 'demo', harness: 'pi' },
    });
    device.connection?.closed();

    const response = await chat(conversation.id, 'Hallo?');
    expect(response.status).toBe(409);
    expect(((await response.json()) as { error: { code: string } }).error.code).toBe('device_offline');
    expect((await load(conversation.id)).messages).toHaveLength(0);
  });

  it('accepts tunnels only with a valid device token', async () => {
    const { token, deviceId } = await pair();
    const http = serve({ fetch: server.app.fetch, hostname: '127.0.0.1', port: 0 });
    await new Promise<void>((resolve) => http.once('listening', () => resolve()));
    attachTunnel(http, server.database.db, server.hub);
    const { port } = http.address() as AddressInfo;
    const url = `ws://127.0.0.1:${port}${TUNNEL_PATH}`;

    try {
      const status = await new Promise<number>((resolve, reject) => {
        const ws = new WebSocket(url, { headers: { authorization: 'Bearer hd_wrong' } });
        ws.on('unexpected-response', (_request, response) => resolve(response.statusCode ?? 0));
        ws.on('open', () => reject(new Error('opened without a valid token')));
      });
      expect(status).toBe(401);

      const ws = new WebSocket(url, { headers: { authorization: `Bearer ${token}` } });
      const welcome = await new Promise<unknown>((resolve, reject) => {
        ws.once('message', (data) => resolve(JSON.parse(data.toString())));
        ws.once('error', reject);
      });
      expect(welcome).toEqual({ type: 'welcome', version: TUNNEL_VERSION });
      ws.send(JSON.stringify({ type: 'hello', version: TUNNEL_VERSION, projects: [PROJECT] }));
      await eventually(async () => server.hub.status(deviceId).online);

      // Revoking the device in the web app cuts the tunnel at once.
      const cookieDevices = (await (await server.request('GET', '/api/devices', { cookie })).json()) as DeviceSummary[];
      expect(cookieDevices[0]?.online).toBe(true);
      const closed = new Promise<number>((resolve) => ws.once('close', (code) => resolve(code)));
      await server.request('DELETE', `/api/devices/${deviceId}`, { cookie });
      expect(await closed).toBe(4003);
      expect(server.hub.status(deviceId).online).toBe(false);
    } finally {
      await new Promise((resolve) => http.close(resolve));
    }
  });
});
