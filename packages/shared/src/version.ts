/**
 * Program versions of the server and of harness on the computers, so either
 * side can say when the other is older (2.11.2026: the server on the Pi was
 * forgotten in an update, and the new display it brought never showed).
 */

/**
 * Compares two versions like `0.1.15`, numbers part by part; anything after
 * `+` or `-` (commit, pre-release) does not count.
 *
 * @param a - A version.
 * @param b - Another version.
 * @returns Negative if `a` is older, positive if newer, 0 if equal; null if
 *   either is no version (`dev`, a build without one).
 */
export function compareVersions(a: string, b: string): number | null {
  const parts = (version: string) => {
    const core = version.trim().replace(/^v/, '').split(/[+-]/, 1)[0] ?? '';
    return /^\d+(\.\d+)*$/.test(core) ? core.split('.').map(Number) : null;
  };
  const left = parts(a);
  const right = parts(b);
  if (!left || !right) {
    return null;
  }
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    const difference = (left[index] ?? 0) - (right[index] ?? 0);
    if (difference !== 0) {
      return difference;
    }
  }
  return 0;
}
