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
 * Kysely database schema of the tables this app queries.
 */
export interface DB {
  user: UserTable;
  providerCredential: ProviderCredentialTable;
  conversation: ConversationTable;
  message: MessageTable;
  setting: SettingTable;
}
