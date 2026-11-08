import { type PublicSettings, type SearchTestResult, updateServerSettingsSchema } from '@harness/shared';
import { Hono } from 'hono';
import { type AppDeps, type AppEnv, requireAdmin } from '../http/context.ts';
import {
  countUsers,
  getServerSettings,
  loadSearchBackend,
  updateSearchSettings,
  updateServerSettings,
} from '../services/settings.ts';
import { SERVER_COMMIT, SERVER_VERSION } from '../version.ts';
import { DEFAULT_SEARCH_ENDPOINTS, WebSearchError } from '../websearch/providers.ts';

/**
 * Public settings, readable without a session (mounted at `/api/settings`).
 *
 * @param deps - Application dependencies.
 * @returns The router.
 */
export function publicSettingsRoutes(deps: AppDeps) {
  const { db } = deps.database;
  return new Hono().get('/', async (c) => {
    const settings = await getServerSettings(db);
    const hasUsers = (await countUsers(db)) > 0;
    const body: PublicSettings = {
      registration: settings.registration,
      hasUsers,
      setupCode: !hasUsers && Boolean(deps.config.setupCode),
      allowCustomBaseUrls: settings.allowCustomBaseUrls,
      webSearch: settings.search.provider !== 'off',
      version: { server: SERVER_VERSION, commit: SERVER_COMMIT },
    };
    return c.json(body);
  });
}

/** Terms of the admins' test search: harmless and always with hits. */
const TEST_QUERY = 'open source chat';

/**
 * Admin-only server settings (mounted at `/api/admin/settings`). User
 * management itself is served by the Better Auth admin plugin.
 *
 * @param deps - Application dependencies.
 * @returns The router.
 */
export function adminSettingsRoutes(deps: AppDeps) {
  const { db } = deps.database;
  const endpoints = { ...DEFAULT_SEARCH_ENDPOINTS, ...deps.webSearch };
  return new Hono<AppEnv>()
    .use(requireAdmin())
    .get('/', async (c) => c.json(await getServerSettings(db)))
    .patch('/', async (c) => {
      const { search, ...rest } = updateServerSettingsSchema.parse(await c.req.json());
      if (search) {
        await updateSearchSettings(db, deps.secretBox, search);
      }
      return c.json(await updateServerSettings(db, rest));
    })
    // Searches once with the stored settings, so admins see at once whether
    // SearXNG answers with JSON or the key works.
    .post('/search/test', async (c) => {
      const backend = await loadSearchBackend(db, deps.secretBox, endpoints);
      if (!backend) {
        return c.json<SearchTestResult>({
          ok: false,
          code: 'not_configured',
          detail: null,
          message: 'No search provider is set up, or its key cannot be read.',
        });
      }
      try {
        const results = await backend.search({ query: TEST_QUERY, count: 5, signal: c.req.raw.signal });
        return c.json<SearchTestResult>({ ok: true, count: results.length, titles: results.slice(0, 3).map((result) => result.title) });
      } catch (error) {
        if (error instanceof WebSearchError) {
          return c.json<SearchTestResult>({
            ok: false,
            code: error.code,
            detail: error.detail ?? null,
            message: error.message.replace(/^\[[^\]]*\]\s*/, ''),
          });
        }
        throw error;
      }
    });
}
