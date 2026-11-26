// Service worker of the web app: without a connection the app still opens
// and shows the chats it loaded last, read-only.
//
// - Pages: from the network; without one, the page kept from last time (the
//   app renders every route itself).
// - /assets/*: hashed names never change, so from the cache once loaded.
//   Files that the current build no longer lists (asset-list.json) go.
// - Other files of the app (theme-init.js, icons, manifest): from the
//   network, the last copy without one.
// - A few reads of the API (chats, attachments, models, settings): from the
//   network, and the last answer kept for when the server is out of reach;
//   such an answer carries `x-hearth-offline: 1`.
// Everything else goes straight to the server: sign-in and the session
// (never kept here), every write, live events and streams.
const SHELL = 'hearth-shell-v1';
const ASSETS = 'hearth-assets-v1';
const DATA = 'hearth-data-v1';
const OURS = [SHELL, ASSETS, DATA];

const READS = [
  /^\/api\/conversations$/,
  /^\/api\/conversations\/(?!search$)[^/]+$/,
  /^\/api\/files\/[^/]+$/,
  /^\/api\/providers$/,
  /^\/api\/providers\/[^/]+\/models$/,
  /^\/api\/models$/,
  /^\/api\/settings$/,
  /^\/api\/features$/,
  /^\/api\/features\/default-model$/,
  /^\/api\/devices$/,
];

self.addEventListener('install', (event) => {
  event.waitUntil(
    (async () => {
      const page = await fetch('/', { cache: 'no-store' }).catch(() => null);
      if (page?.ok) {
        await (await caches.open(SHELL)).put('/', page);
      }
      await self.skipWaiting();
    })(),
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      for (const name of await caches.keys()) {
        if (name.startsWith('hearth-') && !OURS.includes(name)) {
          await caches.delete(name);
        }
      }
      await self.clients.claim();
    })(),
  );
});

self.addEventListener('fetch', (event) => {
  const request = event.request;
  if (request.method !== 'GET') {
    return;
  }
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) {
    return;
  }
  if (request.mode === 'navigate' && !url.pathname.startsWith('/api/')) {
    event.respondWith(page(request));
  } else if (url.pathname.startsWith('/assets/')) {
    event.respondWith(asset(request));
  } else if (!url.pathname.startsWith('/api/') && url.pathname !== '/asset-list.json') {
    event.respondWith(file(request));
  } else if (READS.some((pattern) => pattern.test(url.pathname)) && !request.headers.has('range')) {
    event.respondWith(read(request));
  }
});

/** The page from the network, kept; without a network, the kept one. */
async function page(request) {
  try {
    const response = await fetch(request);
    if (response.ok && (response.headers.get('content-type') ?? '').includes('text/html')) {
      const cache = await caches.open(SHELL);
      await cache.put('/', response.clone());
      // A fresh page may be a new build: drop what it no longer uses.
      void pruneAssets();
    }
    return response;
  } catch (error) {
    const kept = await (await caches.open(SHELL)).match('/');
    if (kept) {
      return kept;
    }
    throw error;
  }
}

/** Another file of the app: from the network and kept; the copy without one. */
async function file(request) {
  const cache = await caches.open(SHELL);
  try {
    const response = await fetch(request);
    if (response.ok) {
      await cache.put(request, response.clone());
    }
    return response;
  } catch (error) {
    const kept = await cache.match(request);
    if (kept) {
      return kept;
    }
    throw error;
  }
}

/** A hashed file: from the cache, else from the network and kept. */
async function asset(request) {
  const cache = await caches.open(ASSETS);
  const kept = await cache.match(request);
  if (kept) {
    return kept;
  }
  const response = await fetch(request);
  if (response.ok) {
    await cache.put(request, response.clone());
  }
  return response;
}

/** Removes kept files that the current build does not list. */
async function pruneAssets() {
  try {
    const response = await fetch('/asset-list.json', { cache: 'no-store' });
    if (!response.ok) {
      return;
    }
    const listed = new Set((await response.json()).map((file) => `/${file}`));
    const cache = await caches.open(ASSETS);
    for (const request of await cache.keys()) {
      if (!listed.has(new URL(request.url).pathname)) {
        await cache.delete(request);
      }
    }
  } catch {
    // Offline or no list (a build without it): keep everything for now.
  }
}

/**
 * A read of the API from the network, its answer kept; when the server is
 * out of reach, the kept answer, marked. Without one, the request fails as
 * it would without this worker.
 */
async function read(request) {
  const cache = await caches.open(DATA);
  try {
    const response = await fetch(request);
    if (response.ok) {
      await cache.put(request, response.clone());
    } else if (response.status === 401 || response.status === 404) {
      // Signed out, or gone: nothing to show later either.
      await cache.delete(request);
    }
    return response;
  } catch (error) {
    const kept = await cache.match(request);
    if (!kept) {
      throw error;
    }
    const headers = new Headers(kept.headers);
    headers.set('x-hearth-offline', '1');
    return new Response(kept.body, { status: kept.status, statusText: kept.statusText, headers });
  }
}
