/**
 * Sources that usually serve a kind of question well. They only weigh:
 * a preferred hit moves up a few places, nothing is removed, and the model
 * still sees and may read everything. Without a kind nothing moves.
 */
import type { SearchKind, WebSearchResult } from '@hearth/shared';

/**
 * Domains per kind. An entry matches the domain and its subdomains; an
 * entry starting with a dot matches a suffix such as `.gov`.
 */
const PREFERRED: Record<SearchKind, readonly string[]> = {
  facts: [
    'wikipedia.org',
    'britannica.com',
    '.gov',
    '.gov.uk',
    '.edu',
    '.ac.uk',
    'europa.eu',
    'bund.de',
    'destatis.de',
    'rki.de',
    'admin.ch',
    'gv.at',
    'who.int',
    'un.org',
    'oecd.org',
    'worldbank.org',
    'ourworldindata.org',
    'nature.com',
    'science.org',
    'sciencedirect.com',
    'springer.com',
    'arxiv.org',
    'mpg.de',
    'fraunhofer.de',
    'helmholtz.de',
    'spektrum.de',
    'verbraucherzentrale.de',
    'test.de',
    'duden.de',
    'dwds.de',
  ],
  learning: [
    'studyflix.de',
    'simpleclub.com',
    'serlo.org',
    'khanacademy.org',
    'leifiphysik.de',
    'lernhelfer.de',
    'sofatutor.com',
    'mathebibel.de',
    'frustfrei-lernen.de',
    'bpb.de',
    'planet-wissen.de',
    'wikibooks.org',
    'wikiversity.org',
    'wikipedia.org',
    'britannica.com',
    'brilliant.org',
  ],
  experiences: [
    'reddit.com',
    'stackexchange.com',
    'gutefrage.net',
    'computerbase.de',
    'hardwareluxx.de',
    'motor-talk.de',
    'chefkoch.de',
    'tripadvisor.com',
    'tripadvisor.de',
    'trustpilot.com',
    'drwindows.de',
    'android-hilfe.de',
    'mikrocontroller.net',
  ],
  tech: [
    // Makers of platforms and devices, with their docs and support pages.
    'microsoft.com',
    'google.com',
    'android.com',
    'apple.com',
    'mozilla.org',
    'chrome.com',
    'chromium.org',
    'web.dev',
    'samsung.com',
    'raspberrypi.com',
    'nvidia.com',
    'intel.com',
    'amd.com',
    'docs.aws.amazon.com',
    'cloudflare.com',
    'ubuntu.com',
    'debian.org',
    'archlinux.org',
    'fedoraproject.org',
    'redhat.com',
    'ubuntuusers.de',
    // Languages, frameworks and tools.
    'python.org',
    'nodejs.org',
    'docs.rs',
    'rust-lang.org',
    'go.dev',
    'typescriptlang.org',
    'react.dev',
    'kernel.org',
    'man7.org',
    'readthedocs.io',
    'github.com',
    'gitlab.com',
    'stackoverflow.com',
    'superuser.com',
    'serverfault.com',
    'askubuntu.com',
    'stackexchange.com',
    'npmjs.com',
    'pypi.org',
    'crates.io',
    'docker.com',
    'kubernetes.io',
    'postgresql.org',
    'sqlite.org',
    'w3.org',
    'whatwg.org',
    'git-scm.com',
    'jetbrains.com',
    'visualstudio.com',
    'nginx.org',
    'apache.org',
    'php.net',
    'kotlinlang.org',
    'swift.org',
    'dart.dev',
    'flutter.dev',
    'angular.dev',
    'vuejs.org',
    'svelte.dev',
    'nextjs.org',
    'vite.dev',
    'tailwindcss.com',
    'home-assistant.io',
    'openwrt.org',
  ],
  news: [
    'tagesschau.de',
    'zdf.de',
    'deutschlandfunk.de',
    'br.de',
    'ndr.de',
    'wdr.de',
    'swr.de',
    'dw.com',
    'spiegel.de',
    'zeit.de',
    'sueddeutsche.de',
    'faz.net',
    'handelsblatt.com',
    'nzz.ch',
    'srf.ch',
    'derstandard.at',
    'orf.at',
    'heise.de',
    'golem.de',
    'netzpolitik.org',
    'reuters.com',
    'apnews.com',
    'bbc.com',
    'bbc.co.uk',
    'theguardian.com',
    'euronews.com',
  ],
};

/** Image collections that crowd out answers for any kind of question. */
const DEMOTED: readonly string[] = ['pinterest.com', 'pinterest.de', 'pinterest.at', 'pinterest.ch'];

/** Places a preferred hit moves up, or a demoted one down. */
const SHIFT = 5;
/** How much a preferred source adds to a score of 0 to 1: a tie-breaker, not a trump. */
const SCORE_BONUS = 0.15;

function matches(host: string, entry: string): boolean {
  return entry.startsWith('.') ? host.endsWith(entry) : host === entry || host.endsWith(`.${entry}`);
}

function hostOf(url: string): string {
  try {
    return new URL(url).hostname.toLowerCase().replace(/\.$/, '');
  } catch {
    return '';
  }
}

/**
 * Whether a URL belongs to the preferred sources of a kind of question.
 *
 * @param url - Address of a hit.
 * @param kind - Kind of question.
 * @returns True if its host is on the kind's list.
 */
export function isPreferred(url: string, kind: SearchKind): boolean {
  const host = hostOf(url);
  return host !== '' && PREFERRED[kind].some((entry) => matches(host, entry));
}

/**
 * How a hit's source counts for a kind of question.
 *
 * @param url - Address of a hit.
 * @param kind - Kind of question, if the model named one.
 * @returns 1 for a preferred source, -1 for an image collection, else 0.
 */
export function sourceWeight(url: string, kind: SearchKind | undefined): -1 | 0 | 1 {
  if (kind !== undefined && isPreferred(url, kind)) {
    return 1;
  }
  const host = hostOf(url);
  return DEMOTED.some((entry) => matches(host, entry)) ? -1 : 0;
}

/**
 * Orders hits by Jev's scores (0 to 1), a preferred source of the kind a
 * little higher and an image collection a little lower; equal values keep
 * the provider's order.
 *
 * @param results - Hits, best first by the provider.
 * @param scores - Jev's score per hit, in the same order.
 * @param kind - Kind of question, if the model named one.
 * @returns The hits in their new order, with relevance and preferred marks.
 */
export function rankByScores(results: WebSearchResult[], scores: number[], kind: SearchKind | undefined): WebSearchResult[] {
  return results
    .map((result, index) => {
      const weight = sourceWeight(result.url, kind);
      const relevance = scores[index] ?? 0;
      return {
        result: { ...result, relevance, ...(weight === 1 ? { preferred: true } : {}) },
        value: relevance + weight * SCORE_BONUS,
        index,
      };
    })
    .sort((a, b) => b.value - a.value || a.index - b.index)
    .map(({ result }) => result);
}

/**
 * Orders hits by the provider's rank, preferred sources of the kind a few
 * places higher and image collections a few places lower; equal places
 * keep the provider's order.
 *
 * @param results - Hits, best first.
 * @param kind - Kind of question, if the model named one.
 * @returns The hits in their new order, preferred ones marked.
 */
export function weighByKind(results: WebSearchResult[], kind: SearchKind | undefined): WebSearchResult[] {
  return results
    .map((result, index) => {
      const weight = sourceWeight(result.url, kind);
      return { result: weight === 1 ? { ...result, preferred: true } : result, place: index - weight * SHIFT, index };
    })
    .sort((a, b) => a.place - b.place || a.index - b.index)
    .map(({ result }) => result);
}
