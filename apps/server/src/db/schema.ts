import type { ProviderKind } from '@harness/shared';

/**
 * Row of the Better Auth `user` table, limited to the columns this app reads.
 * Better Auth owns the table and its migrations.
 */
export interface UserTable {
  id: string;
  email: string;
  name: string;
  role: string | null;
}

/**
 * Row of `providerCredential`: one API key (or endpoint) of one user.
 */
export interface ProviderCredentialTable {
  id: string;
  userId: string;
  kind: ProviderKind;
  label: string;
  baseUrl: string | null;
  /** AES-GCM token produced by {@link SecretBox.encrypt}, bound to `userId:id`. */
  encryptedKey: string | null;
  keyHint: string | null;
  createdAt: number;
  updatedAt: number;
}

/**
 * Row of `conversation`.
 */
export interface ConversationTable {
  id: string;
  userId: string;
  title: string | null;
  credentialId: string | null;
  modelId: string | null;
  createdAt: number;
  updatedAt: number;
}

/**
 * Row of `message`. `parts` and `metadata` hold JSON, stored as text so the
 * schema is identical on SQLite and Postgres.
 */
export interface MessageTable {
  id: string;
  conversationId: string;
  role: 'system' | 'user' | 'assistant';
  parts: string;
  metadata: string | null;
  /** Position within the conversation, starting at 1. */
  seq: number;
  createdAt: number;
}

/**
 * Row of `setting`: a key with a JSON-encoded value.
 */
export interface SettingTable {
  key: string;
  value: string;
}

/**
 * Row of `device`: a paired computer. Only the SHA-256 hash of its token is
 * stored.
 */
export interface DeviceTable {
  id: string;
  userId: string;
  name: string;
  tokenHash: string;
  createdAt: number;
  lastUsedAt: number | null;
  /** Idle minutes until the device stops its VM; null: the default. */
  idleMinutes: number | null;
}

/**
 * Row of `devicePairing`: a pairing in progress or recently finished.
 */
export interface DevicePairingTable {
  id: string;
  /** SHA-256 of the device code the device polls with. */
  deviceCodeHash: string;
  /** Code the user confirms, stored without the dash. */
  userCode: string;
  name: string;
  status: 'pending' | 'approved' | 'denied';
  /** Account that approved or denied. */
  userId: string | null;
  /** Device created on approval. */
  deviceId: string | null;
  /** The new device's token, encrypted, until the device fetched it. */
  encryptedToken: string | null;
  createdAt: number;
  expiresAt: number;
}

/**
 * Row of `chatLink`: a conversation whose replies come from an agent in a
 * project on one of the user's devices.
 */
export interface ChatLinkTable {
  conversationId: string;
  deviceId: string;
  project: string;
  harness: string;
  createdAt: number;
}

/**
 * Row of `deviceProject`: a project a device offers to web chats, kept so
 * it shows up while the device is offline.
 */
export interface DeviceProjectTable {
  deviceId: string;
  name: string;
  /** JSON array of harness names. */
  harnesses: string;
  updatedAt: number;
}

/**
 * Kysely database schema of the tables this app queries.
 */
export interface DB {
  user: UserTable;
  providerCredential: ProviderCredentialTable;
  conversation: ConversationTable;
  message: MessageTable;
  setting: SettingTable;
  device: DeviceTable;
  devicePairing: DevicePairingTable;
  chatLink: ChatLinkTable;
  deviceProject: DeviceProjectTable;
}
