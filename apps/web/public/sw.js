// Offline support for the in-browser converter. The converter page posts the URLs it
// and its worker loaded; they are cached here and served when the network is gone.
// Pages other than /app keep their normal network behaviour.
const CACHE = "ariadshift-app-v1";

self.addEventListener("install", () => {
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      for (const name of await caches.keys()) {
        if (name !== CACHE) {
          await caches.delete(name);
        }
      }
      await self.clients.claim();
    })(),
  );
});

async function cacheUrls(urls) {
  const cache = await caches.open(CACHE);
  const wanted = new Set();
  for (const url of urls) {
    if (new URL(url).origin !== self.location.origin) {
      continue;
    }
    wanted.add(url);
    try {
      const response = await fetch(url, { cache: "no-cache" });
      if (response.ok) {
        await cache.put(url, response);
      }
    } catch {
      // Offline or gone; keep any copy already cached.
    }
  }
  for (const request of await cache.keys()) {
    if (!wanted.has(request.url)) {
      await cache.delete(request);
    }
  }
}

self.addEventListener("message", (event) => {
  if (event.data?.type === "cache" && Array.isArray(event.data.urls)) {
    event.waitUntil(cacheUrls(event.data.urls));
  }
});

function isConverterPage(url) {
  return url.pathname === "/app" || url.pathname.startsWith("/app/");
}

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") {
    return;
  }
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) {
    return;
  }
  if (request.mode === "navigate") {
    if (!isConverterPage(url)) {
      return;
    }
    event.respondWith(
      (async () => {
        try {
          return await fetch(request);
        } catch (error) {
          const cache = await caches.open(CACHE);
          const cached =
            (await cache.match(request, { ignoreSearch: true })) ??
            (await cache.match(`${self.location.origin}/app`));
          if (cached) {
            return cached;
          }
          throw error;
        }
      })(),
    );
    return;
  }
  event.respondWith(
    (async () => {
      const cache = await caches.open(CACHE);
      const cached = await cache.match(request);
      if (!cached) {
        return fetch(request);
      }
      // A worker takes its location from the response URL, which for a cached entry has no
      // fragment; the bundler passes the worker's bootstrap parameters in the fragment. A
      // fresh Response has no URL, so the worker keeps the request URL with its fragment.
      if (request.destination === "worker") {
        return new Response(cached.body, {
          status: cached.status,
          statusText: cached.statusText,
          headers: cached.headers,
        });
      }
      return cached;
    })(),
  );
});
