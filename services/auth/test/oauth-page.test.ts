// The consent page script (pages/oauth.js) against a stub DOM. What it shows
// must come from the signed authorization parameters only, because those are
// the only ones the provider enforces.

import { runInNewContext } from "node:vm";
import { describe, expect, it } from "vitest";
import { parseConfig } from "../src/config.ts";
import { ASSETS, PAGE_HEADERS, pageHeaders, pageOptions, signInPage } from "../src/pages.ts";

interface StubElement {
  textContent: string;
  hidden: boolean;
  disabled: boolean;
  items: string[];
  dataset: Record<string, string>;
  classList: { toggle: () => void };
  addEventListener: (
    type: string,
    handler: (event: { preventDefault: () => void }) => void,
  ) => void;
  append: (child: { textContent?: string }) => void;
  querySelector: () => { focus: () => void };
  trigger: (type: string) => void;
}

function stubElement(): StubElement {
  const listeners = new Map<string, Array<(event: { preventDefault: () => void }) => void>>();
  const element: StubElement = {
    textContent: "",
    hidden: false,
    disabled: false,
    items: [],
    dataset: {},
    classList: { toggle: () => {} },
    addEventListener: (type, handler) => {
      const list = listeners.get(type) ?? [];
      list.push(handler);
      listeners.set(type, list);
    },
    append: (child) => {
      element.items.push(child.textContent ?? "");
    },
    querySelector: () => ({ focus: () => {} }),
    trigger: (type) => {
      for (const handler of listeners.get(type) ?? []) {
        handler({ preventDefault: () => {} });
      }
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

interface TurnstileStubOptions {
  mode?: "pass" | "error" | "expired" | "pending";
  token?: string;
}

async function renderSignIn(
  search: string,
  options: {
    allowSignUp?: boolean;
    turnstileSiteKey?: string;
    turnstile?: TurnstileStubOptions;
    signUpResponse?: { ok?: boolean; status?: number; body?: Record<string, unknown> };
    continueResponse?: { ok?: boolean; status?: number; body?: Record<string, unknown> };
    signInResponse?: { ok?: boolean; status?: number; body?: Record<string, unknown> };
    formValues?: Record<string, string>;
  } = {},
) {
  const allowSignUp = options.allowSignUp ?? true;
  const turnstileSiteKey = options.turnstileSiteKey ?? "1x00000000000000000000AA";
  const elements = new Map<string, StubElement>();
  const byId = (id: string): StubElement => {
    let element = elements.get(id);
    if (!element) {
      element = stubElement();
      elements.set(id, element);
    }
    return element;
  };
  byId("password-form").hidden = false;
  if (allowSignUp) {
    byId("sign-up-form").hidden = true;
    byId("turnstile").hidden = true;
    byId("create-account");
    byId("sign-up-submit");
  }
  byId("two-factor-form").hidden = true;

  const calls: Array<{ url: string; init: RequestInit }> = [];
  const assigned: string[] = [];
  const renderedWidgets: Array<Record<string, unknown>> = [];
  let removedWidgets = 0;

  const turnstileConfig = options.turnstile ?? { mode: "pass", token: "turnstile-token-ok" };
  const turnstile = {
    render: (_container: unknown, widgetOptions: Record<string, unknown>) => {
      renderedWidgets.push(widgetOptions);
      if (turnstileConfig.mode === "error") {
        (widgetOptions["error-callback"] as (() => void) | undefined)?.();
      } else if (turnstileConfig.mode === "expired") {
        (widgetOptions["expired-callback"] as (() => void) | undefined)?.();
      } else if (turnstileConfig.mode !== "pending") {
        (widgetOptions.callback as ((token: string) => void) | undefined)?.(
          turnstileConfig.token ?? "turnstile-token-ok",
        );
      }
      return "widget-1";
    },
    remove: () => {
      removedWidgets++;
    },
  };

  const defaultConsentSearch = consentSearch(signed());
  const signUpReply = options.signUpResponse ?? {
    ok: true,
    status: 200,
    body: { user: { id: "user-1", email: "ada@example.com" } },
  };
  const continueReply = options.continueResponse ?? {
    ok: true,
    status: 200,
    body: {
      redirect: true,
      url: `http://localhost:3001/oauth/consent${defaultConsentSearch}`,
    },
  };
  const signInReply = options.signInResponse ?? {
    ok: true,
    status: 200,
    body: {
      redirect: true,
      url: `http://localhost:3001/oauth/consent${defaultConsentSearch}`,
    },
  };
  const values: Record<string, string> = {
    name: "Ada Lovelace",
    email: "ada@example.com",
    password: "correct horse battery staple",
    ...options.formValues,
  };

  class StubFormData {
    get(key: string) {
      return values[key] ?? null;
    }
  }

  runInNewContext((ASSETS["oauth.js"] as { body: string }).body, {
    window: {
      location: {
        search,
        href: `http://localhost:3001/oauth/sign-in${search}`,
        pathname: "/oauth/sign-in",
        assign: (url: string) => {
          assigned.push(url);
        },
      },
      turnstile,
    },
    document: {
      body: {
        dataset: {
          page: "sign-in",
          ...(allowSignUp && turnstileSiteKey ? { turnstileSiteKey } : {}),
        },
        append: () => {},
      },
      head: { append: () => {} },
      getElementById: (id: string) => {
        if (
          !allowSignUp &&
          (id === "sign-up-form" ||
            id === "create-account" ||
            id === "sign-up-submit" ||
            id === "turnstile")
        ) {
          return null;
        }
        return byId(id);
      },
      querySelectorAll: (selector: string) => {
        if (selector === "button") {
          return allowSignUp ? [byId("sign-up-submit"), byId("create-account")] : [];
        }
        return [];
      },
      createElement: () => ({ textContent: "" }),
    },
    FormData: StubFormData,
    fetch: async (input: string, init: RequestInit = {}) => {
      calls.push({ url: input, init });
      if (input === "/api/auth/oauth2/continue") {
        return {
          ok: continueReply.ok ?? true,
          status: continueReply.status ?? 200,
          json: async () => continueReply.body ?? {},
        };
      }
      if (input === "/api/auth/sign-in/email") {
        return {
          ok: signInReply.ok ?? true,
          status: signInReply.status ?? 200,
          json: async () => signInReply.body ?? {},
        };
      }
      return {
        ok: signUpReply.ok ?? true,
        status: signUpReply.status ?? 200,
        json: async () => signUpReply.body ?? {},
      };
    },
    URL,
    URLSearchParams,
  });

  const flush = async () => {
    for (let i = 0; i < 6; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  };
  await flush();
  return { byId, calls, assigned, renderedWidgets, removedWidgets: () => removedWidgets, flush };
}

describe("sign-in page sign-up", () => {
  it("renders the create-account toggle, sign-up form, and Turnstile CSP when sign-up is enabled", () => {
    const html = signInPage({
      socialProviders: [],
      passkeys: false,
      allowSignUp: true,
      turnstileSiteKey: "1x00000000000000000000AA",
    });
    expect(html).toContain('id="create-account"');
    expect(html).toContain('id="sign-up-form"');
    expect(html).toContain('id="turnstile"');
    expect(html).toContain('data-turnstile-site-key="1x00000000000000000000AA"');
    expect(pageHeaders({ socialProviders: [], passkeys: false })).toEqual(PAGE_HEADERS);
    const csp =
      pageHeaders({
        socialProviders: [],
        passkeys: false,
        allowSignUp: true,
        turnstileSiteKey: "1x00000000000000000000AA",
      })["Content-Security-Policy"] ?? "";
    expect(csp).toContain("script-src 'self' https://challenges.cloudflare.com");
    expect(csp).toContain("frame-src https://challenges.cloudflare.com");
  });

  it("hides Create an account, the sign-up form, and Turnstile CSP when AUTH_ALLOW_SIGN_UP=false", () => {
    const config = parseConfig({
      DATABASE_URL: "postgres://ariad_auth:pw@postgres:5432/ariadshift",
      BETTER_AUTH_SECRET: "0123456789abcdef0123456789abcdef",
      BETTER_AUTH_URL: "https://auth.example.com",
      BETTER_AUTH_TRUSTED_ORIGINS: "https://app.example.com",
      ARIAD_PUBLIC_URL: "https://api.example.com",
      AUTH_ALLOW_SIGN_UP: "false",
      AUTH_TURNSTILE_SECRET: "prod-turnstile-secret",
      AUTH_TURNSTILE_SITE_KEY: "0x4AAAAAAABBBBBBBBBBBBBB",
    });
    const options = pageOptions(config);
    expect(options.allowSignUp).toBe(false);
    expect(options.turnstileSiteKey).toBeUndefined();
    const html = signInPage(options);
    expect(html).not.toContain('id="create-account"');
    expect(html).not.toContain('id="sign-up-form"');
    expect(html).not.toContain("data-turnstile-site-key");
    expect(pageHeaders(options)).toEqual(PAGE_HEADERS);
  });

  it("never falls back to the Cloudflare test site key in a production config", () => {
    const prodWithTurnstile = parseConfig({
      DATABASE_URL: "postgres://ariad_auth:pw@postgres:5432/ariadshift",
      BETTER_AUTH_SECRET: "0123456789abcdef0123456789abcdef",
      BETTER_AUTH_URL: "https://auth.example.com",
      BETTER_AUTH_TRUSTED_ORIGINS: "https://app.example.com",
      ARIAD_PUBLIC_URL: "https://api.example.com",
      AUTH_ALLOW_SIGN_UP: "true",
      AUTH_TURNSTILE_SECRET: "prod-turnstile-secret",
      AUTH_TURNSTILE_SITE_KEY: "0x4AAAAAAACCCCCCCCCCCCCC",
    });
    const htmlWithTurnstile = signInPage(pageOptions(prodWithTurnstile));
    expect(htmlWithTurnstile).toContain('data-turnstile-site-key="0x4AAAAAAACCCCCCCCCCCCCC"');
    expect(htmlWithTurnstile).not.toContain("1x00000000000000000000AA");

    const prodWithoutTurnstile = parseConfig({
      DATABASE_URL: "postgres://ariad_auth:pw@postgres:5432/ariadshift",
      BETTER_AUTH_SECRET: "0123456789abcdef0123456789abcdef",
      BETTER_AUTH_URL: "https://auth.example.com",
      BETTER_AUTH_TRUSTED_ORIGINS: "https://app.example.com",
      ARIAD_PUBLIC_URL: "https://api.example.com",
      AUTH_ALLOW_SIGN_UP: "true",
    });
    const htmlWithoutTurnstile = signInPage(pageOptions(prodWithoutTurnstile));
    expect(htmlWithoutTurnstile).not.toContain("data-turnstile-site-key");
    expect(htmlWithoutTurnstile).not.toContain("1x00000000000000000000AA");
  });

  it("creates an account with a stubbed bot-check widget and continues via /oauth2/continue", async () => {
    const authParams: [string, string][] = [
      ...signed(),
      ["code_challenge", "challenge-xyz"],
      ["code_challenge_method", "S256"],
      ["resource", "http://localhost:8080/mcp"],
    ];
    const search = consentSearch(authParams);
    const { byId, calls, assigned, renderedWidgets, flush } = await renderSignIn(search, {
      turnstile: { mode: "pass", token: "turnstile-proof-ok" },
      continueResponse: {
        ok: true,
        status: 200,
        body: {
          redirect: true,
          url: `http://localhost:3001/oauth/consent${search}`,
        },
      },
    });

    byId("create-account").trigger("click");
    await flush();

    expect(byId("password-form").hidden).toBe(true);
    expect(byId("sign-up-form").hidden).toBe(false);
    expect(renderedWidgets).toHaveLength(1);
    expect(renderedWidgets[0]?.action).toBe("signup");
    expect(renderedWidgets[0]?.sitekey).toBe("1x00000000000000000000AA");
    expect(byId("sign-up-submit").disabled).toBe(false);

    byId("sign-up-form").trigger("submit");
    await flush();

    expect(calls).toHaveLength(2);
    expect(calls[0]?.url).toBe("/api/auth/sign-up/email");
    const headers = calls[0]?.init.headers as Record<string, string>;
    expect(headers["x-captcha-response"]).toBe("turnstile-proof-ok");
    const sent = JSON.parse(String(calls[0]?.init.body));
    expect(sent.name).toBe("Ada Lovelace");
    expect(sent.email).toBe("ada@example.com");
    expect(sent.password).toBe("correct horse battery staple");
    const forwardedQuery = new URLSearchParams(sent.oauth_query);
    expect(forwardedQuery.get("client_id")).toBe("client-1");
    expect(forwardedQuery.get("redirect_uri")).toBe(REDIRECT);
    expect(forwardedQuery.get("scope")).toBe("openid offline_access");
    expect(forwardedQuery.get("state")).toBe("s");
    expect(forwardedQuery.get("code_challenge")).toBe("challenge-xyz");
    expect(forwardedQuery.get("code_challenge_method")).toBe("S256");
    expect(forwardedQuery.getAll("resource")).toEqual(["http://localhost:8080/mcp"]);

    expect(calls[1]?.url).toBe("/api/auth/oauth2/continue");
    const continueSent = JSON.parse(String(calls[1]?.init.body));
    expect(continueSent.created).toBe(true);
    expect(continueSent.oauth_query).toBe(sent.oauth_query);
    expect(assigned).toEqual([`http://localhost:3001/oauth/consent${search}`]);
    expect(renderedWidgets).toHaveLength(1);
  });

  it("opens the sign-up form automatically on prompt=create and continues to consent", async () => {
    const createSearch = consentSearch([...signed(), ["prompt", "create"]]);
    const consentTarget = `http://localhost:3001/oauth/consent${consentSearch(signed())}`;
    const { byId, calls, assigned, renderedWidgets, flush } = await renderSignIn(createSearch, {
      turnstile: { mode: "pass", token: "turnstile-proof-create" },
      continueResponse: {
        ok: true,
        status: 200,
        body: { redirect: true, url: consentTarget },
      },
    });

    expect(byId("password-form").hidden).toBe(true);
    expect(byId("sign-up-form").hidden).toBe(false);
    expect(renderedWidgets).toHaveLength(1);

    byId("sign-up-form").trigger("submit");
    await flush();

    expect(calls.map((c) => c.url)).toEqual([
      "/api/auth/sign-up/email",
      "/api/auth/oauth2/continue",
    ]);
    expect(JSON.parse(String(calls[1]?.init.body)).created).toBe(true);
    expect(assigned).toEqual([consentTarget]);
  });

  it("keeps plain password sign-in working under the stricter page checks", async () => {
    const search = consentSearch([
      ...signed(),
      ["code_challenge", "challenge-xyz"],
      ["code_challenge_method", "S256"],
    ]);
    const consentTarget = `http://localhost:3001/oauth/consent${search}`;
    const { byId, calls, assigned, flush } = await renderSignIn(search, {
      allowSignUp: false,
      signInResponse: {
        ok: true,
        status: 200,
        body: { redirect: true, url: consentTarget },
      },
    });

    byId("password-form").trigger("submit");
    await flush();

    expect(calls).toHaveLength(1);
    expect(calls[0]?.url).toBe("/api/auth/sign-in/email");
    const sent = JSON.parse(String(calls[0]?.init.body));
    expect(sent.email).toBe("ada@example.com");
    expect(sent.password).toBe("correct horse battery staple");
    expect(new URLSearchParams(sent.oauth_query).get("client_id")).toBe("client-1");
    expect(assigned).toEqual([consentTarget]);
    expect(byId("status").textContent).toBe("Signed in. Continuing…");
  });

  it.each([
    [
      "repeated redirect_uri",
      consentSearch(signed(), [["redirect_uri", "https://evil.example/cb"]]),
    ],
    ["free-form return_to", consentSearch(signed(), [["return_to", "https://evil.example/steal"]])],
    ["free-form callbackURL", consentSearch(signed(), [["callbackURL", "https://evil.example"]])],
    ["signed custom next", consentSearch([...signed(), ["next", "custom-value"]])],
    ["signed custom url", consentSearch([...signed(), ["url", "custom-value"]])],
    [
      "unsigned state",
      consentSearch(
        signed().filter(([k]) => k !== "state"),
        [["state", "forged"]],
      ),
    ],
    ["missing signature", "?client_id=client-1&redirect_uri=http://127.0.0.1:33418/callback"],
  ])(
    "refuses tampered sign-in parameter (%s) before calling sign-up",
    async (_label, tamperedSearch) => {
      const { byId, calls, assigned, flush } = await renderSignIn(tamperedSearch);
      byId("create-account").trigger("click");
      await flush();
      expect(byId("sign-up-submit").disabled).toBe(true);
      expect(byId("status").textContent).toMatch(/not valid/);

      byId("sign-up-form").trigger("submit");
      await flush();
      expect(calls).toHaveLength(0);
      expect(assigned).toHaveLength(0);
    },
  );

  it("reports an error when /oauth2/continue does not return a safe redirect", async () => {
    const search = consentSearch(signed());
    const { byId, calls, assigned, renderedWidgets, flush } = await renderSignIn(search, {
      continueResponse: {
        ok: true,
        status: 200,
        body: { redirect: false },
      },
    });
    byId("create-account").trigger("click");
    await flush();
    byId("sign-up-form").trigger("submit");
    await flush();
    expect(calls).toHaveLength(2);
    expect(assigned).toHaveLength(0);
    expect(renderedWidgets).toHaveLength(2);
    expect(byId("status").textContent).toMatch(/could not continue/);
  });

  it.each(["error", "expired"] as const)(
    "blocks sign-up when the stubbed bot-check widget reports %s",
    async (mode) => {
      const search = consentSearch(signed());
      const { byId, calls, assigned, flush } = await renderSignIn(search, {
        turnstile: { mode },
      });
      byId("create-account").trigger("click");
      await flush();
      expect(byId("sign-up-submit").disabled).toBe(true);
      expect(byId("status").textContent).toMatch(/Bot check/);

      byId("sign-up-form").trigger("submit");
      await flush();
      expect(calls).toHaveLength(0);
      expect(assigned).toHaveLength(0);
    },
  );
});
