import {
  type CredentialModels,
  createProviderCredentialSchema,
  type ProviderKind,
  updateProviderCredentialSchema,
  usesCustomEndpoint,
} from '@harness/shared';
import { Hono } from 'hono';
import type { Kysely } from 'kysely';
import type { DB } from '../db/schema.ts';
import { type AppDeps, type AppEnv, isAdmin, type SessionUser } from '../http/context.ts';
import { HttpError, notFound } from '../http/errors.ts';
import { ProviderRequestError } from '../llm.ts';
import {
  type CredentialRow,
  createCredential,
  deleteCredential,
  findCredential,
  listCredentialRows,
  listCredentials,
  resolveCredential,
  updateCredential,
} from '../services/credentials.ts';
import { getServerSettings } from '../services/settings.ts';

/**
 * Rejects credentials that point at a user-chosen endpoint unless the user is
 * an admin or admins allowed custom endpoints. Without this check any user
 * could make the server send requests into its internal network.
 *
 * @param db - Open database.
 * @param user - Acting user, or the owner of the calling device.
 * @param kind - Provider kind.
 * @param baseUrl - Endpoint stored with the credential, if any.
 * @throws {HttpError} 403 if the endpoint is not allowed.
 */
export async function assertEndpointAllowed(
  db: Kysely<DB>,
  user: { role?: string | null },
  kind: ProviderKind,
  baseUrl: string | null | undefined,
): Promise<void> {
  if (!usesCustomEndpoint(kind, baseUrl) || isAdmin(user)) {
    return;
  }
  const settings = await getServerSettings(db);
  if (!settings.allowCustomBaseUrls) {
    throw new HttpError(403, 'custom_base_url_forbidden', 'Custom endpoints are disabled by the administrator');
  }
}

/**
 * Routes for the signed-in user's provider credentials (`/api/providers`) and
 * the models they offer (`/api/models`).
 *
 * @param deps - Application dependencies.
 * @returns Routers to mount under `/api/providers` and `/api/models`.
 */
export function providerRoutes(deps: AppDeps) {
  const { db } = deps.database;
  const { secretBox, llm } = deps;

  // Listing failures are part of the result so one broken key does not hide the others.
  async function modelsOf(user: SessionUser, row: CredentialRow): Promise<CredentialModels> {
    const base = { credentialId: row.id, kind: row.kind, label: row.label };
    try {
      await assertEndpointAllowed(db, user, row.kind, row.baseUrl);
      const models = await llm.listModels(resolveCredential(secretBox, row));
      // Whether a model reads images decides if the chat offers to attach them.
      const described = await Promise.all(
        models.map(async (model) => {
          const images = (await deps.pricing.lookup(row.kind, row.baseUrl, model.id))?.images;
          return images === undefined ? model : { ...model, images };
        }),
      );
      return { ...base, models: described, error: null };
    } catch (error) {
      if (error instanceof ProviderRequestError || error instanceof HttpError) {
        return { ...base, models: [], error: error.message };
      }
      throw error;
    }
  }

  const providers = new Hono<AppEnv>()
    .get('/', async (c) => c.json(await listCredentials(db, c.get('user').id)))
    .post('/', async (c) => {
      const user = c.get('user');
      const input = createProviderCredentialSchema.parse(await c.req.json());
      await assertEndpointAllowed(db, user, input.kind, input.baseUrl);
      return c.json(await createCredential(db, secretBox, user.id, input), 201);
    })
    .patch('/:id', async (c) => {
      const user = c.get('user');
      const row = await findCredential(db, user.id, c.req.param('id'));
      if (!row) {
        throw notFound('Credential');
      }
      const patch = updateProviderCredentialSchema.parse(await c.req.json());
      const nextBaseUrl = patch.baseUrl === undefined ? row.baseUrl : patch.baseUrl;
      await assertEndpointAllowed(db, user, row.kind, nextBaseUrl);
      const updated = await updateCredential(db, secretBox, row, patch);
      llm.invalidate(row.id);
      return c.json(updated);
    })
    .delete('/:id', async (c) => {
      const row = await findCredential(db, c.get('user').id, c.req.param('id'));
      if (!row) {
        throw notFound('Credential');
      }
      await deleteCredential(db, row);
      llm.invalidate(row.id);
      return c.body(null, 204);
    })
    .get('/:id/models', async (c) => {
      const user = c.get('user');
      const row = await findCredential(db, user.id, c.req.param('id'));
      if (!row) {
        throw notFound('Credential');
      }
      // An explicit connection test must hit the provider, not the cache.
      llm.invalidate(row.id);
      return c.json(await modelsOf(user, row));
    });

  const models = new Hono<AppEnv>().get('/', async (c) => {
    const user = c.get('user');
    const rows = await listCredentialRows(db, user.id);
    return c.json(await Promise.all(rows.map((row) => modelsOf(user, row))));
  });

  return { providers, models };
}
