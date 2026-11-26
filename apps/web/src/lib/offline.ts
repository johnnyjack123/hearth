/**
 * Reading without a connection. The service worker (`public/sw.js`) keeps
 * the app and the chats it loaded; this module registers it, tells the app
 * when the server is out of reach (a request failed, or the worker answered
 * from its cache), and keeps what the session check needs to start without
 * the server: the account, never the session's token.
 */
import { useSyncExternalStore } from 'react';

/** Who was signed in, for a start without the server. */
const SESSION_KEY = 'hearth.offlineSession';

/** The worker's cache of API reads (`DATA` in `public/sw.js`). */
const DATA_CACHE = 'hearth-data-v1';

let unreachable = false;
const listeners = new Set<() => void>();

function emit(): void {
  for (const listener of listeners) {
    listener();
  }
}

/**
 * Notes whether the last request reached the server.
 *
 * @param reached - False for a network error or an answer from the worker's cache.
 */
export function noteReachable(reached: boolean): void {
  if (unreachable === !reached) {
    return;
  }
  unreachable = !reached;
  emit();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  window.addEventListener('online', listener);
  window.addEventListener('offline', listener);
  return () => {
    listeners.delete(listener);
    window.removeEventListener('online', listener);
    window.removeEventListener('offline', listener);
  };
}

/** @returns Whether the app shows what it kept rather than the server's current state. */
export function isOffline(): boolean {
  return unreachable || !navigator.onLine;
}

/** @returns Whether the app is offline, and re-renders when that changes. */
export function useOffline(): boolean {
  return useSyncExternalStore(subscribe, isOffline);
}

/**
 * Registers the service worker of the production build. Browsers offer one
 * only in secure contexts (HTTPS, localhost); over `http://` in the LAN the
 * app simply works online only.
 */
export function registerServiceWorker(): void {
  if (!import.meta.env.PROD || !('serviceWorker' in navigator)) {
    return;
  }
  navigator.serviceWorker.register('/sw.js').catch(() => undefined);
}

/**
 * Keeps the signed-in account for a start without the server, without
 * anything that would sign in elsewhere (the token, address, browser).
 *
 * @param data - What the session check returned; null forgets it.
 */
export function keepSession(data: { user: object; session: object } | null): void {
  try {
    if (data === null) {
      localStorage.removeItem(SESSION_KEY);
      return;
    }
    const session = { ...data.session, token: '', ipAddress: null, userAgent: null };
    localStorage.setItem(SESSION_KEY, JSON.stringify({ user: data.user, session }));
  } catch {
    // Storage blocked: no start without the server.
  }
}

/**
 * @returns The kept account, or null.
 */
export function keptSession<T>(): T | null {
  try {
    const raw = localStorage.getItem(SESSION_KEY);
    return raw === null ? null : (JSON.parse(raw) as T);
  } catch {
    return null;
  }
}

/**
 * Forgets the kept account and the worker's copies of the account's chats,
 * e.g. when signing out or in: the next person on this browser must not
 * read them without a connection.
 */
export async function clearOfflineData(): Promise<void> {
  keepSession(null);
  if ('caches' in window) {
    await caches.delete(DATA_CACHE).catch(() => false);
  }
}

/**
 * Asks the server whether it is back (its health route, which the worker
 * never answers from its cache).
 *
 * @returns Whether it answered.
 */
export async function serverReachable(): Promise<boolean> {
  try {
    const response = await fetch('/api/health', { cache: 'no-store' });
    noteReachable(response.ok);
    return response.ok;
  } catch {
    noteReachable(false);
    return false;
  }
}
