import { runInNewContext } from "node:vm";
import { offlineUrls } from "@/converter/offline";
import { serviceWorkerSource } from "../scripts/service-worker-source.mjs";

const ORIGIN = "https://site.test";
const APP_CACHE = "ariadshift-app-v1";

// Same-origin responses a signed-in page loads when web and auth share one origin.
const SENSITIVE = [
  `${ORIGIN}/api/auth/get-session`,
  `${ORIGIN}/api/auth/token`,
  `${ORIGIN}/v1/jobs/5f0e8d3a-2c4b-4e71-8b6a-9d1c3e2f4a57/outputs`,
  `${ORIGIN}/ariadshift/outputs/report.docx?X-Amz-Signature=abc&X-Amz-Expires=300`,
];
const SHELL = [`${ORIGIN}/_next/static/chunks/a.js`, `${ORIGIN}/app`];

type Key = string | { url: string };

function keyOf(request: Key): string {
  return typeof request === "string" ? new URL(request, ORIGIN).href : request.url;
}

class MemoryCache {
  readonly entries = new Map<string, Response>();

  async put(request: Key, response: Response) {
    this.entries.set(keyOf(request), response);
  }

  async match(request: Key) {
    return this.entries.get(keyOf(request));
  }

  async keys() {
    return [...this.entries.keys()].map((url) => ({ url }));
  }

  async delete(request: Key) {
    return this.entries.delete(keyOf(request));
  }
}

interface Dispatched {
  response?: Promise<Response>;
}

/** Runs the generated service worker against in-memory caches and a recording fetch. */
async function serviceWorker() {
  const stores = new Map<string, MemoryCache>();
  const open = (name: string) => {
    const store = stores.get(name) ?? new MemoryCache();
    stores.set(name, store);
    return store;
  };
  const fetched: string[] = [];
  const listeners = new Map<string, (event: object) => void>();
  runInNewContext(await serviceWorkerSource([]), {
    self: {
      location: new URL(`${ORIGIN}/sw.js`),
      addEventListener: (type: string, listener: (event: object) => void) =>
        listeners.set(type, listener),
      skipWaiting: () => undefined,
      clients: { claim: async () => undefined },
    },
    caches: {
      open: async (name: string) => open(name),
      keys: async () => [...stores.keys()],
      delete: async (name: string) => stores.delete(name),
    },
    fetch: async (input: Key) => {
      fetched.push(keyOf(input));
      return new Response("body", { headers: { "set-auth-jwt": "eyJhbGciOi" } });
    },
    URL,
    Response,
  });
  const dispatch = async (type: string, init: object): Promise<Dispatched> => {
    const pending: Promise<unknown>[] = [];
    const result: Dispatched = {};
    listeners.get(type)?.({
      ...init,
      waitUntil: (promise: Promise<unknown>) => pending.push(promise),
      respondWith: (response: Promise<Response>) => {
        result.response = response;
      },
    });
    await Promise.all(pending);
    return result;
  };
  return { open, fetched, dispatch };
}

describe("offline cache boundary", () => {
  it("collects only the app shell and static assets", () => {
    expect(
      offlineUrls(`${ORIGIN}/app`, [...SENSITIVE, ...SHELL, `${ORIGIN}/app?_rsc=1`], []),
    ).toEqual(SHELL);
  });

  it("never fetches or stores auth, API or document responses", async () => {
    const worker = await serviceWorker();
    await worker.dispatch("message", { data: { type: "cache", urls: [...SHELL, ...SENSITIVE] } });
    expect(worker.fetched).toEqual(SHELL);
    expect([...worker.open(APP_CACHE).entries.keys()].sort()).toEqual(SHELL);
  });

  it("removes sensitive entries an earlier worker stored when it activates", async () => {
    const worker = await serviceWorker();
    const cache = worker.open(APP_CACHE);
    for (const url of [...SHELL, ...SENSITIVE]) {
      await cache.put(url, new Response("cached"));
    }
    await worker.dispatch("activate", {});
    expect([...cache.entries.keys()].sort()).toEqual(SHELL);
  });

  it("leaves auth and API requests to the network", async () => {
    const worker = await serviceWorker();
    for (const url of SENSITIVE) {
      const { response } = await worker.dispatch("fetch", {
        request: { method: "GET", url, mode: "cors", destination: "" },
      });
      expect(response, url).toBeUndefined();
    }
    const { response } = await worker.dispatch("fetch", {
      request: { method: "GET", url: SHELL[0], mode: "no-cors", destination: "script" },
    });
    expect(response).toBeDefined();
  });
});
