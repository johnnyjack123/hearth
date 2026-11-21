import {
  type ConversationDetail,
  type ConversationSummary,
  HOST_HARNESS,
  type PairingPoll,
  type PairingStart,
  type ProviderCredential,
  type ServerMessage,
  TUNNEL_VERSION,
  VERSION_APP_VERSION,
} from '@hearth/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
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

  beforeEach(async () => {
    llm = createScriptedLlm(() => script);
    server = await createTestServer(kind, { llm });
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
