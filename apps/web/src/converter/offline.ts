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
    if (url.origin !== page.origin || url.pathname === "/sw.js" || url.searchParams.has("_rsc")) {
      continue;
    }
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
    });
    return true;
  } catch {
    return false;
  }
}
