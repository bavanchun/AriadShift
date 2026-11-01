// The consent page script (pages/oauth.js) against a stub DOM. What it shows
// must come from the signed authorization parameters only, because those are
// the only ones the provider enforces.

import { runInNewContext } from "node:vm";
import { describe, expect, it } from "vitest";
import { ASSETS } from "../src/pages.ts";

interface StubElement {
  textContent: string;
  hidden: boolean;
  disabled: boolean;
  items: string[];
  classList: { toggle: () => void };
  addEventListener: () => void;
  append: (child: { textContent: string }) => void;
}

function stubElement(): StubElement {
  const element: StubElement = {
    textContent: "",
    hidden: false,
    disabled: false,
    items: [],
    classList: { toggle: () => {} },
    addEventListener: () => {},
    append: (child) => {
      element.items.push(child.textContent);
    },
  };
  return element;
}

const REDIRECT = "http://127.0.0.1:33418/callback";

/** A consent URL query: `extra` first, so a first-value lookup sees it. */
function consentSearch(signed: [string, string][], extra: [string, string][] = []): string {
  const query = new URLSearchParams();
  for (const [key, value] of [...extra, ...signed]) query.append(key, value);
  for (const key of new Set(signed.map(([key]) => key))) query.append("ba_param", key);
  query.append("sig", "signature");
  return `?${query}`;
}

function signed(redirect = REDIRECT): [string, string][] {
  return [
    ["response_type", "code"],
    ["client_id", "client-1"],
    ["redirect_uri", redirect],
    ["scope", "openid offline_access"],
    ["state", "s"],
  ];
}

async function renderConsent(search: string) {
  const elements = new Map<string, StubElement>();
  const byId = (id: string): StubElement => {
    let element = elements.get(id);
    if (!element) {
      element = stubElement();
      elements.set(id, element);
    }
    return element;
  };
  const answers: Record<string, unknown> = {
    "/api/auth/get-session": { user: { email: "victim@example.com" } },
    "/api/auth/oauth2/public-client": { client_name: "Registered client" },
  };
  runInNewContext((ASSETS["oauth.js"] as { body: string }).body, {
    window: {
      location: { search, href: `http://localhost:3001/oauth/consent${search}`, assign: () => {} },
    },
    document: {
      body: { dataset: { page: "consent" } },
      getElementById: byId,
      querySelectorAll: () => [byId("allow"), byId("deny")],
      createElement: () => ({ textContent: "" }),
    },
    fetch: async (input: string) => {
      const path = new URL(input, "http://localhost:3001").pathname;
      return { ok: true, status: 200, json: async () => answers[path] ?? {} };
    },
    URL,
    URLSearchParams,
  });
  // Let the page's fetch calls settle.
  for (let i = 0; i < 5; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  return byId;
}

describe("consent page", () => {
  it("shows the signed metadata client host", async () => {
    const values = signed().map(([key, value]): [string, string] => [
      key,
      key === "client_id" ? "https://client.example/metadata" : value,
    ]);
    const page = await renderConsent(consentSearch(values));
    expect(page("client-host").textContent).toBe("client.example");
  });
  it("shows an http redirect's origin and the signed scopes", async () => {
    const page = await renderConsent(consentSearch(signed()));
    expect(page("redirect").textContent).toBe("http://127.0.0.1:33418");
    expect(page("scopes").items).toContain(
      "Stay connected for up to 30 days without signing in again",
    );
    expect(page("client-name").textContent).toBe("Registered client");
  });

  it("ignores a resource the signed request does not carry", async () => {
    const page = await renderConsent(
      consentSearch(signed(), [["resource", "https://evil.example/mcp"]]),
    );
    expect(page("resources").textContent).toBe("AriadShift");
  });

  it("refuses a link that repeats a signed parameter", async () => {
    const page = await renderConsent(
      consentSearch(signed(), [["redirect_uri", "https://attacker.example/callback"]]),
    );
    expect(page("redirect").textContent).toBe("");
    expect(page("allow").disabled).toBe(true);
    expect(page("status").textContent).toMatch(/not valid/);
  });

  it("shows a native client's full redirect URI", async () => {
    const page = await renderConsent(consentSearch(signed("com.example.app:/oauth/callback")));
    expect(page("redirect").textContent).toBe("com.example.app:/oauth/callback");
  });

  it.each(["response_type", "client_id", "scope", "state", "sig"])(
    "refuses repeated singleton parameter %s",
    async (name) => {
      const page = await renderConsent(consentSearch(signed(), [[name, "altered"]]));
      expect(page("allow").disabled).toBe(true);
      expect(page("deny").disabled).toBe(true);
      expect(page("status").textContent).toMatch(/not valid/);
    },
  );

  it("keeps multiple signed resources", async () => {
    const page = await renderConsent(
      consentSearch([
        ...signed(),
        ["resource", "https://api.example.com/mcp"],
        ["resource", "https://second.example.com/mcp"],
      ]),
    );
    expect(page("resources").textContent).toBe(
      "https://api.example.com/mcp, https://second.example.com/mcp",
    );
    expect(page("allow").disabled).toBe(false);
  });

  it("refuses a link without a signature", async () => {
    const page = await renderConsent("?client_id=unsigned&scope=forged");
    expect(page("allow").disabled).toBe(true);
    expect(page("scopes").items).toEqual([]);
  });
});
