import { z } from 'zod';
import { MAX_IDLE_MINUTES, type RemoteProject, type VmState } from './agent.ts';
import type { ModelInfo } from './api.ts';
import type { ProviderKind } from './providers.ts';

/**
 * A computer paired with an account (the `harness` CLI or desktop app). Its
 * token only grants LLM access through the server, never the rest of the API.
 */
export interface DeviceSummary {
  /** Device id. */
  id: string;
  /** Name the device reported when pairing, e.g. `harness auf LAPTOP`. */
  name: string;
  /** Pairing time, epoch milliseconds. */
  createdAt: number;
  /** Last LLM request, epoch milliseconds, or null if never used. */
  lastUsedAt: number | null;
  /** Whether the device is connected right now (background service). */
  online: boolean;
  /** Projects it offers to web chats, also while offline. */
  projects: RemoteProject[];
  /** Its VM while online, else null. */
  vm: VmState | null;
  /**
   * What keeps the VM busy and when the device stops it; null while offline
   * or for devices that do not report it yet (protocol 1).
   */
  activity: DeviceActivity | null;
  /** Idle minutes until the device stops its VM; 0: never. */
  idleMinutes: number;
}

/**
 * Use of a device's VM, as the device reports it.
 */
export interface DeviceActivity {
  /** Agent replies running on it right now. */
  replies: number;
  /** harness commands in a terminal that use the VM (`harness run`, `shell` …). */
  terminals: number;
  /**
   * When the device stops the idle VM, epoch milliseconds (server clock);
   * null while something uses it, while it is off, or with idle time 0.
   */
  stopsAt: number | null;
}

/**
 * Validates a change of a device's settings in the web app.
 */
export const updateDeviceSchema = z
  .object({
    idleMinutes: z.number().int().min(0).max(MAX_IDLE_MINUTES),
  })
  .strict();

/** See {@link updateDeviceSchema}. */
export type UpdateDeviceInput = z.infer<typeof updateDeviceSchema>;

/**
 * Answer of a device to "stop the VM".
 */
export interface VmStopResult {
  /** Whether the VM is off now. */
  ok: boolean;
  /** What happened, in German, from the device. */
  message: string;
}

/**
 * Validates the body that starts a pairing (sent by the device, unauthenticated).
 */
export const startPairingSchema = z
  .object({
    name: z.string().trim().min(1).max(80),
  })
  .strict();

/**
 * Answer to a pairing start: the device keeps `deviceCode` secret and polls
 * with it; the user confirms `userCode` in the web app.
 */
export interface PairingStart {
  /** Secret the device polls with. */
  deviceCode: string;
  /** Short code shown on both sides, formatted `ABCD-EFGH`. */
  userCode: string;
  /** Page where the user confirms the pairing. */
  verificationUri: string;
  /** The same page with the code filled in. */
  verificationUriComplete: string;
  /** Seconds until the pairing expires. */
  expiresIn: number;
  /** Seconds the device should wait between polls. */
  interval: number;
}

/**
 * Validates a poll for the device token.
 */
export const pairingTokenSchema = z
  .object({
    deviceCode: z.string().min(16).max(200),
  })
  .strict();

/**
 * Answer to a poll. The token is handed out exactly once.
 */
export type PairingPoll =
  | { status: 'pending' | 'denied' | 'expired' }
  | { status: 'approved'; deviceId: string; token: string };

/**
 * A pairing as shown on the confirmation page.
 */
export interface PairingInfo {
  /** Short code, formatted `ABCD-EFGH`. */
  userCode: string;
  /** Name the device reported. */
  name: string;
  /** Current state. */
  status: 'pending' | 'approved' | 'denied' | 'expired';
  /** Expiry, epoch milliseconds. */
  expiresAt: number;
}

/**
 * Validates the user's decision on a pairing.
 */
export const pairingDecisionSchema = z
  .object({
    approve: z.boolean(),
  })
  .strict();

/**
 * The paired device and its owner, as reported to the device itself.
 */
export interface DeviceSelf {
  /** The device. */
  device: DeviceSummary;
  /** Account the device acts for. */
  user: {
    /** Account email. */
    email: string;
    /** Display name. */
    name: string;
  };
}

/**
 * A credential as seen by a paired device: enough to configure a harness
 * against the server's LLM pass-through, never the key itself.
 */
export interface DeviceCredential {
  /** Credential id; part of the pass-through path. */
  id: string;
  /** Provider kind; decides which API the harness speaks. */
  kind: ProviderKind;
  /** User-chosen display name. */
  label: string;
  /** Custom endpoint, or null for the provider default. */
  baseUrl: string | null;
  /**
   * Models of providers the harness does not know itself (Ollama,
   * OpenAI-compatible); null for providers with a built-in catalog or when
   * listing failed.
   */
  models: ModelInfo[] | null;
}
