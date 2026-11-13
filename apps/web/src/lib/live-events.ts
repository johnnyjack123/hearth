import type { LiveEvent } from '@hearth/shared';

/**
 * What the page hears: the server's events, and `reconnected` when the
 * connection is open again after a break, during which events may have
 * been lost.
 */
export type LiveSignal = LiveEvent | { type: 'reconnected' };

/** Tabs of one browser share a connection; the one holding this lock keeps it. */
const LOCK = 'hearth.events';
const CHANNEL = 'hearth.events';
/** The server pings every 20 s; nothing for this long means the connection died. */
const STALE_MS = 50_000;
const CHECK_MS = 15_000;
/** A refused connection (server restarting, proxy error) is tried again after this, doubling. */
const RETRY_MIN_MS = 2_000;
const RETRY_MAX_MS = 30_000;
const EVENT_TYPES: LiveEvent['type'][] = ['hello', 'ping', 'chat', 'list'];
/** A lock granted later than this was held by another tab before. */
const TAKEOVER_MS = 250;

interface Connection {
  /** Opens the connection anew if it went quiet. */
  check: () => void;
}

/**
 * Keeps `GET /api/events` open until `stop` aborts. EventSource retries a
 * broken connection by itself, but gives up for good on an HTTP error (a
 * proxy's 502 while the server restarts) and never notices one that a
 * phone dropped in the background: both are handled here.
 */
function holdConnection(stop: AbortSignal, deliver: (signal: LiveSignal) => void, takesOver = false): Connection {
  let source: EventSource | null = null;
  let lastSeen = Date.now();
  // Taking over from another tab: what happened in between is unknown.
  let opened = takesOver;
  let retryMs = RETRY_MIN_MS;
  let retryTimer: number | undefined;

  const open = () => {
    source?.close();
    window.clearTimeout(retryTimer);
    lastSeen = Date.now();
    const current = new EventSource('/api/events');
    source = current;
    current.addEventListener('open', () => {
      lastSeen = Date.now();
      retryMs = RETRY_MIN_MS;
      if (opened) {
        deliver({ type: 'reconnected' });
      }
      opened = true;
    });
    for (const type of EVENT_TYPES) {
      current.addEventListener(type, (event) => {
        lastSeen = Date.now();
        try {
          deliver(JSON.parse((event as MessageEvent<string>).data) as LiveEvent);
        } catch {
          // A line this page does not understand (newer server): skip it.
        }
      });
    }
    current.addEventListener('error', () => {
      if (current.readyState === EventSource.CLOSED && source === current && !stop.aborted) {
        retryTimer = window.setTimeout(open, retryMs);
        retryMs = Math.min(retryMs * 2, RETRY_MAX_MS);
      }
    });
  };

  const check = () => {
    if (!stop.aborted && Date.now() - lastSeen > STALE_MS) {
      open();
    }
  };
  const interval = window.setInterval(check, CHECK_MS);
  stop.addEventListener(
    'abort',
    () => {
      window.clearInterval(interval);
      window.clearTimeout(retryTimer);
      source?.close();
    },
    { once: true },
  );
  open();
  return { check };
}

/**
 * Listens to the server's events for this page. Tabs of one browser share
 * one connection (Web Locks elect the tab that holds it, a BroadcastChannel
 * passes the events on): browsers allow only six connections per server
 * over HTTP/1.1, and every tab holding one would soon block the app. Over
 * plain `http://` there are no Web Locks; then each tab connects itself.
 *
 * @param onSignal - Called with every event, in every tab.
 * @returns A function that stops listening.
 */
export function connectLiveEvents(onSignal: (signal: LiveSignal) => void): () => void {
  const stop = new AbortController();
  const shared = typeof BroadcastChannel === 'function' && typeof navigator.locks?.request === 'function';
  const channel = shared ? new BroadcastChannel(CHANNEL) : null;
  let connection: Connection | null = null;
  let lastSeen = Date.now();
  let hiddenAt: number | null = null;

  const deliver = (signal: LiveSignal) => {
    onSignal(signal);
    channel?.postMessage(signal);
  };

  const lead = (steal: boolean) => {
    const resign = new AbortController();
    stop.signal.addEventListener('abort', () => resign.abort(), { once: true });
    const asked = Date.now();
    navigator.locks
      .request(LOCK, steal ? { steal: true } : { signal: stop.signal }, () => {
        if (resign.signal.aborted) {
          return undefined;
        }
        // Granted at once: no tab held it. Granted later: one did until now.
        connection = holdConnection(resign.signal, deliver, steal || Date.now() - asked > TAKEOVER_MS);
        return new Promise<void>((resolve) => resign.signal.addEventListener('abort', () => resolve(), { once: true }));
      })
      .catch(() => {
        // Stopped while waiting, or another tab took over because it heard
        // nothing from this one (frozen in the background): step back and
        // wait in line again.
        resign.abort();
        connection = null;
        if (!stop.signal.aborted) {
          lead(false);
        }
      });
  };

  // Only the visible page acts: a tab in the background is often frozen.
  const check = () => {
    if (document.visibilityState !== 'visible') {
      return;
    }
    if (connection) {
      connection.check();
    } else if (Date.now() - lastSeen > STALE_MS) {
      lastSeen = Date.now();
      lead(true);
    }
  };

  const onVisibility = () => {
    if (document.visibilityState === 'hidden') {
      hiddenAt = Date.now();
      return;
    }
    // Back after a while (phone asleep, app in the background): what this
    // page shows may be old, whatever the connection says.
    if (hiddenAt !== null && Date.now() - hiddenAt > STALE_MS) {
      onSignal({ type: 'reconnected' });
    }
    hiddenAt = null;
    check();
  };

  if (channel) {
    channel.addEventListener('message', (event: MessageEvent<LiveSignal>) => {
      lastSeen = Date.now();
      onSignal(event.data);
    });
    lead(false);
  } else {
    connection = holdConnection(stop.signal, deliver);
  }
  const interval = window.setInterval(check, CHECK_MS);
  document.addEventListener('visibilitychange', onVisibility);

  return () => {
    stop.abort();
    window.clearInterval(interval);
    document.removeEventListener('visibilitychange', onVisibility);
    channel?.close();
  };
}
