import http from 'node:http';
import type { AddressInfo } from 'node:net';
import {
  type ConversationDetail,
  type ConversationSummary,
  HOST_HARNESS,
  type HostGrants,
  type PairingPoll,
  type PairingStart,
  type ProviderCredential,
  type ServerMessage,
  TUNNEL_VERSION,
  VERSION_APP_VERSION,
  VERSION_HOST,
} from '@hearth/shared';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import type { ChatMessage } from '../src/services/conversations.ts';
import type { DeviceHub, TunnelConnection, TunnelSocket } from '../src/tunnel/hub.ts';
import { createScriptedLlm, createTestServer, DIALECTS, eventually, type Step, type TestServer } from './helpers.ts';

const CLI = { origin: '' };
const HOST_PROJECT = {
  name: 'demo',
  harnesses: [HOST_HARNESS],
  host: { root: '/home/erika/demo', os: 'linux' as const, shell: 'bash', browserApprovals: false },
};

type HostCall = Extract<ServerMessage, { type: 'host.call' }>;

/** A device in the direct mode, scripted by the test. */
class HostDevice implements TunnelSocket {
  readonly received: ServerMessage[] = [];
  connection: TunnelConnection | undefined;
  onCall: ((call: HostCall) => void) | undefined;

  send(data: string): void {
    const message = JSON.parse(data) as ServerMessage;
    this.received.push(message);
    if (message.type === 'host.call') {
      setTimeout(() => this.onCall?.(message), 0);
    }
  }

  close(): void {}

  say(message: unknown): void {
    this.connection?.receive(JSON.stringify(message));
  }

  result(call: HostCall, output: string, extra: { isError?: boolean; exitCode?: number } = {}): void {
    this.say({ type: 'host.result', session: call.session, call: call.call, output, isError: extra.isError ?? false, ...extra });
  }

  approval(call: HostCall, approval: string, state: 'pending' | 'resolved', extra: Record<string, unknown> = {}): void {
    this.say({
      type: 'host.approval',
      session: call.session,
      call: call.call,
      approval,
      state,
      request: { kind: 'command', command: String(call.input.command ?? ''), shell: 'bash', cwd: '/home/erika/demo' },
      summary: `Befehl ausführen: ${String(call.input.command ?? '')}`,
      why: 'Befehle laufen mit allen deinen Rechten.',
      chatWide: true,
      browser: false,
      ...extra,
    });
  }
}

function connect(hub: DeviceHub, deviceId: string, userId: string, version = TUNNEL_VERSION): HostDevice {
  const device = new HostDevice();
  device.connection = hub.attach(deviceId, userId, device);
  device.say({ type: 'hello', version, projects: version >= 11 ? [HOST_PROJECT] : [] });
  return device;
}

describe.each(DIALECTS)('the direct mode on %s', (kind) => {
  let server: TestServer;
  let cookie: string;
  let credential: ProviderCredential;
  let script: Step[] = [];
  let llm: ReturnType<typeof createScriptedLlm>;
  // Stand-in for Jev at OpenRouter: answers with the level set by the test
  // (0 is the worst level of the scale) and keeps what it was asked.
  let jev: http.Server;
  let jevLevel = 0;
  const jevSeen: { state: Record<string, unknown> }[] = [];

  beforeAll(async () => {
    jev = http.createServer((request, response) => {
      let body = '';
      request.on('data', (chunk: Buffer) => {
        body += chunk.toString();
      });
      request.on('end', () => {
        jevSeen.push(JSON.parse(body) as { state: Record<string, unknown> });
        response.writeHead(200, { 'content-type': 'application/json' });
        response.end(JSON.stringify({ answers: { item_1: { type: 'score', score: jevLevel } }, usage: { input_tokens: 300, output_tokens: 0, cost: 0.00001 } }));
      });
    });
    await new Promise<void>((resolve) => jev.listen(0, '127.0.0.1', resolve));
  });

  afterAll(async () => {
    await new Promise((resolve) => jev.close(resolve));
  });

  beforeEach(async () => {
    llm = createScriptedLlm(() => script);
    jevSeen.length = 0;
    server = await createTestServer(kind, { llm, helpers: { apiUrl: `http://127.0.0.1:${(jev.address() as AddressInfo).port}/api/v1`, timeoutMs: 2_000 } });
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

  async function pair(): Promise<{ deviceId: string; userId: string }> {
    const start = (await (
      await server.request('POST', '/api/pairing', { body: { name: 'hearth auf TEST' }, headers: CLI })
    ).json()) as PairingStart;
    await server.request('POST', `/api/devices/pairing/${start.userCode}`, { cookie, body: { approve: true } });
    const poll = (await (
      await server.request('POST', '/api/pairing/token', { body: { deviceCode: start.deviceCode }, headers: CLI })
    ).json()) as PairingPoll;
    if (poll.status !== 'approved') {
      throw new Error('not approved');
    }
    const row = await server.database.db.selectFrom('device').select('userId').where('id', '=', poll.deviceId).executeTakeFirstOrThrow();
    return { deviceId: poll.deviceId, userId: row.userId };
  }

  async function linkedChat(deviceId: string): Promise<string> {
    const conversation = (await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary;
    const linked = await server.request('PUT', `/api/conversations/${conversation.id}/link`, {
      cookie,
      body: { deviceId, project: 'demo', harness: HOST_HARNESS },
    });
    expect(linked.status).toBe(200);
    return conversation.id;
  }

  function chat(conversationId: string, text: string) {
    return server.request('POST', `/api/conversations/${conversationId}/chat`, {
      cookie,
      body: {
        message: { id: `m-${Math.random().toString(36).slice(2)}`, role: 'user', parts: [{ type: 'text', text }] },
        credentialId: credential.id,
        modelId: 'mock-1',
      },
    });
  }

  async function load(conversationId: string): Promise<ConversationDetail<ChatMessage>> {
    return (await (await server.request('GET', `/api/conversations/${conversationId}`, { cookie })).json()) as ConversationDetail<ChatMessage>;
  }

  it('lets the chat model work through the device, with its questions in the chat', async () => {
    const { deviceId, userId } = await pair();
    const device = connect(server.hub, deviceId, userId);
    const chatId = await linkedChat(deviceId);
    script = [{ tool: 'read', input: { path: 'README.md' } }, { tool: 'shell', input: { command: 'cargo test' } }, { text: 'Alles grün.' }];
    device.onCall = (call) => {
      if (call.tool === 'read') {
        device.result(call, '# Demo\n');
        return;
      }
      // A command asks first; the answer comes from the computer.
      device.approval(call, 'ABC234', 'pending');
      setTimeout(() => {
        device.approval(call, 'ABC234', 'resolved', { decision: 'once', by: 'computer' });
        device.result(call, 'test result: ok\n', { exitCode: 0 });
      }, 10);
    };

    const response = await chat(chatId, 'Lies die README und teste');
    expect(response.status).toBe(200);
    const body = await response.text();
    expect(body).toContain('"type":"data-hostApproval"');
    expect(body).toContain('Alles grün.');

    const calls = device.received.filter((message): message is HostCall => message.type === 'host.call');
    expect(calls.map((call) => [call.tool, call.input])).toEqual([
      ['read', { path: 'README.md' }],
      ['shell', { command: 'cargo test' }],
    ]);
    expect(calls[0]).toMatchObject({ session: chatId, project: 'demo', conversation: { id: chatId } });

    // The model knew where it works, and read the device's results.
    const first = llm.calls[0];
    expect(first?.system).toContain('in the project folder /home/erika/demo');
    expect(first?.system).toContain('with bash');
    expect(first?.tools).toEqual(expect.arrayContaining(['read', 'write', 'edit', 'list', 'find', 'grep', 'shell']));
    expect(JSON.stringify(llm.calls[1]?.prompt)).toContain('# Demo');

    await eventually(async () => (await load(chatId)).messages.length === 2);
    const reply = (await load(chatId)).messages[1];
    expect(reply?.metadata).toMatchObject({ agent: { project: 'demo', harness: HOST_HARNESS } });
    const parts = reply?.parts ?? [];
    expect(parts.some((part) => part.type === 'tool-shell' && 'output' in part && (part.output as { exitCode?: number }).exitCode === 0)).toBe(true);
    const card = parts.find((part) => part.type === 'data-hostApproval');
    expect(card && 'data' in card ? card.data : null).toMatchObject({ approval: 'ABC234', state: 'resolved', decision: 'once', by: 'computer' });

    // The next message reads the earlier calls as real tool calls again.
    script = [{ text: 'Ok.' }];
    await (await chat(chatId, 'Danke')).text();
    expect(JSON.stringify(llm.calls.at(-1)?.prompt)).toContain('test result: ok');
  });

  it('passes the chat’s answer on and stops waiting calls with the reply', async () => {
    const { deviceId, userId } = await pair();
    const device = connect(server.hub, deviceId, userId);
    const chatId = await linkedChat(deviceId);
    script = [{ tool: 'shell', input: { command: 'rm -rf build' } }];
    device.onCall = (call) => device.approval(call, 'QRS789', 'pending');

    const response = await chat(chatId, 'Räum auf');
    const reading = response.text();
    await eventually(async () => device.received.some((message) => message.type === 'host.call'));
    const answered = await server.request('POST', `/api/conversations/${chatId}/host-approvals/QRS789`, { cookie, body: { decision: 'chat' } });
    expect(answered.status).toBe(204);
    expect(device.received.at(-1)).toEqual({ type: 'host.decide', session: chatId, approval: 'QRS789', decision: 'chat' });
    expect((await server.request('POST', `/api/conversations/${chatId}/host-approvals/nope`, { cookie, body: { decision: 'chat' } })).status).toBe(404);
    expect((await server.request('POST', `/api/conversations/${chatId}/host-approvals/QRS789`, { cookie, body: { decision: 'always' } })).status).toBe(400);

    // Stop cancels the call on the device.
    expect((await server.request('POST', `/api/conversations/${chatId}/stop`, { cookie })).status).toBeLessThan(300);
    await reading;
    await eventually(async () => device.received.some((message) => message.type === 'host.cancel'));
  });

  it('has Jev rate each command before the device asks, where the device lets Jev check', async () => {
    // Helpers need an OpenRouter key of the user; the chat itself runs elsewhere.
    await server.request('POST', '/api/providers', {
      cookie,
      body: { kind: 'openrouter', label: 'OpenRouter', apiKey: 'sk-or-test-0123456789abcdef' },
    });
    const { deviceId, userId } = await pair();
    const device = connect(server.hub, deviceId, userId);
    const chatId = await linkedChat(deviceId);
    jevLevel = 0;
    script = [{ tool: 'read', input: { path: 'a.txt' } }, { tool: 'shell', input: { command: 'rm -rf ~' } }, { text: 'Erledigt.' }];
    device.onCall = (call) => {
      if (call.tool === 'read') {
        device.result(call, 'a\n');
        return;
      }
      device.approval(call, 'ABC234', 'pending');
      setTimeout(() => {
        device.approval(call, 'ABC234', 'resolved', { decision: 'deny', by: 'computer' });
        device.result(call, 'The user denied this.', { isError: true });
      }, 10);
    };
    const body = await (await chat(chatId, 'Räum mein Home auf')).text();

    // Only the command was rated, and the device got the rating with it.
    const calls = device.received.filter((message): message is HostCall => message.type === 'host.call');
    expect(calls[0]?.assessment).toBeUndefined();
    expect(calls[1]?.assessment).toEqual({ danger: 1, model: 'typesafe/jev-1.13' });
    expect(jevSeen).toHaveLength(1);
    expect(jevSeen[0]?.state).toMatchObject({ command: 'rm -rf ~', shell: 'bash', folder: '/home/erika/demo', request: 'Räum mein Home auf' });
    // The question's card carries it as well.
    expect(body).toMatch(/"type":"data-hostApproval".*"assessment":\{"model":"typesafe\/jev-1\.13","danger":1/);

    await eventually(async () => (await load(chatId)).messages.length === 2);
    const reply = (await load(chatId)).messages[1];
    const shell = reply?.parts.find((part) => part.type === 'tool-shell');
    expect(shell && 'output' in shell ? (shell.output as { commandAssessment?: unknown }).commandAssessment : null).toMatchObject({ danger: 1 });
    const helpers = (reply?.metadata as { usage?: { helpers?: { purpose?: string }[] } }).usage?.helpers ?? [];
    expect(helpers.some((call) => call.purpose === 'command')).toBe(true);

    // Switched off for the device: no rating, no call to Jev.
    await server.request('PATCH', `/api/devices/${deviceId}`, { cookie, body: { jevReview: false } });
    script = [{ tool: 'shell', input: { command: 'ls' } }, { text: 'Ok.' }];
    device.onCall = (call) => device.result(call, 'a.txt\n', { exitCode: 0 });
    await (await chat(chatId, 'Was liegt da?')).text();
    const last = device.received.filter((message): message is HostCall => message.type === 'host.call').at(-1);
    expect(last?.input).toEqual({ command: 'ls' });
    expect(last?.assessment).toBeUndefined();
    expect(jevSeen).toHaveLength(1);
  });

  it('keeps the rating from devices that would take it for the tool’s input', async () => {
    await server.request('POST', '/api/providers', {
      cookie,
      body: { kind: 'openrouter', label: 'OpenRouter', apiKey: 'sk-or-test-0123456789abcdef' },
    });
    const { deviceId, userId } = await pair();
    const device = connect(server.hub, deviceId, userId, VERSION_HOST);
    const chatId = await linkedChat(deviceId);
    script = [{ tool: 'shell', input: { command: 'cargo test' } }, { text: 'Ok.' }];
    device.onCall = (call) => device.result(call, 'ok\n', { exitCode: 0 });
    await (await chat(chatId, 'Teste')).text();
    const call = device.received.find((message): message is HostCall => message.type === 'host.call');
    expect(call).toBeDefined();
    expect(call && 'assessment' in call).toBe(false);
  });

  it('shows what the user allowed for the chat and takes it back on request', async () => {
    const { deviceId, userId } = await pair();
    const device = connect(server.hub, deviceId, userId);
    const chatId = await linkedChat(deviceId);
    const grants = async () => (await (await server.request('GET', `/api/conversations/${chatId}/host-grants`, { cookie })).json()) as HostGrants;
    expect(await grants()).toEqual({ online: true, supported: true, grants: [] });

    device.say({ type: 'host.grants', session: chatId, grants: [{ id: 'ABC234', kind: 'command', target: 'cargo test' }] });
    expect((await grants()).grants).toEqual([{ id: 'ABC234', kind: 'command', target: 'cargo test' }]);
    // Another device cannot write over this chat's answers.
    const other = await pair();
    const intruder = connect(server.hub, other.deviceId, other.userId);
    intruder.say({ type: 'host.grants', session: chatId, grants: [] });
    expect((await grants()).grants).toHaveLength(1);

    const revoked = await server.request('DELETE', `/api/conversations/${chatId}/host-grants/ABC234`, { cookie });
    expect(revoked.status).toBe(204);
    expect(device.received.at(-1)).toEqual({ type: 'host.revoke', session: chatId, grant: 'ABC234' });
    expect((await server.request('DELETE', `/api/conversations/${chatId}/host-grants/nope`, { cookie })).status).toBe(404);
    device.say({ type: 'host.grants', session: chatId, grants: [] });
    expect((await grants()).grants).toEqual([]);

    // Gone with the connection: they live in the device's service.
    device.say({ type: 'host.grants', session: chatId, grants: [{ id: 'DEF567', kind: 'folder', target: '/tmp' }] });
    device.connection?.closed();
    expect(await grants()).toEqual({ online: false, supported: false, grants: [] });
    expect((await server.request('DELETE', `/api/conversations/${chatId}/host-grants/DEF567`, { cookie })).status).toBe(409);
  });

  it('takes the project’s write setting from the device', async () => {
    const { deviceId, userId } = await pair();
    const device = new HostDevice();
    device.connection = server.hub.attach(deviceId, userId, device);
    device.say({ type: 'hello', version: TUNNEL_VERSION, projects: [{ ...HOST_PROJECT, host: { ...HOST_PROJECT.host, writes: 'ask' } }] });
    expect(server.hub.status(deviceId).projects[0]?.host?.writes).toBe('ask');
  });

  it('refuses the direct mode with devices that do not know it', async () => {
    const { deviceId, userId } = await pair();
    const device = connect(server.hub, deviceId, userId, VERSION_APP_VERSION);
    expect(device.received[0]).toMatchObject({ type: 'welcome' });
    const conversation = (await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary;
    const linked = await server.request('PUT', `/api/conversations/${conversation.id}/link`, {
      cookie,
      body: { deviceId, project: 'demo', harness: HOST_HARNESS },
    });
    expect(linked.status).toBe(409);
  });
});
