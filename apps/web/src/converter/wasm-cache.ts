/** Lazy engine assets have their own versioned cache, independent of the app shell. */
export const ENGINE_CACHE = "ariadshift-engines-v1";

/**
 * Engine binaries this worker has loaded or found in the engine cache. Cache Storage
 * reads leave no Resource Timing entry, so the worker reports these URLs itself; the page
 * uses them to keep the worker's lazy engine chunks in the long-lived engine cache.
 */
export const usedEngines = new Set<string>();

function progress(message: string) {
  if (typeof self !== "undefined" && typeof document === "undefined")
    self.postMessage({ event: "progress", message });
}

async function openEngineCache(): Promise<Cache | undefined> {
  try {
    if (typeof caches !== "undefined") return await caches.open(ENGINE_CACHE);
  } catch {
    /* Private browsing can deny Cache Storage; conversion still works online. */
  }
  return undefined;
}

/** The routes an engine serves, as the converter names them to the user. */
function routeName(url: string) {
  return url.includes("pandoc") ? "DOCX/EPUB" : "PDF";
}

/** A pinned engine binary that is damaged or does not run; the conversion stops here. */
function integrityError(url: string, reason: string) {
  return {
    code: "engine_integrity",
    message: `The ${routeName(url)} engine ${reason}. Reload the page to retry.`,
  };
}

async function sha256Hex(bytes: ArrayBuffer): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return Array.from(digest, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

/** Removes a cached engine binary so the next attempt downloads a fresh copy. */
export async function evictEngine(url: string): Promise<void> {
  try {
    await (await openEngineCache())?.delete(url);
  } catch {
    /* Nothing to remove. */
  }
}

async function download(url: string, label: string): Promise<Response> {
  let response: Response;
  try {
    response = await fetch(url);
  } catch {
    throw {
      code: "engine_unavailable",
      message:
        "This engine is not cached yet. Connect to the internet to download it, or open this route in the desktop app.",
    };
  }
  if (!response.ok)
    throw {
      code: "engine_unavailable",
      message:
        "The conversion engine could not be downloaded. Connect to the internet and try again.",
    };
  if (response.body) {
    const reader = response.body.getReader();
    const total = response.headers.get("content-encoding")
      ? 0
      : Number(response.headers.get("content-length"));
    const chunks: Uint8Array<ArrayBuffer>[] = [];
    let loaded = 0;
    let last = -1;
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      chunks.push(new Uint8Array(value));
      loaded += value.length;
      const step =
        total > 0
          ? Math.min(100, Math.floor((loaded / total) * 10) * 10)
          : Math.floor(loaded / (5 * 1024 * 1024)) * 5;
      if (step !== last) {
        progress(`Downloading ${label}: ${step}${total > 0 ? "%" : " MB"}`);
        last = step;
      }
    }
    response = new Response(new Blob(chunks), { headers: response.headers });
  }
  return response;
}

/**
 * Returns an engine binary from the engine cache, or downloads and caches it. With a
 * `sha256` digest it fails closed: a cached copy that does not match is evicted, and a
 * download that does not match is never cached. Either way the conversion stops with an
 * integrity error instead of running an unverified engine; the next attempt downloads anew.
 */
export async function engineResponse(url: string, sha256?: string): Promise<Response> {
  const label = url.includes("pandoc") ? "Pandoc" : "Typst";
  progress(`Loading ${label} on this device…`);
  const cache = await openEngineCache();
  let cached: Response | undefined;
  try {
    cached = await cache?.match(url);
  } catch {
    /* Treat an unreadable cache as empty. */
  }
  if (cached) {
    if (!sha256) {
      usedEngines.add(url);
      return cached;
    }
    const bytes = await cached.arrayBuffer();
    if ((await sha256Hex(bytes)) !== sha256) {
      await evictEngine(url);
      throw integrityError(url, "failed an integrity check");
    }
    usedEngines.add(url);
    return new Response(bytes, { headers: cached.headers });
  }
  let response = await download(url, label);
  if (sha256) {
    const bytes = await response.arrayBuffer();
    if ((await sha256Hex(bytes)) !== sha256) throw integrityError(url, "failed an integrity check");
    response = new Response(bytes, { headers: response.headers });
  }
  progress(`Preparing ${label} on this device…`);
  try {
    await cache?.put(url, response.clone());
  } catch {
    /* Storage full; use this download. */
  }
  usedEngines.add(url);
  return response;
}

/** Verifies and compiles an engine binary, evicting the cached copy if it does not compile. */
export async function compileEngine(url: string, sha256: string): Promise<WebAssembly.Module> {
  const response = await engineResponse(url, sha256);
  try {
    return await WebAssembly.compile(await response.arrayBuffer());
  } catch {
    await evictEngine(url);
    usedEngines.delete(url);
    throw integrityError(url, "could not be loaded");
  }
}
