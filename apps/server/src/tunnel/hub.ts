import { randomUUID } from 'node:crypto';
import {
  type AgentEvent,
  type AppFileResult,
  type ApprovalDecision,
  type AppsReport,
  type AppsSetResult,
  type DeviceActivity,
  type DeviceAppTemplate,
  type DeviceMessage,
  deviceMessageSchema,
  DEFAULT_IDLE_MINUTES,
  MIN_TUNNEL_VERSION,
  type RemoteProject,
  type ServerMessage,
  TUNNEL_VERSION,
  VERSION_APP_FILES,
  VERSION_APPS,
  VERSION_FILES,
  VERSION_PI_SETTINGS,
  VERSION_TAKEOVER,
  VERSION_TEMPLATES,
  VERSION_VM_CONTROL,
  type VmState,
  type VmStopResult,
} from '@harness/shared';
import { HttpError } from '../http/errors.ts';
import { AgentBills } from '../openrouter.ts';

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
  /** Projects offered right now; empty while offline. */
  projects: RemoteProject[];
  /** The device's VM while online. */
  vm: VmState | null;
  /** Use of the VM while online, from devices with protocol 2. */
  activity: DeviceActivity | null;
  /** Its apps while online, from devices with protocol 5. */
  apps: AppsReport | null;
}

/** Settings the server keeps for a device and hands it on connect. */
export interface DeviceSettings {
  /** Idle minutes until the device stops its VM; 0: never. */
  idleMinutes: number;
}

/** How long a device may take to stop its VM (podman, then a hard stop). */
const VM_STOP_TIMEOUT_MS = 3 * 60_000;

/** Switching apps only edits a file on the device. */
const APPS_SET_TIMEOUT_MS = 30_000;

/** Asking for a program file reads its first bytes and writes a request. */
const APP_FILE_TIMEOUT_MS = 30_000;

/** Hooks of the hub into the rest of the server. */
export interface HubListener {
  /** A device reported its projects; they are kept for while it is offline. */
  projects?(deviceId: string, projects: RemoteProject[]): void | Promise<void>;
  /** A device reported its apps; kept like its projects. */
  apps?(deviceId: string, report: AppsReport): void | Promise<void>;
  /** The own programs of an account, for its devices after their hello. */
  templates?(userId: string): DeviceAppTemplate[] | Promise<DeviceAppTemplate[]>;
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
export type RunFailure = 'device_replaced' | 'device_lost' | 'device_protocol' | 'chat_unlinked' | 'agent_ended' | 'timeout';

/** Prompt for a session, as {@link DeviceHub.startRun} sends it. */
export type PromptMessage = Extract<ServerMessage, { type: 'session.prompt' }>;

/** Request to take over a session's work, as {@link DeviceHub.startTakeover} sends it. */
export type TakeoverMessage = Extract<ServerMessage, { type: 'session.takeover' }>;

/** Request to summarize an agent's context, as {@link DeviceHub.startCompaction} sends it. */
export type CompactMessage = Extract<ServerMessage, { type: 'session.compact' }>;

/**
 * A prompt as a device of `version` understands it: protocol 1 and 2 know
 * no autosync (the field would only confuse a reader of their logs), and
 * before protocol 4 there is no thinking level.
 */
function forVersion(message: PromptMessage, version: number): PromptMessage {
  const { thinking, files, ...rest } = message;
  return {
    ...rest,
    ...(version < VERSION_TAKEOVER ? { autosync: false } : {}),
    ...(thinking && version >= VERSION_PI_SETTINGS ? { thinking } : {}),
    ...(files?.length && version >= VERSION_FILES ? { files } : {}),
  };
}

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

/** A request to the device that waits for its answer (`vm.result`, `apps.result`). */
interface PendingRequest {
  resolve: (result: VmStopResult | AppsSetResult | AppFileResult) => void;
  reject: (error: HttpError) => void;
  timer: ReturnType<typeof setTimeout>;
}

interface Connection {
  socket: TunnelSocket;
  userId: string;
  greeted: boolean;
  /** Protocol the device speaks, known after its hello. */
  version: number;
  projects: RemoteProject[];
  vm: VmState;
  activity: DeviceActivity | null;
  apps: AppsReport | null;
  settings: DeviceSettings;
  requests: Map<string, PendingRequest>;
  /** Counts lists of own programs sent, so a slow first one cannot follow a newer. */
  templatesSent: number;
}

interface ActiveRun {
  deviceId: string;
  run: AgentRun;
  /** The user's question while the reply runs in research mode. */
  research?: { question: string };
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
   * What OpenRouter billed for the agents' model calls through the
   * pass-through, until the agent's report of each call picks it up.
   */
  readonly bills = new AgentBills();

  /**
   * @param listener - Optional hooks, e.g. to store reported projects.
   */
  constructor(private readonly listener: HubListener = {}) {}

  /**
   * Takes over a device's socket. A newer connection of the same device
   * replaces the older one.
   *
   * @param deviceId - Authenticated device.
   * @param userId - Its owner.
   * @param socket - The open socket.
   * @param settings - The device's settings, sent once it has greeted.
   * @returns Handlers for the socket's frames and its end.
   */
  attach(deviceId: string, userId: string, socket: TunnelSocket, settings?: DeviceSettings): TunnelConnection {
    const previous = this.#devices.get(deviceId);
    if (previous) {
      this.#drop(deviceId, previous, 'device_replaced');
      previous.socket.close(CLOSE_REPLACED, 'replaced');
    }
    const connection: Connection = {
      socket,
      userId,
      greeted: false,
      version: 0,
      projects: [],
      vm: 'unknown',
      activity: null,
      apps: null,
      settings: settings ?? previous?.settings ?? { idleMinutes: DEFAULT_IDLE_MINUTES },
      requests: new Map(),
      templatesSent: 0,
    };
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
    return connection?.greeted
      ? { online: true, projects: connection.projects, vm: connection.vm, activity: connection.activity, apps: connection.apps }
      : { online: false, projects: [], vm: null, activity: null, apps: null };
  }

  /**
   * Hands changed settings to a connected device; a device that connects
   * later gets them with its greeting from the database.
   *
   * @param deviceId - Device id.
   * @param settings - The new settings.
   */
  configure(deviceId: string, settings: DeviceSettings): void {
    const connection = this.#devices.get(deviceId);
    if (!connection) {
      return;
    }
    connection.settings = settings;
    if (connection.greeted && connection.version >= VERSION_VM_CONTROL) {
      this.#send(connection, { type: 'settings', idleMinutes: settings.idleMinutes });
    }
  }

  /**
   * Hands the account's own programs to its connected devices (protocol 6);
   * a device that connects later gets them after its hello.
   *
   * @param userId - The account.
   * @param templates - All of its programs.
   */
  sendTemplates(userId: string, templates: DeviceAppTemplate[]): void {
    for (const connection of this.#devices.values()) {
      if (connection.userId === userId && connection.greeted && connection.version >= VERSION_TEMPLATES) {
        connection.templatesSent += 1;
        this.#send(connection, { type: 'apps.templates', templates });
      }
    }
  }

  /**
   * Asks a device to stop its VM and waits for its answer. Whatever runs
   * there ends; the next message of a chat starts the VM again.
   *
   * @param deviceId - Device id.
   * @returns The device's answer.
   * @throws {HttpError} 409 if the device is offline or too old for it,
   *   504 if it does not answer in time.
   */
  stopVm(deviceId: string): Promise<VmStopResult> {
    const connection = this.#devices.get(deviceId);
    if (!connection?.greeted) {
      return Promise.reject(new HttpError(409, 'device_offline', 'The device is not connected'));
    }
    if (connection.version < VERSION_VM_CONTROL) {
      return Promise.reject(new HttpError(409, 'device_outdated', 'The device cannot stop its VM yet; update harness'));
    }
    return this.#ask(connection, VM_STOP_TIMEOUT_MS, (request) => ({ type: 'vm.stop', request }));
  }

  /**
   * Asks a device to switch a project's apps and waits for its answer. The
   * device decides: only offered projects, only apps it knows. Its new apps
   * report arrives before the answer.
   *
   * @param deviceId - Device id.
   * @param project - Project on the device.
   * @param enabled - Apps the project has switched on from now on.
   * @returns The device's answer.
   * @throws {HttpError} 409 if the device is offline or too old for it,
   *   504 if it does not answer in time.
   */
  setApps(deviceId: string, project: string, enabled: string[]): Promise<AppsSetResult> {
    const connection = this.#devices.get(deviceId);
    if (!connection?.greeted) {
      return Promise.reject(new HttpError(409, 'device_offline', 'The device is not connected'));
    }
    if (connection.version < VERSION_APPS) {
      return Promise.reject(new HttpError(409, 'device_outdated', 'The device cannot switch apps yet; update harness'));
    }
    return this.#ask(connection, APPS_SET_TIMEOUT_MS, (request) => ({ type: 'apps.set', request, project, enabled }));
  }

  /**
   * Asks a device for a program file as a new app. The device checks the
   * file and stores a request that only the user can approve there (tray
   * icon or `harness app approve`); nothing of the file moves before.
   *
   * @param deviceId - Device id.
   * @param file - Path on the device, app name and title.
   * @returns The device's answer, with the id to approve on the computer.
   * @throws {HttpError} 409 if the device is offline or too old for it,
   *   504 if it does not answer in time.
   */
  requestAppFile(deviceId: string, file: { path: string; name: string; title: string }): Promise<AppFileResult> {
    const connection = this.#devices.get(deviceId);
    if (!connection?.greeted) {
      return Promise.reject(new HttpError(409, 'device_offline', 'The device is not connected'));
    }
    if (connection.version < VERSION_APP_FILES) {
      return Promise.reject(new HttpError(409, 'device_outdated', 'The device cannot take program files yet; update harness'));
    }
    return this.#ask(connection, APP_FILE_TIMEOUT_MS, (request) => ({ type: 'apps.file', request, ...file })).then((result) => ({
      ok: result.ok,
      message: result.message,
      approval: 'approval' in result ? (result.approval ?? null) : null,
    }));
  }

  /**
   * Sends a request with a fresh id and waits for the device's answer to it.
   *
   * @param connection - The device's connection.
   * @param timeoutMs - How long to wait.
   * @param message - The request for an id.
   * @returns The answer.
   */
  #ask(
    connection: Connection,
    timeoutMs: number,
    message: (request: string) => ServerMessage,
  ): Promise<VmStopResult | (AppsSetResult & { approval?: string })> {
    const request = randomUUID();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        connection.requests.delete(request);
        reject(new HttpError(504, 'device_timeout', 'The device did not answer in time'));
      }, timeoutMs);
      timer.unref?.();
      connection.requests.set(request, { resolve, reject, timer });
      this.#send(connection, message(request));
    });
  }

  /**
   * Sends a prompt to a device and returns the reply's events.
   *
   * @param deviceId - Device the session runs on.
   * @param prompt - The prompt.
   * @param research - Set when the user switched research mode on: while
   *   the reply runs, the device's agents may search the web
   *   ({@link DeviceHub.researchQuestion}).
   * @returns The run.
   * @throws {HttpError} 409 if the device is offline or the session is still
   *   answering. Whether the project may run is the device's decision; it
   *   refuses with a reason the chat shows.
   */
  startRun(deviceId: string, prompt: PromptMessage, research?: { question: string }): AgentRun {
    if (prompt.files?.length) {
      this.assertFiles(deviceId);
    }
    const run = this.#start(deviceId, prompt);
    const active = this.#runs.get(prompt.session);
    if (active && research) {
      active.research = research;
    }
    return run;
  }

  /**
   * Checks that a device takes files with a prompt; call before storing the
   * message, so an old device leaves no unanswered one behind.
   *
   * @param deviceId - Device id.
   * @throws {HttpError} 409 `device_outdated` below protocol 9. An older
   *   device would drop the files without a word.
   */
  assertFiles(deviceId: string): void {
    const connection = this.#devices.get(deviceId);
    if (connection?.greeted && connection.version < VERSION_FILES) {
      throw new HttpError(409, 'device_outdated', 'The device cannot take files yet; update harness');
    }
  }

  /**
   * Whether an agent on a device may search the web: only while one of its
   * replies runs in research mode. Searches come through the gateway with
   * the device token and no session, so the device as a whole counts.
   *
   * @param deviceId - Device id.
   * @returns The user's question of that reply, which ranks the hits; null
   *   if no such reply runs.
   */
  researchQuestion(deviceId: string): string | null {
    for (const active of this.#runs.values()) {
      if (active.deviceId === deviceId && active.research) {
        return active.research.question;
      }
    }
    return null;
  }

  /**
   * Asks a device to take over a session's agent work into the user's
   * folder; the events of the run report how far it got.
   *
   * @param deviceId - Device the session runs on.
   * @param message - The request.
   * @returns The run.
   * @throws {HttpError} 409 if the device is offline, too old for it, or the
   *   session is still answering.
   */
  startTakeover(deviceId: string, message: TakeoverMessage): AgentRun {
    const connection = this.#devices.get(deviceId);
    if (connection?.greeted && connection.version < VERSION_TAKEOVER) {
      throw new HttpError(409, 'device_outdated', 'The device cannot take work over yet; update harness');
    }
    return this.#start(deviceId, message);
  }

  /**
   * Asks a device to have the agent of a session summarize its context now;
   * the run reports pi's `compaction` events and ends with `done`.
   *
   * @param deviceId - Device the session runs on.
   * @param message - The request.
   * @returns The run.
   * @throws {HttpError} 409 if the device is offline, too old for it, or the
   *   session is still answering.
   */
  startCompaction(deviceId: string, message: CompactMessage): AgentRun {
    const connection = this.#devices.get(deviceId);
    if (connection?.greeted && connection.version < VERSION_PI_SETTINGS) {
      throw new HttpError(409, 'device_outdated', 'The device cannot summarize the agent yet; update harness');
    }
    return this.#start(deviceId, message);
  }

  /**
   * Gives up on a run the device does not finish, so the session can start
   * a new one.
   *
   * @param session - Session id.
   * @param reason - Why; the run ends with it.
   */
  abandonRun(session: string, reason: RunFailure): void {
    this.#runs.get(session)?.run.fail(reason);
    this.#runs.delete(session);
  }

  #start(deviceId: string, message: PromptMessage | TakeoverMessage | CompactMessage): AgentRun {
    const connection = this.#devices.get(deviceId);
    if (!connection?.greeted) {
      throw new HttpError(409, 'device_offline', 'The device is not connected');
    }
    if (this.#runs.has(message.session)) {
      throw new HttpError(409, 'conflict', 'The agent is still answering');
    }
    const run = new AgentRun(() => this.#send(connection, { type: 'session.cancel', session: message.session }));
    this.#runs.set(message.session, { deviceId, run });
    this.#send(connection, message.type === 'session.prompt' ? forVersion(message, connection.version) : message);
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
        // The device answers the welcome with the lower of both versions.
        if (message.version < MIN_TUNNEL_VERSION || message.version > TUNNEL_VERSION) {
          connection.socket.close(CLOSE_VERSION, `protocol ${MIN_TUNNEL_VERSION}-${TUNNEL_VERSION} required`);
          this.#drop(deviceId, connection, 'device_protocol');
          return;
        }
        connection.greeted = true;
        connection.version = message.version;
        connection.projects = message.projects;
        this.#store(deviceId, message.projects);
        if (connection.version >= VERSION_VM_CONTROL) {
          this.#send(connection, { type: 'settings', idleMinutes: connection.settings.idleMinutes });
        }
        if (connection.version >= VERSION_TEMPLATES) {
          this.#greetWithTemplates(connection);
        }
        return;
      case 'projects':
        connection.projects = message.projects;
        this.#store(deviceId, message.projects);
        return;
      case 'status':
        connection.vm = message.vm;
        connection.activity =
          message.replies === undefined
            ? null
            : {
                replies: message.replies,
                terminals: message.terminals ?? 0,
                // Relative on the wire, so the device's clock does not matter.
                stopsAt: message.stopIn === null || message.stopIn === undefined ? null : Date.now() + message.stopIn * 1000,
              };
        return;
      case 'vm.result':
      case 'apps.result': {
        const pending = connection.requests.get(message.request);
        if (pending) {
          clearTimeout(pending.timer);
          connection.requests.delete(message.request);
          pending.resolve({
            ok: message.ok,
            message: message.message,
            ...(message.type === 'apps.result' && message.approval ? { approval: message.approval } : {}),
          });
        }
        return;
      }
      case 'apps':
        connection.apps = message.report;
        void Promise.resolve(this.listener.apps?.(deviceId, message.report)).catch((error: unknown) => {
          console.warn('[tunnel] storing apps failed:', error instanceof Error ? error.message : error);
        });
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

  #greetWithTemplates(connection: Connection): void {
    const load = this.listener.templates;
    if (!load) {
      return;
    }
    const before = connection.templatesSent;
    void Promise.resolve(load(connection.userId))
      .then((templates) => {
        // A change in the meantime went out already, and it is newer.
        const current = [...this.#devices.values()].includes(connection);
        if (current && connection.templatesSent === before) {
          connection.templatesSent += 1;
          this.#send(connection, { type: 'apps.templates', templates });
        }
      })
      .catch((error: unknown) => {
        console.warn('[tunnel] loading own programs failed:', error instanceof Error ? error.message : error);
      });
  }

  #store(deviceId: string, projects: RemoteProject[]): void {
    void Promise.resolve(this.listener.projects?.(deviceId, projects)).catch((error: unknown) => {
      console.warn('[tunnel] storing projects failed:', error instanceof Error ? error.message : error);
    });
  }

  #drop(deviceId: string, connection: Connection, reason: RunFailure): void {
    // The late close of a replaced socket must not end the new one's runs.
    if (this.#devices.get(deviceId) !== connection) {
      return;
    }
    this.#devices.delete(deviceId);
    for (const pending of connection.requests.values()) {
      clearTimeout(pending.timer);
      pending.reject(new HttpError(409, 'device_offline', 'The device disconnected'));
    }
    connection.requests.clear();
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
