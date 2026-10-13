/**
 * Whether a redirect target stays on this site: a path, not `//host` or a
 * full URL, so a crafted login link cannot send people elsewhere.
 *
 * @param value - Candidate from the URL.
 * @returns True for same-site paths.
 */
export function isLocalPath(value: unknown): value is string {
  return typeof value === 'string' && value.startsWith('/') && !value.startsWith('//') && !value.includes('\\');
}
