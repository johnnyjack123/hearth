/**
 * `web_fetch` for agents on a paired computer (pi in an instance). The
 * project's gateway has already checked the host against the project's
 * network rules, asking the user where they say so, and passes on the URL
 * together with the host it checked. The server loads the page as in the
 * web chat, but follows redirects only within that host: a redirect to
 * another host goes back to the agent, and the gateway checks its next
 * request like any other.
 */
import type { WebFetchOutput } from '@hearth/shared';
import type { Helpers } from '../helpers/openrouter.ts';
import { WebFetchError } from './errors.ts';
import { parseTarget } from './target.ts';
import { loadPage, modelText, type PageRequest, type WebFetchConfig } from './tool.ts';

/** Header with the URL the agent wants to load; the gateway checks its host. */
export const URL_HEADER = 'x-hearth-url';

/**
 * Header with the host the gateway allowed. The gateway sets it itself and
 * drops any copy the agent sends.
 */
export const APPROVED_HOST_HEADER = 'x-hearth-approved-host';

/** What the agent learns about a loaded page besides the model's text. */
export type AgentPage = Pick<WebFetchOutput, 'url' | 'status' | 'contentType' | 'title' | 'truncated' | 'warnings' | 'findings'>;

/**
 * Answer to the agent: the page, as text for the model plus a summary, or
 * the URL of a redirect to another host, which the agent requests anew.
 */
export type AgentFetchResult = { text: string; page: AgentPage } | { redirect: string };

/** Host names compare without case and trailing dot, as the gateway prints them. */
function sameHost(hostname: string, approved: string): boolean {
  const normalize = (host: string) => host.toLowerCase().replace(/\.$/, '');
  return normalize(hostname) === normalize(approved);
}

/** Stops a redirect to a host the gateway did not check. */
class Elsewhere extends Error {
  constructor(readonly url: URL) {
    super('Redirect to another host');
  }
}

/**
 * Loads a page for an agent.
 *
 * @param raw - URL from {@link URL_HEADER}.
 * @param approvedHost - Host from {@link APPROVED_HOST_HEADER}.
 * @param request - Sections, lines or the whole page, as in the web chat.
 * @param options - Limits and checks, the user's helper models and the
 *   request's abort signal.
 * @returns See {@link AgentFetchResult}.
 * @throws {WebFetchError} On invalid or blocked URLs, a URL whose host is
 *   not the checked one, and everything `fetchPage` throws.
 */
export async function agentFetch(
  raw: string,
  approvedHost: string,
  request: PageRequest,
  options: { config: WebFetchConfig; helpers: Helpers | null; signal?: AbortSignal },
): Promise<AgentFetchResult> {
  const start = parseTarget(raw.trim(), options.config);
  // The gateway and this parser read the URL independently; any difference
  // would load a host nobody checked.
  if (!sameHost(start.hostname, approvedHost)) {
    throw new WebFetchError('invalid_url', `The host ${start.hostname} is not the one the network rules were checked for.`);
  }
  try {
    const output = await loadPage(start, request, {
      ...options,
      onRedirect: async (next) => {
        if (!sameHost(next.hostname, approvedHost)) {
          throw new Elsewhere(next);
        }
      },
    });
    const { url, status, contentType, title, truncated, warnings, findings } = output;
    return { text: modelText(output), page: { url, status, contentType, title, truncated, warnings, findings } };
  } catch (error) {
    if (error instanceof Elsewhere) {
      return { redirect: error.url.href };
    }
    throw error;
  }
}
