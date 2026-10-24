// Registers the service worker that keeps /app usable offline after the first visit.

/** Same-origin URLs the converter needs offline: the page, its assets, the worker and wasm. */
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
    // Skip other sites, the service worker itself and prefetched data for other pages.
    if (
      !["http:", "https:"].includes(url.protocol) ||
      url.origin !== page.origin ||
      url.pathname === "/sw.js" ||
      url.searchParams.has("_rsc")
    ) {
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
      // Preserve lazy engine code when a replacement worker refreshes the app shell.
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
