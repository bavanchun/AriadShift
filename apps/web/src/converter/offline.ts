// Registers the service worker that keeps /app usable offline after the first visit.
import { isOfflineAsset } from "../../offline-policy.mjs";

/**
 * Same-origin URLs the converter needs offline: the page, its assets, the worker and wasm.
 * Only the app shell and static assets qualify; account, API and document responses the
 * page also loaded never reach the cache.
 */
export function offlineUrls(
  pageUrl: string,
  pageResources: string[],
  workerResources: string[],
): string[] {
  const page = new URL(pageUrl);
  const urls = new Set<string>([`${page.origin}/app`]);
  for (const candidate of [...pageResources, ...workerResources]) {
    let url: URL;
    try {
      url = new URL(candidate, page);
    } catch {
      continue;
    }
    // Skip other sites, the service worker, page data, and every auth, API or document URL.
    if (!isOfflineAsset(url, page.origin)) {
      continue;
    }
    // Heavy binaries already live in the engine cache; keep only one disk copy.
    if (url.pathname.startsWith("/wasm/pandoc/") || /ariad_typst.*\.wasm$/.test(url.pathname))
      continue;
    url.hash = "";
    urls.add(url.href);
  }
  return [...urls].sort();
}

export async function enableOffline(workerResources: string[]): Promise<boolean> {
  if (!("serviceWorker" in navigator)) {
    return false;
  }
  try {
    await navigator.serviceWorker.register("/sw.js", { scope: "/" });
    const registration = await navigator.serviceWorker.ready;
    const pageResources = performance.getEntriesByType("resource").map((entry) => entry.name);
    registration.active?.postMessage({
      type: "cache",
      urls: offlineUrls(window.location.href, pageResources, workerResources),
      // Preserve lazy engine code when a replacement worker refreshes the app shell. The
      // worker lists the engine binaries it used, including those read from Cache Storage.
      engineChunks: workerResources.some((url) => /pandoc\.wasm|ariad_typst.*\.wasm/.test(url))
        ? offlineUrls(window.location.href, [], workerResources).filter((url) =>
            new URL(url).pathname.endsWith(".js"),
          )
        : [],
    });
    return true;
  } catch {
    return false;
  }
}
