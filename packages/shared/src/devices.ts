import { z } from 'zod';
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
