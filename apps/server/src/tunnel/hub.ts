import {
  type AgentEvent,
  type ApprovalDecision,
  type DeviceMessage,
  deviceMessageSchema,
  type RemoteProject,
  type ServerMessage,
  TUNNEL_VERSION,
} from '@harness/shared';
import { HttpError } from '../http/errors.ts';

/**
 * The part of a WebSocket the hub needs; `ws` sockets satisfy it, and tests
 * can pass a fake.
 */
export interface TunnelSocket {
  /** Sends one text frame. */
  send(data: string): void;
  /** Closes the connection. */
  close(code?: number, reason?: string): void;
}

/** Live state of a device, as the web app shows it. */
export interface DeviceStatus {
  /** Connected and greeted. */
  online: boolean;
  /** Projects offered to web chats; empty while offline. */
  projects: RemoteProject[];
}

/** Callbacks for one socket, returned by {@link DeviceHub.attach}. */
export interface TunnelConnection {
  /** Handles a frame from the device. */
  receive(raw: string): void;
  /** The socket closed. */
  closed(): void;
}

/**
 * Why the server ended a run itself. Sent as the error text of the reply;
 * the web app shows a translated message for each.
 */
export type RunFailure = 'device_replaced' | 'device_lost' | 'device_protocol' | 'chat_unlinked' | 'agent_ended';

/** Prompt for a session, as {@link DeviceHub.startRun} sends it. */
export type PromptMessage = Extract<ServerMessage, { type: 'session.prompt' }>;

// Close codes in the private range 4000-4999.
const CLOSE_REPLACED = 4000;
const CLOSE_VERSION = 4001;
const CLOSE_PROTOCOL = 4002;
const CLOSE_REVOKED = 4003;

/**
 * One reply of an agent: the events of the device until `done`, `error` or
 * the end of the session, consumed with `for await`.
 */
export class AgentRun implements AsyncIterable<AgentEvent> {
  readonly #queue: AgentEvent[] = [];
  #wake: (() => void) | null = null;
  #ended = false;

  /**
   * @param cancel - Asks the device to interrupt the agent.
   */
  constructor(readonly cancel: () => void) {}

  /** Whether no further events will come. */
  get ended(): boolean {
    return this.#ended;
  }

  /**
   * Queues an event; `done` and `error` end the run.
   *
   * @param event - Event from the device.
   */
  push(event: AgentEvent): void {
    if (this.#ended) {
      return;
    }
    this.#queue.push(event);
    if (event.type === 'done' || event.type === 'error') {
      this.#ended = true;
    }
    this.#notify();
  }

  /**
   * Ends the run without a reply from the device, e.g. when it disconnects.
   *
   * @param reason - Why; the web app translates it.
   */
  fail(reason: RunFailure): void {
    this.push({ type: 'error', message: reason });
  }

  #notify(): void {
    const wake = this.#wake;
    this.#wake = null;
    wake?.();
  }

  async *[Symbol.asyncIterator](): AsyncIterator<AgentEvent> {
    for (;;) {
      const event = this.#queue.shift();
      if (event) {
        yield event;
        if (event.type === 'done' || event.type === 'error') {
          return;
        }
        continue;
      }
      await new Promise<void>((resolve) => {
        this.#wake = resolve;
      });
    }
  }
}

interface Connection {
  socket: TunnelSocket;
  userId: string;
  greeted: boolean;
  projects: RemoteProject[];
}

interface ActiveRun {
  deviceId: string;
  run: AgentRun;
}

/**
 * Connected devices (`harness connect`) and the agent replies running on
 * them. Lives in memory: after a server restart devices reconnect and the
 * next message starts a new reply.
 */
export class DeviceHub {
  readonly #devices = new Map<string, Connection>();
  readonly #runs = new Map<string, ActiveRun>();

  /**
   * Takes over a device's socket. A newer connection of the same device
   * replaces the older one.
   *
   * @param deviceId - Authenticated device.
   * @param userId - Its owner.
   * @param socket - The open socket.
   * @returns Handlers for the socket's frames and its end.
   */
  attach(deviceId: string, userId: string, socket: TunnelSocket): TunnelConnection {
    const previous = this.#devices.get(deviceId);
    if (previous) {
      this.#drop(deviceId, previous, 'device_replaced');
      previous.socket.close(CLOSE_REPLACED, 'replaced');
    }
    const connection: Connection = { socket, userId, greeted: false, projects: [] };
    this.#devices.set(deviceId, connection);
    this.#send(connection, { type: 'welcome', version: TUNNEL_VERSION });

    return {
      receive: (raw) => this.#receive(deviceId, connection, raw),
      closed: () => this.#drop(deviceId, connection, 'device_lost'),
    };
  }

  /**
   * Disconnects a device, e.g. after its token was revoked.
   *
   * @param deviceId - Device id.
   */
  disconnect(deviceId: string): void {
    const connection = this.#devices.get(deviceId);
    if (connection) {
      this.#drop(deviceId, connection, 'device_lost');
      connection.socket.close(CLOSE_REVOKED, 'revoked');
    }
  }

  /**
   * @param deviceId - Device id.
   * @returns Whether it is connected and which projects it offers.
   */
  status(deviceId: string): DeviceStatus {
    const connection = this.#devices.get(deviceId);
    return connection?.greeted ? { online: true, projects: connection.projects } : { online: false, projects: [] };
  }

  /**
   * Sends a prompt to a device and returns the reply's events.
   *
   * @param deviceId - Device the session runs on.
   * @param prompt - The prompt.
   * @returns The run.
   * @throws {HttpError} 409 if the device is offline, does not offer the
   *   project, or the session is still answering.
   */
  startRun(deviceId: string, prompt: PromptMessage): AgentRun {
    const connection = this.#devices.get(deviceId);
    if (!connection?.greeted) {
      throw new HttpError(409, 'device_offline', 'The device is not connected');
    }
    const offered = connection.projects.find((project) => project.name === prompt.project);
    if (!offered?.harnesses.includes(prompt.harness)) {
      throw new HttpError(409, 'device_offline', 'The device does not offer this project or harness');
    }
    if (this.#runs.has(prompt.session)) {
      throw new HttpError(409, 'conflict', 'The agent is still answering');
    }
    const run = new AgentRun(() => this.#send(connection, { type: 'session.cancel', session: prompt.session }));
    this.#runs.set(prompt.session, { deviceId, run });
    this.#send(connection, prompt);
    return run;
  }

  /**
   * Ends a session on its device, e.g. when the chat is unlinked.
   *
   * @param deviceId - Device the session runs on.
   * @param session - Session id.
   */
  closeSession(deviceId: string, session: string): void {
    this.#runs.get(session)?.run.fail('chat_unlinked');
    this.#runs.delete(session);
    const connection = this.#devices.get(deviceId);
    if (connection?.greeted) {
      this.#send(connection, { type: 'session.close', session });
    }
  }

  /**
   * Passes the user's answer to a gateway question on to the device.
   *
   * @param deviceId - Device the session runs on.
   * @param session - Session whose gateway asked.
   * @param id - Question id.
   * @param decision - The answer.
   * @throws {HttpError} 409 if the device is offline.
   */
  decide(deviceId: string, session: string, id: number, decision: ApprovalDecision): void {
    const connection = this.#devices.get(deviceId);
    if (!connection?.greeted) {
      throw new HttpError(409, 'device_offline', 'The device is not connected');
    }
    this.#send(connection, { type: 'approval.decide', session, id, decision });
  }

  #receive(deviceId: string, connection: Connection, raw: string): void {
    if (this.#devices.get(deviceId) !== connection) {
      return;
    }
    let message: DeviceMessage;
    try {
      message = deviceMessageSchema.parse(JSON.parse(raw));
    } catch {
      // A device that sends nonsense is buggy or not ours; better to drop it
      // than to guess.
      connection.socket.close(CLOSE_PROTOCOL, 'invalid message');
      this.#drop(deviceId, connection, 'device_protocol');
      return;
    }
    if (message.type !== 'hello' && !connection.greeted) {
      connection.socket.close(CLOSE_PROTOCOL, 'hello expected');
      this.#drop(deviceId, connection, 'device_protocol');
      return;
    }
    switch (message.type) {
      case 'hello':
        if (message.version !== TUNNEL_VERSION) {
          connection.socket.close(CLOSE_VERSION, `protocol ${TUNNEL_VERSION} required`);
          this.#drop(deviceId, connection, 'device_protocol');
          return;
        }
        connection.greeted = true;
        connection.projects = message.projects;
        return;
      case 'projects':
        connection.projects = message.projects;
        return;
      case 'session.event': {
        // A device only reports on sessions the server opened with it.
        const active = this.#runs.get(message.session);
        if (active?.deviceId !== deviceId) {
          return;
        }
        active.run.push(message.event);
        if (active.run.ended) {
          this.#runs.delete(message.session);
        }
        return;
      }
      case 'session.closed': {
        const active = this.#runs.get(message.session);
        if (active?.deviceId === deviceId) {
          if (message.reason) {
            active.run.push({ type: 'error', message: message.reason });
          } else {
            active.run.fail('agent_ended');
          }
          this.#runs.delete(message.session);
        }
        return;
      }
    }
  }

  #drop(deviceId: string, connection: Connection, reason: RunFailure): void {
    // The late close of a replaced socket must not end the new one's runs.
    if (this.#devices.get(deviceId) !== connection) {
      return;
    }
    this.#devices.delete(deviceId);
    for (const [session, active] of this.#runs) {
      if (active.deviceId === deviceId) {
        active.run.fail(reason);
        this.#runs.delete(session);
      }
    }
  }

  #send(connection: Connection, message: ServerMessage): void {
    try {
      connection.socket.send(JSON.stringify(message));
    } catch (error) {
      console.warn('[tunnel] send failed:', error instanceof Error ? error.message : error);
    }
  }
}
