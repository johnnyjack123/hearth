import { adminClient } from 'better-auth/client/plugins';
import { createAuthClient } from 'better-auth/react';

/**
 * Better Auth client for sign-in, sessions and the admin API. The server
 * mounts Better Auth at `/api/auth` on the same origin.
 */
export const authClient = createAuthClient({
  basePath: '/api/auth',
  plugins: [adminClient()],
});

/**
 * Session as returned by Better Auth, or null when signed out.
 */
export type Session = typeof authClient.$Infer.Session;

/**
 * Checks for the admin role. Roles are stored comma-separated.
 *
 * @param user - Signed-in user, if any.
 * @returns True for admins.
 */
export function isAdmin(user: { role?: string | null } | null | undefined): boolean {
  return (user?.role ?? '').split(',').map((role) => role.trim()).includes('admin');
}
