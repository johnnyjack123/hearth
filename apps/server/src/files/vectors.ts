/**
 * Embeddings of long documents, made once per document and kept in
 * `fileVectors`. A search in a document then embeds only the question, and
 * every section takes part, not only the 200 best by keywords that a web
 * page sends with each call. Qwen3-Embedding-8B cost 0.01 $ per million
 * tokens on 2.11.2026: a document of 500,000 characters (~130,000 tokens)
 * costs about 0.13 cents once.
 */
import { createHash } from 'node:crypto';
import type { Kysely } from 'kysely';
import type { DB } from '../db/schema.ts';
import { embed, type Helpers, vectorOf } from '../helpers/openrouter.ts';
import type { KeptVectors } from '../webfetch/sections.ts';

function encode(vector: Float32Array | null): string | null {
  if (!vector) {
    return null;
  }
  // Little-endian, as `vectorOf` reads it, whatever the machine.
  const bytes = Buffer.alloc(vector.length * 4);
  vector.forEach((value, index) => bytes.writeFloatLE(value, index * 4));
  return bytes.toString('base64');
}

/** The kept embeddings of the documents of one database. */
export class DocumentVectors {
  /** Embeddings being made, so a second search of a document waits for the first. */
  readonly #making = new Map<string, Promise<(Float32Array | null)[] | null>>();

  /**
   * @param db - Open database.
   */
  constructor(private readonly db: Kysely<DB>) {}

  /**
   * The vectors of one document for `selectSections`.
   *
   * @param fileId - The document.
   * @returns Its vector source.
   */
  of(fileId: string): KeptVectors {
    return (inputs, helpers, signal) => {
      const key = `${fileId}\n${helpers.embeddingModel}`;
      let making = this.#making.get(key);
      if (!making) {
        making = this.#load(fileId, inputs, helpers, signal).finally(() => this.#making.delete(key));
        this.#making.set(key, making);
      }
      return making;
    };
  }

  async #load(fileId: string, inputs: string[], helpers: Helpers, signal: AbortSignal | undefined): Promise<(Float32Array | null)[] | null> {
    const model = helpers.embeddingModel;
    const hash = createHash('sha256').update(JSON.stringify(inputs)).digest('hex');
    const row = await this.db.selectFrom('fileVectors').selectAll().where('fileId', '=', fileId).executeTakeFirst();
    const vectors: (Float32Array | null)[] = inputs.map(() => null);
    if (row && row.model === model && row.hash === hash) {
      const stored = JSON.parse(row.vectors) as unknown[];
      stored.slice(0, inputs.length).forEach((value, index) => {
        vectors[index] = typeof value === 'string' ? vectorOf(value) : null;
      });
    }
    // Only what is missing: everything the first time, after a failed
    // batch only its sections.
    const missing = vectors.flatMap((vector, index) => (vector ? [] : [index]));
    if (missing.length > 0) {
      const made = await embed(
        helpers,
        missing.map((index) => inputs[index] ?? ''),
        signal,
      );
      let added = false;
      missing.forEach((index, place) => {
        const vector = made?.[place];
        if (vector) {
          vectors[index] = vector;
          added = true;
        }
      });
      if (added) {
        const value = { model, hash, vectors: JSON.stringify(vectors.map(encode)), updatedAt: Date.now() };
        try {
          await this.db
            .insertInto('fileVectors')
            .values({ fileId, ...value })
            .onConflict((oc) => oc.column('fileId').doUpdateSet(value))
            .execute();
        } catch (error) {
          // The chat was deleted meanwhile, or the database is busy: this
          // search still has its vectors, the next one makes them again.
          console.warn(`[files] vectors of ${fileId} not kept: ${error instanceof Error ? error.message : String(error)}`);
        }
      }
    }
    return vectors.some((vector) => vector !== null) ? vectors : null;
  }
}

const kept = new WeakMap<Kysely<DB>, DocumentVectors>();

/**
 * The document vectors of a database, one per server.
 *
 * @param db - Open database.
 * @returns The store.
 */
export function documentVectors(db: Kysely<DB>): DocumentVectors {
  let store = kept.get(db);
  if (!store) {
    store = new DocumentVectors(db);
    kept.set(db, store);
  }
  return store;
}
