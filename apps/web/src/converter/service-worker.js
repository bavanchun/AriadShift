// Offline support for the in-browser converter. The converter page posts the URLs it
// and its worker loaded; they are cached here and served when the network is gone.
// The worker is registered for the whole origin so that the converter's own worker
// script falls under it, but it only answers /app navigations; content-hashed assets
// come from the cache first and everything else from the network first.
const CACHE = "ariadshift-app-v1";

self.addEventListener("install", () => {
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      for (const name of await caches.keys()) {
        if (name !== CACHE && name !== "ariadshift-engines-v1") {
          await caches.delete(name);
        }
      }
      // The generated script embeds this build's versioned binaries and hashed chunks.
      // Its bytes change with the manifest, so deployments activate a new worker.
      if (self.ENGINE_URLS?.length) {
        const wanted = new Set(self.ENGINE_URLS.map((url) => new URL(url, self.location).href));
        const engines = await caches.open("ariadshift-engines-v1");
        for (const request of await engines.keys()) {
          if (!wanted.has(request.url)) await engines.delete(request);
        }
      }
      await self.clients.claim();
    })(),
  );
});

async function cacheUrls(urls, engineChunks = []) {
  const cache = await caches.open(CACHE);
  const engines = await caches.open("ariadshift-engines-v1");
  const lazy = new Set(engineChunks);
  const wanted = new Set();
  for (const url of urls) {
    if (new URL(url).origin !== self.location.origin) {
      continue;
    }
    wanted.add(url);
    try {
      const response = await fetch(url, { cache: "no-cache" });
      if (response.ok) {
        if (lazy.has(url)) await engines.put(url, response.clone());
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
    event.waitUntil(
      cacheUrls(
        event.data.urls,
        Array.isArray(event.data.engineChunks) ? event.data.engineChunks : [],
      ),
    );
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
      let response;
      if (url.pathname.startsWith("/_next/static/")) {
        response =
          (await cache.match(request)) ??
          (await (await caches.open("ariadshift-engines-v1")).match(request)) ??
          (await fetch(request));
      } else {
        try {
          return await fetch(request);
        } catch (error) {
          response = await cache.match(request);
          if (!response) {
            throw error;
          }
        }
      }
      // A worker takes its location from the response URL, which for a cached entry has no
      // fragment; the bundler passes the worker's bootstrap parameters in the fragment. A
      // fresh Response has no URL, so the worker keeps the request URL with its fragment.
      if (request.destination === "worker") {
        return new Response(response.body, {
          status: response.status,
          statusText: response.statusText,
          headers: response.headers,
        });
      }
      return response;
    })(),
  );
});
