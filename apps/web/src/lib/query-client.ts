import { MutationCache, QueryCache, QueryClient } from '@tanstack/react-query';
import { ApiError } from './api.ts';

let onUnauthorized: () => void = () => {};

/**
 * Registers what happens when any request reports an expired session. The
 * router registers a redirect to the login page; kept as a callback to avoid
 * an import cycle between router and query client.
 *
 * @param handler - Called on every 401 response.
 */
export function setUnauthorizedHandler(handler: () => void): void {
  onUnauthorized = handler;
}

function handleError(error: unknown): void {
  if (error instanceof ApiError && error.status === 401) {
    onUnauthorized();
  }
}

/**
 * The app-wide TanStack Query client.
 */
export const queryClient = new QueryClient({
  queryCache: new QueryCache({ onError: handleError }),
  mutationCache: new MutationCache({ onError: handleError }),
  defaultOptions: {
    queries: {
      retry: (count, error) => !(error instanceof ApiError && error.status < 500) && count < 2,
      refetchOnWindowFocus: false,
    },
  },
});
