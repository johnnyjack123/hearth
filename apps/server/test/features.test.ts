import { type ChatFeatures, DEFAULT_CHAT_FEATURES } from '@hearth/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { getServerSettings } from '../src/services/settings.ts';
import { createTestServer, DIALECTS, SETUP_CODE, type TestServer } from './helpers.ts';

describe.each(DIALECTS)('chat features and registration on %s', (kind) => {
  let server: TestServer;

  beforeEach(async () => {
    server = await createTestServer(kind);
  });

  afterEach(async () => {
    await server.close();
  });

  const features = async (cookie: string) => (await (await server.request('GET', '/api/features', { cookie })).json()) as ChatFeatures;

  it('keeps chat features per user', async () => {
    const admin = await server.signUp('admin@example.com');
    const user = await server.signUp('user@example.com');
    expect(await features(user)).toEqual(DEFAULT_CHAT_FEATURES);

    const changed = await server.request('PATCH', '/api/features', { cookie: user, body: { calculator: false, openRouterNoFourBit: true } });
    expect(changed.status).toBe(200);
    expect(await changed.json()).toEqual({ ...DEFAULT_CHAT_FEATURES, calculator: false, openRouterNoFourBit: true });
    expect(await features(user)).toMatchObject({ calculator: false, openRouterNoFourBit: true });
    // Another user, the admin too, keeps their own.
    expect(await features(admin)).toEqual(DEFAULT_CHAT_FEATURES);

    expect((await server.request('PATCH', '/api/features', { cookie: user, body: { calculator: 'no' } })).status).toBe(400);
    expect((await server.request('PATCH', '/api/features', { cookie: user, body: { registration: 'open' } })).status).toBe(400);
    expect((await server.request('GET', '/api/features')).status).toBe(401);
  });

  it('takes what an admin set for the server before, until the user sets their own', async () => {
    const user = await server.signUp('user@example.com');
    // Stored by a server before 0.1.22, when the features were server settings.
    await server.database.db.insertInto('setting').values({ key: 'webFetch', value: 'false' }).execute();
    expect((await features(user)).webFetch).toBe(false);
    await server.request('PATCH', '/api/features', { cookie: user, body: { webFetch: true } });
    expect((await features(user)).webFetch).toBe(true);
  });

  it('no longer takes the features as server settings', async () => {
    const admin = await server.signUp('admin@example.com');
    expect((await server.request('PATCH', '/api/admin/settings', { cookie: admin, body: { webFetch: false } })).status).toBe(400);
    const settings = (await (await server.request('GET', '/api/admin/settings', { cookie: admin })).json()) as Record<string, unknown>;
    expect(settings).not.toHaveProperty('memory');
    expect(settings).not.toHaveProperty('openRouterCheapest');
  });

  it('keeps registration closed on a fresh server, but never for the first account', async () => {
    // The test helper opens it; a fresh server has no stored value.
    await server.database.db.deleteFrom('setting').where('key', '=', 'registration').execute();
    expect((await getServerSettings(server.database.db)).registration).toBe('closed');
    const settings = (await (await server.request('GET', '/api/settings')).json()) as { registration: string; hasUsers: boolean };
    expect(settings).toMatchObject({ registration: 'closed', hasUsers: false });

    await server.signUp('first@example.com');
    const second = await server.request('POST', '/api/auth/sign-up/email', {
      body: { email: 'second@example.com', name: 'second', password: 'correct-horse-battery' },
      headers: { 'x-hearth-setup-code': SETUP_CODE },
    });
    expect(second.status).toBe(403);
    expect(((await second.json()) as { code: string }).code).toBe('REGISTRATION_CLOSED');
  });
});
