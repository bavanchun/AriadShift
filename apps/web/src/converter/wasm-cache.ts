/** Lazy engine assets have their own versioned cache, independent of the app shell. */
export const ENGINE_CACHE = "ariadshift-engines-v1";

function progress(message: string) {
  if (typeof self !== "undefined" && typeof document === "undefined")
    self.postMessage({ event: "progress", message });
}

export async function engineResponse(url: string): Promise<Response> {
  const label = url.includes("pandoc") ? "Pandoc" : "Typst";
  progress(`Loading ${label} on this device…`);
  let cache: Cache | undefined;
  try {
    if (typeof caches !== "undefined") {
      cache = await caches.open(ENGINE_CACHE);
      const response = await cache.match(url);
      if (response) return response;
    }
  } catch {
    /* Private browsing can deny Cache Storage; conversion still works online. */
  }
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
  progress(`Preparing ${label} on this device…`);
  try {
    await cache?.put(url, response.clone());
  } catch {
    /* Storage full; use this download. */
  }
  return response;
}
