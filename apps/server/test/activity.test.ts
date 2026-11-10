import type { ServerActivity } from '@hearth/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { ActivityTracker } from '../src/http/activity.ts';
import { ActiveStreams } from '../src/streams.ts';
import { createTestServer, DIALECTS, type TestServer } from './helpers.ts';

describe('activity tracker and running replies', () => {
  it('remembers the last request per user', () => {
    let now = 1_000;
    const tracker = new ActivityTracker(() => now);
    expect(tracker.since).toBe(1_000);
    expect(tracker.lastSeen('a')).toBeNull();
    now = 5_000;
    tracker.touch('a');
    expect(tracker.lastSeen('a')).toBe(5_000);
    expect(tracker.users()).toEqual(['a']);
  });

  it('lists conversations whose reply is still being written', async () => {
    const streams = new ActiveStreams();
    let finish: () => void = () => {};
    const open = new ReadableStream<string>({
      start(controller) {
        finish = () => controller.close();
      },
    });
    streams.add('c1', open, () => {});
    expect(streams.running()).toEqual(['c1']);
    finish();
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(streams.running()).toEqual([]);
  });
});

describe.each(DIALECTS)('server activity for admins on %s', (kind) => {
  let server: TestServer;

  beforeEach(async () => {
    server = await createTestServer(kind);
  });

  afterEach(async () => {
    await server.close();
  });

  it('shows admins who used the web app since the start, nobody else may look', async () => {
    const admin = await server.signUp('admin@example.com');
    const user = await server.signUp('user@example.com');
    expect((await server.request('GET', '/api/admin/activity', { cookie: user })).status).toBe(403);
    await server.request('GET', '/api/conversations', { cookie: user });

    const response = await server.request('GET', '/api/admin/activity', { cookie: admin });
    expect(response.status).toBe(200);
    const activity = (await response.json()) as ServerActivity;
    const ids = await server.database.db.selectFrom('user').select(['id', 'email']).execute();
    const idOf = (email: string) => ids.find((row) => row.email === email)?.id;
    const seen = new Map(activity.users.map((entry) => [entry.userId, entry]));
    expect(seen.get(idOf('user@example.com') ?? '')).toMatchObject({ replies: 0, devices: 0 });
    expect(seen.get(idOf('user@example.com') ?? '')?.lastSeenAt).toBeGreaterThanOrEqual(activity.since);
    // The admin's own request counts as well.
    expect(seen.get(idOf('admin@example.com') ?? '')?.lastSeenAt).not.toBeNull();
  });
});
