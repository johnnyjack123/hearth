/**
 * A page that was open while the server got a new version still knows the
 * file names of the old build. Parts of the app load on demand (diagrams,
 * syntax highlighting, some pages), and their old files are gone after the
 * update, so loading one fails ("Failed to fetch dynamically imported
 * module"). A reload brings the new build; answers keep running on the
 * server meanwhile, so nothing is lost.
 */

const LAST_RELOAD_KEY = 'hearth.staleBuildReload';
/** A second failure this soon after a reload is a real error, not an update. */
const RELOAD_GUARD_MS = 30_000;
/** Set once a reload is under way, so a second failure does not count as a loop. */
let reloading = false;

const STALE_BUILD_ERROR =
  /Failed to fetch dynamically imported module|error loading dynamically imported module|Importing a module script failed|Unable to preload CSS/i;

/**
 * Whether an error comes from a part of the app that could not be loaded,
 * as happens after an update.
 *
 * @param error - Anything thrown.
 * @returns True for failed dynamic imports.
 */
export function isStaleBuildError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : typeof error === 'string' ? error : '';
  return STALE_BUILD_ERROR.test(message);
}

/**
 * Reloads the page to get the new build, unless that was just tried.
 *
 * @returns True when the page reloads.
 */
export function reloadForNewBuild(): boolean {
  if (reloading) {
    return true;
  }
  const now = Date.now();
  try {
    const last = Number(sessionStorage.getItem(LAST_RELOAD_KEY) ?? 0);
    if (now - last < RELOAD_GUARD_MS) {
      return false;
    }
    sessionStorage.setItem(LAST_RELOAD_KEY, String(now));
  } catch {
    // Without storage there is no guard against a loop; show the error.
    return false;
  }
  reloading = true;
  window.location.reload();
  return true;
}

/**
 * Reloads when Vite reports that a part of the app could not be loaded,
 * before the error reaches the page.
 */
export function installStaleBuildReload(): void {
  window.addEventListener('vite:preloadError', (event) => {
    if (reloadForNewBuild()) {
      event.preventDefault();
    }
  });
}
