import type { Kysely } from 'kysely';
import { type Migration, type MigrationProvider, Migrator } from 'kysely/migration';
import type { DB } from './schema.ts';

// Rules that keep migrations portable between SQLite and Postgres:
// text ids, bigint epoch-millisecond timestamps, JSON as text, no booleans,
// and only Kysely schema-builder calls (no raw SQL).
const migrations: Record<string, Migration> = {
  '0001_initial': {
    async up(db: Kysely<unknown>) {
      await db.schema
        .createTable('providerCredential')
        .addColumn('id', 'text', (col) => col.primaryKey())
        .addColumn('userId', 'text', (col) => col.notNull().references('user.id').onDelete('cascade'))
        .addColumn('kind', 'text', (col) => col.notNull())
        .addColumn('label', 'text', (col) => col.notNull())
        .addColumn('baseUrl', 'text')
        .addColumn('encryptedKey', 'text')
        .addColumn('keyHint', 'text')
        .addColumn('createdAt', 'bigint', (col) => col.notNull())
        .addColumn('updatedAt', 'bigint', (col) => col.notNull())
        .execute();
      await db.schema
        .createIndex('providerCredential_userId_idx')
        .on('providerCredential')
        .column('userId')
        .execute();

      await db.schema
        .createTable('conversation')
        .addColumn('id', 'text', (col) => col.primaryKey())
        .addColumn('userId', 'text', (col) => col.notNull().references('user.id').onDelete('cascade'))
        .addColumn('title', 'text')
        .addColumn('credentialId', 'text', (col) =>
          col.references('providerCredential.id').onDelete('set null'),
        )
        .addColumn('modelId', 'text')
        .addColumn('createdAt', 'bigint', (col) => col.notNull())
        .addColumn('updatedAt', 'bigint', (col) => col.notNull())
        .execute();
      await db.schema
        .createIndex('conversation_userId_updatedAt_idx')
        .on('conversation')
        .columns(['userId', 'updatedAt'])
        .execute();

      await db.schema
        .createTable('message')
        .addColumn('id', 'text', (col) => col.primaryKey())
        .addColumn('conversationId', 'text', (col) =>
          col.notNull().references('conversation.id').onDelete('cascade'),
        )
        .addColumn('role', 'text', (col) => col.notNull())
        .addColumn('parts', 'text', (col) => col.notNull())
        .addColumn('metadata', 'text')
        .addColumn('seq', 'integer', (col) => col.notNull())
        .addColumn('createdAt', 'bigint', (col) => col.notNull())
        .execute();
      await db.schema
        .createIndex('message_conversationId_seq_idx')
        .on('message')
        .columns(['conversationId', 'seq'])
        .unique()
        .execute();

      await db.schema
        .createTable('setting')
        .addColumn('key', 'text', (col) => col.primaryKey())
        .addColumn('value', 'text', (col) => col.notNull())
        .execute();
    },
    async down(db: Kysely<unknown>) {
      await db.schema.dropTable('message').execute();
      await db.schema.dropTable('conversation').execute();
      await db.schema.dropTable('providerCredential').execute();
      await db.schema.dropTable('setting').execute();
    },
  },
  // Paired computers (harness CLI, desktop app). Tokens and device codes are
  // stored as SHA-256 hashes; a pairing keeps the issued token encrypted only
  // until the device has picked it up.
  '0002_devices': {
    async up(db: Kysely<unknown>) {
      await db.schema
        .createTable('device')
        .addColumn('id', 'text', (col) => col.primaryKey())
        .addColumn('userId', 'text', (col) => col.notNull().references('user.id').onDelete('cascade'))
        .addColumn('name', 'text', (col) => col.notNull())
        .addColumn('tokenHash', 'text', (col) => col.notNull().unique())
        .addColumn('createdAt', 'bigint', (col) => col.notNull())
        .addColumn('lastUsedAt', 'bigint')
        .execute();
      await db.schema.createIndex('device_userId_idx').on('device').column('userId').execute();

      await db.schema
        .createTable('devicePairing')
        .addColumn('id', 'text', (col) => col.primaryKey())
        .addColumn('deviceCodeHash', 'text', (col) => col.notNull().unique())
        .addColumn('userCode', 'text', (col) => col.notNull().unique())
        .addColumn('name', 'text', (col) => col.notNull())
        .addColumn('status', 'text', (col) => col.notNull())
        .addColumn('userId', 'text', (col) => col.references('user.id').onDelete('cascade'))
        .addColumn('deviceId', 'text')
        .addColumn('encryptedToken', 'text')
        .addColumn('createdAt', 'bigint', (col) => col.notNull())
        .addColumn('expiresAt', 'bigint', (col) => col.notNull())
        .execute();
    },
    async down(db: Kysely<unknown>) {
      await db.schema.dropTable('devicePairing').execute();
      await db.schema.dropTable('device').execute();
    },
  },
  // Chats linked to a project on a device (Phase 4). Removing the device or
  // the conversation removes the link.
  '0003_chat_links': {
    async up(db: Kysely<unknown>) {
      await db.schema
        .createTable('chatLink')
        .addColumn('conversationId', 'text', (col) =>
          col.primaryKey().references('conversation.id').onDelete('cascade'),
        )
        .addColumn('deviceId', 'text', (col) => col.notNull().references('device.id').onDelete('cascade'))
        .addColumn('project', 'text', (col) => col.notNull())
        .addColumn('harness', 'text', (col) => col.notNull())
        .addColumn('createdAt', 'bigint', (col) => col.notNull())
        .execute();
      await db.schema.createIndex('chatLink_deviceId_idx').on('chatLink').column('deviceId').execute();
    },
    async down(db: Kysely<unknown>) {
      await db.schema.dropTable('chatLink').execute();
    },
  },
  // Projects a device offers, kept while it is offline.
  '0004_device_projects': {
    async up(db: Kysely<unknown>) {
      await db.schema
        .createTable('deviceProject')
        .addColumn('deviceId', 'text', (col) => col.notNull().references('device.id').onDelete('cascade'))
        .addColumn('name', 'text', (col) => col.notNull())
        .addColumn('harnesses', 'text', (col) => col.notNull())
        .addColumn('updatedAt', 'bigint', (col) => col.notNull())
        .addPrimaryKeyConstraint('deviceProject_pk', ['deviceId', 'name'])
        .execute();
    },
    async down(db: Kysely<unknown>) {
      await db.schema.dropTable('deviceProject').execute();
    },
  },
  // Idle time per device, chosen in the web app.
  '0005_device_idle': {
    async up(db: Kysely<unknown>) {
      await db.schema.alterTable('device').addColumn('idleMinutes', 'integer').execute();
    },
    async down(db: Kysely<unknown>) {
      await db.schema.alterTable('device').dropColumn('idleMinutes').execute();
    },
  },
  // Autosync per linked chat.
  '0006_link_autosync': {
    async up(db: Kysely<unknown>) {
      await db.schema
        .alterTable('chatLink')
        .addColumn('autosync', 'integer', (col) => col.notNull().defaultTo(0))
        .execute();
    },
    async down(db: Kysely<unknown>) {
      await db.schema.alterTable('chatLink').dropColumn('autosync').execute();
    },
  },
  // Whether Jev checks the agent's changes before a device takes them over.
  '0007_device_review': {
    async up(db: Kysely<unknown>) {
      await db.schema
        .alterTable('device')
        .addColumn('jevReview', 'integer', (col) => col.notNull().defaultTo(1))
        .execute();
    },
    async down(db: Kysely<unknown>) {
      await db.schema.alterTable('device').dropColumn('jevReview').execute();
    },
  },
  // The app catalog a device last reported, as JSON (shown while offline).
  '0008_device_apps': {
    async up(db: Kysely<unknown>) {
      await db.schema.alterTable('device').addColumn('apps', 'text').execute();
    },
    async down(db: Kysely<unknown>) {
      await db.schema.alterTable('device').dropColumn('apps').execute();
    },
  },
  '0009_app_templates': {
    async up(db: Kysely<unknown>) {
      await db.schema
        .createTable('appTemplate')
        .addColumn('id', 'text', (col) => col.primaryKey())
        .addColumn('userId', 'text', (col) => col.notNull().references('user.id').onDelete('cascade'))
        .addColumn('name', 'text', (col) => col.notNull())
        .addColumn('title', 'text', (col) => col.notNull())
        .addColumn('description', 'text')
        .addColumn('kind', 'text', (col) => col.notNull())
        .addColumn('packages', 'text', (col) => col.notNull())
        .addColumn('script', 'text', (col) => col.notNull())
        .addColumn('hosts', 'text', (col) => col.notNull())
        .addColumn('path', 'text', (col) => col.notNull())
        .addColumn('requires', 'text', (col) => col.notNull())
        .addColumn('createdAt', 'bigint', (col) => col.notNull())
        .addColumn('updatedAt', 'bigint', (col) => col.notNull())
        .execute();
      await db.schema.createIndex('appTemplate_userId_name_idx').on('appTemplate').columns(['userId', 'name']).unique().execute();
    },
    async down(db: Kysely<unknown>) {
      await db.schema.dropTable('appTemplate').execute();
    },
  },
  '0010_files': {
    async up(db: Kysely<unknown>) {
      await db.schema
        .createTable('file')
        .addColumn('id', 'text', (col) => col.primaryKey())
        .addColumn('userId', 'text', (col) => col.notNull().references('user.id').onDelete('cascade'))
        .addColumn('conversationId', 'text', (col) => col.references('conversation.id').onDelete('cascade'))
        .addColumn('name', 'text', (col) => col.notNull())
        .addColumn('mediaType', 'text', (col) => col.notNull())
        .addColumn('kind', 'text', (col) => col.notNull())
        .addColumn('size', 'bigint', (col) => col.notNull())
        .addColumn('text', 'text')
        .addColumn('truncated', 'integer', (col) => col.notNull())
        .addColumn('createdAt', 'bigint', (col) => col.notNull())
        .execute();
      await db.schema.createIndex('file_userId_idx').on('file').column('userId').execute();
      await db.schema.createIndex('file_conversationId_idx').on('file').column('conversationId').execute();
    },
    async down(db: Kysely<unknown>) {
      await db.schema.dropTable('file').execute();
    },
  },
  '0011_memories': {
    async up(db: Kysely<unknown>) {
      await db.schema
        .createTable('memory')
        .addColumn('id', 'text', (col) => col.primaryKey())
        .addColumn('userId', 'text', (col) => col.notNull().references('user.id').onDelete('cascade'))
        .addColumn('text', 'text', (col) => col.notNull())
        .addColumn('source', 'text', (col) => col.notNull())
        // A memory outlives the chat it came from.
        .addColumn('conversationId', 'text', (col) => col.references('conversation.id').onDelete('set null'))
        .addColumn('createdAt', 'bigint', (col) => col.notNull())
        .addColumn('updatedAt', 'bigint', (col) => col.notNull())
        .execute();
      await db.schema.createIndex('memory_userId_idx').on('memory').column('userId').execute();
    },
    async down(db: Kysely<unknown>) {
      await db.schema.dropTable('memory').execute();
    },
  },
  '0012_file_vectors': {
    async up(db: Kysely<unknown>) {
      await db.schema
        .createTable('fileVectors')
        .addColumn('fileId', 'text', (col) => col.primaryKey().references('file.id').onDelete('cascade'))
        .addColumn('model', 'text', (col) => col.notNull())
        .addColumn('hash', 'text', (col) => col.notNull())
        .addColumn('vectors', 'text', (col) => col.notNull())
        .addColumn('updatedAt', 'bigint', (col) => col.notNull())
        .execute();
    },
    async down(db: Kysely<unknown>) {
      await db.schema.dropTable('fileVectors').execute();
    },
  },
  '0013_device_versions': {
    async up(db: Kysely<unknown>) {
      await db.schema.alterTable('device').addColumn('harnessVersion', 'text').execute();
    },
    async down(db: Kysely<unknown>) {
      await db.schema.alterTable('device').dropColumn('harnessVersion').execute();
    },
  },
  '0014_costs': {
    async up(db: Kysely<unknown>) {
      await db.schema
        .createTable('costDay')
        .addColumn('userId', 'text', (col) => col.notNull().references('user.id').onDelete('cascade'))
        .addColumn('day', 'text', (col) => col.notNull())
        .addColumn('microdollars', 'bigint', (col) => col.notNull())
        .addPrimaryKeyConstraint('costDay_pk', ['userId', 'day'])
        .execute();
      await db.schema
        .createTable('userLimit')
        .addColumn('userId', 'text', (col) => col.primaryKey().references('user.id').onDelete('cascade'))
        .addColumn('dailyMicrodollars', 'bigint')
        .addColumn('updatedAt', 'bigint', (col) => col.notNull())
        .execute();
    },
    async down(db: Kysely<unknown>) {
      await db.schema.dropTable('userLimit').execute();
      await db.schema.dropTable('costDay').execute();
    },
  },
  // Memories models store wait for the user (security review M2).
  '0015_memory_pending': {
    async up(db: Kysely<unknown>) {
      await db.schema
        .alterTable('memory')
        .addColumn('pending', 'integer', (col) => col.notNull().defaultTo(0))
        .execute();
    },
    async down(db: Kysely<unknown>) {
      await db.schema.alterTable('memory').dropColumn('pending').execute();
    },
  },
};

// Migrations are bundled into the server build, so they are provided in code
// instead of being discovered on disk.
const provider: MigrationProvider = {
  getMigrations: async () => migrations,
};

/**
 * Applies all pending app migrations. Must run after Better Auth's migrations
 * because the app tables reference its `user` table.
 *
 * @param db - Open database.
 * @throws {Error} If a migration fails; earlier migrations stay applied.
 */
export async function migrateToLatest(db: Kysely<DB>): Promise<void> {
  const migrator = new Migrator({ db, provider });
  const { error, results } = await migrator.migrateToLatest();
  for (const result of results ?? []) {
    if (result.status === 'Error') {
      console.error(`[db] migration ${result.migrationName} failed`);
    }
  }
  if (error) {
    throw error instanceof Error ? error : new Error(String(error));
  }
}
