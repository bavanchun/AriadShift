// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import CloudPanel, { type CloudPanelProps } from "@/components/cloud/cloud-panel";
import type { PlanLimits } from "@/lib/cloud/api";
import type { DiscardOutcome } from "@/lib/cloud/cloud-job";
import type { CloudConfig } from "@/lib/cloud/config";

const CONFIG: CloudConfig = {
  apiUrl: "https://api.example.com",
  apiHost: "api.example.com",
  authUrl: "https://auth.example.com",
  turnstileSiteKey: null,
  operator: null,
};

function jwtFor(sub: string, tag = "1"): string {
  const encode = (value: unknown) => Buffer.from(JSON.stringify(value)).toString("base64url");
  return `${encode({ alg: "EdDSA", typ: "JWT" })}.${encode({ sub, jti: tag })}.sig`;
}

const CAPABILITIES = {
  version: "ariad-capabilities/0",
  generated_at: "2026-10-25T11:00:00Z",
  packs: ["core"],
  formats: [
    { id: "markdown", extensions: ["md"], media_type: "text/markdown" },
    { id: "html", extensions: ["html"], media_type: "text/html" },
    { id: "docx", extensions: ["docx"], media_type: "application/octet-stream" },
  ],
  edges: [
    { from: "markdown", to: "ariad-ir+json", engine: "ariad-core", pack: "core", license: "MIT" },
    { from: "ariad-ir+json", to: "html", engine: "ariad-core", pack: "core", license: "MIT" },
    { from: "ariad-ir+json", to: "docx", engine: "pandoc", pack: "core", license: "GPL" },
  ],
};

function limits(patch: Partial<PlanLimits["quotas"]["jobs_per_day"]> = {}): PlanLimits {
  const quota = { limit: 20, remaining: 20, used: 0 };
  return {
    plan: "anonymous",
    authenticated: false,
    retention_s: 3600,
    limits: { max_input_bytes: 10 * 1024 * 1024 },
    quotas: {
      uploads_per_day: quota,
      jobs_per_day: { ...quota, ...patch },
      input_bytes_per_day: { limit: null, remaining: null, used: 0 },
      pending_uploads: quota,
      concurrent_jobs: quota,
    },
  };
}

function server(options: { session?: unknown } = {}) {
  const requests: string[] = [];
  const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init);
    const url = new URL(request.url);
    requests.push(`${request.method} ${url.pathname}`);
    if (url.pathname === "/api/auth/get-session") {
      return Response.json(options.session ?? null);
    }
    if (url.pathname === "/v1/capabilities") {
      return Response.json(CAPABILITIES);
    }
    return Response.json({ error: { code: "not_found", message: "no" } }, { status: 404 });
  }) as typeof fetch;
  return { fetch: fetchImpl, requests };
}

function renderPanel(props: Partial<CloudPanelProps> & { limits?: PlanLimits | null } = {}) {
  const { limits: planLimits, ...rest } = props;
  const backend = server();
  const operations =
    planLimits === null
      ? {}
      : {
          getLimits: async () => ({
            data: planLimits ?? limits(),
            response: new Response(null, { status: 200 }),
          }),
        };
  const cleanup = { current: null as (() => Promise<DiscardOutcome>) | null };
  const onUseDevice = vi.fn();
  render(
    <CloudPanel
      config={CONFIG}
      file={new File(["# Hi\n"], "notes.md", { type: "text/markdown" })}
      sourceFormat="markdown"
      preferredTarget="docx"
      profile="editable"
      onUseDevice={onUseDevice}
      cleanup={cleanup}
      onServerCopy={vi.fn()}
      apiOptions={{ fetch: backend.fetch, operations }}
      {...rest}
    />,
  );
  return { backend, cleanup, onUseDevice };
}

describe("CloudPanel", () => {
  afterEach(() => {
    delete window.turnstile;
  });

  it("names the destination, retention and limits before anything leaves the device", async () => {
    const { backend, cleanup } = renderPanel();
    const disclosure = await screen.findByTestId("cloud-disclosure");
    await waitFor(() => expect(disclosure.textContent).toContain("1 hour"));
    expect(disclosure.textContent).toContain(
      "This file will be uploaded to api.example.com for conversion.",
    );
    expect(disclosure.textContent).toContain("never logged");
    expect(screen.getByTestId("cloud-limits").textContent).toBe(
      "Without an account: 20 cloud conversions a day, files up to 10 MB. 20 of 20 left today.",
    );
    expect(screen.getByLabelText("Target format for notes.md")).toHaveProperty("value", "docx");
    const start = screen.getByRole("button", { name: "Upload and convert" });
    await waitFor(() => expect(start.getAttribute("aria-disabled")).toBeNull());
    expect(backend.requests.some((request) => request.includes("/v1/uploads"))).toBe(false);
    expect(cleanup.current).toBeTypeOf("function");
    expect(screen.getByRole("button", { name: "Sign in" })).toBeDefined();
  });

  it("works without the limits endpoint", async () => {
    renderPanel({ limits: null });
    expect((await screen.findByTestId("cloud-limits")).textContent).toBe(
      "The server's plan limits apply; a file it cannot take is refused.",
    );
    expect(screen.getByTestId("cloud-disclosure").textContent).toContain(
      "when the server's retention period ends",
    );
  });

  it("refuses an exhausted quota before uploading and offers the device", async () => {
    const { onUseDevice } = renderPanel({ limits: limits({ remaining: 0 }) });
    const refusal = await screen.findByTestId("cloud-refusal");
    expect(refusal.textContent).toContain("You have used today's 20 cloud conversions.");
    const start = screen.getByRole("button", { name: "Upload and convert" });
    expect(start.getAttribute("aria-disabled")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "Convert on this device" }));
    expect(onUseDevice).toHaveBeenCalled();
  });

  it("refuses a file larger than the plan accepts", async () => {
    renderPanel({
      file: new File([new Uint8Array(11 * 1024 * 1024)], "big.md", { type: "text/markdown" }),
    });
    expect((await screen.findByTestId("cloud-refusal")).textContent).toContain(
      "This file is 11 MB. The anonymous plan accepts files up to 10 MB.",
    );
  });

  it("asks for Turnstile only when a site key is configured", async () => {
    const renderWidget = vi.fn(
      (_element: HTMLElement, options: { callback: (token: string) => void }) => {
        options.callback("token-1");
        return "widget-1";
      },
    );
    window.turnstile = { render: renderWidget, reset: vi.fn(), remove: vi.fn() };
    renderPanel({ config: { ...CONFIG, turnstileSiteKey: "site-key" } });
    await screen.findByTestId("turnstile");
    await waitFor(() => expect(renderWidget).toHaveBeenCalled());
    const start = screen.getByRole("button", { name: "Upload and convert" });
    await waitFor(() => expect(start.getAttribute("aria-disabled")).toBeNull());
  });

  it("opens the sign-in dialog", async () => {
    renderPanel();
    fireEvent.click(await screen.findByRole("button", { name: "Sign in" }));
    const dialog = screen.getByTestId("account-dialog");
    expect(dialog.hasAttribute("open")).toBe(true);
    expect(screen.getByLabelText("Email")).toBeDefined();
    expect(screen.getByLabelText("Password")).toBeDefined();
  });

  it("keeps anonymous cloud conversion working when the auth service is unreachable or 5xx", async () => {
    for (const authFailure of ["network", "503"] as const) {
      const requests: string[] = [];
      const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
        const request = input instanceof Request ? input : new Request(input, init);
        const url = new URL(request.url);
        requests.push(`${request.method} ${url.pathname}`);
        if (url.pathname.startsWith("/api/auth/")) {
          if (authFailure === "network") {
            throw new TypeError("Failed to fetch");
          }
          return new Response("unavailable", { status: 503 });
        }
        if (url.pathname === "/v1/capabilities") {
          return Response.json(CAPABILITIES);
        }
        return Response.json({ error: { code: "not_found", message: "no" } }, { status: 404 });
      }) as typeof fetch;

      const { unmount } = render(
        <CloudPanel
          config={CONFIG}
          file={new File(["# Hi\n"], "notes.md", { type: "text/markdown" })}
          sourceFormat="markdown"
          preferredTarget="docx"
          profile="editable"
          onUseDevice={vi.fn()}
          cleanup={{ current: null }}
          onServerCopy={vi.fn()}
          apiOptions={{
            fetch: fetchImpl,
            operations: {
              getLimits: async () => ({
                data: limits(),
                response: new Response(null, { status: 200 }),
              }),
            },
          }}
        />,
      );

      const start = await screen.findByRole("button", { name: "Upload and convert" });
      await waitFor(() => expect(start.getAttribute("aria-disabled")).toBeNull());
      expect(requests).not.toContain("GET /api/auth/token");
      unmount();
    }
  });

  it("fails closed with sign-in service unavailable when token refresh 5xxs for a signed-in user", async () => {
    const requests: string[] = [];
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(input, init);
      const url = new URL(request.url);
      requests.push(`${request.method} ${url.pathname}`);
      if (url.pathname === "/api/auth/get-session") {
        return Response.json(null);
      }
      if (url.pathname === "/api/auth/sign-in/email") {
        return Response.json({
          redirect: false,
          token: "session",
          user: { id: "u1", email: "ada@example.com", name: "Ada" },
        });
      }
      if (url.pathname === "/api/auth/token") {
        return new Response("unavailable", { status: 503 });
      }
      if (url.pathname === "/v1/capabilities") {
        return Response.json(CAPABILITIES);
      }
      return Response.json({ error: { code: "not_found", message: "no" } }, { status: 404 });
    }) as typeof fetch;

    render(
      <CloudPanel
        config={CONFIG}
        file={new File(["# Hi\n"], "notes.md", { type: "text/markdown" })}
        sourceFormat="markdown"
        preferredTarget="docx"
        profile="editable"
        onUseDevice={vi.fn()}
        cleanup={{ current: null }}
        onServerCopy={vi.fn()}
        apiOptions={{
          fetch: fetchImpl,
          operations: {
            getLimits: async () => ({
              data: limits(),
              response: new Response(null, { status: 200 }),
            }),
          },
        }}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Sign in" }));
    const dialog = screen.getByTestId("account-dialog");
    fireEvent.change(within(dialog).getByLabelText("Email"), {
      target: { value: "ada@example.com" },
    });
    fireEvent.change(within(dialog).getByLabelText("Password"), {
      target: { value: "secret123" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Sign in" }));

    await screen.findByText("Signed in as ada@example.com");
    const start = await screen.findByRole("button", { name: "Upload and convert" });
    await waitFor(() => expect(start.getAttribute("aria-disabled")).toBeNull());

    fireEvent.click(start);
    expect(await screen.findByText("Sign-in service unavailable")).toBeDefined();
    expect(requests.filter((entry) => entry === "POST /v1/uploads")).toHaveLength(0);
  });

  it("keeps the user signed in and offers a retry when sign-out fails", async () => {
    let signOutAttempts = 0;
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(input, init);
      const url = new URL(request.url);
      if (url.pathname === "/api/auth/get-session") {
        return Response.json(
          { user: { id: "u1", email: "ada@example.com", name: "Ada" } },
          { headers: { "set-auth-jwt": jwtFor("u1") } },
        );
      }
      if (url.pathname === "/api/auth/sign-out") {
        signOutAttempts += 1;
        if (signOutAttempts === 1) {
          return new Response("gateway timeout", { status: 504 });
        }
        return Response.json({ success: true });
      }
      if (url.pathname === "/v1/capabilities") {
        return Response.json(CAPABILITIES);
      }
      return Response.json({ error: { code: "not_found", message: "no" } }, { status: 404 });
    }) as typeof fetch;

    render(
      <CloudPanel
        config={CONFIG}
        file={new File(["# Hi\n"], "notes.md", { type: "text/markdown" })}
        sourceFormat="markdown"
        preferredTarget="docx"
        profile="editable"
        onUseDevice={vi.fn()}
        cleanup={{ current: null }}
        onServerCopy={vi.fn()}
        apiOptions={{
          fetch: fetchImpl,
          operations: {
            getLimits: async () => ({
              data: { ...limits(), plan: "pro", authenticated: true },
              response: new Response(null, { status: 200 }),
            }),
          },
        }}
      />,
    );

    await screen.findByText("Signed in as ada@example.com");
    fireEvent.click(screen.getByRole("button", { name: "Sign out" }));

    const alert = await screen.findByTestId("cloud-sign-out-error");
    expect(alert.textContent).toMatch(/could not sign out|refused/i);
    expect(screen.getByText("Signed in as ada@example.com")).toBeDefined();

    fireEvent.click(within(alert).getByRole("button", { name: "Try again" }));
    await screen.findByRole("button", { name: "Sign in" });
    expect(screen.queryByText("Signed in as ada@example.com")).toBeNull();
    expect(screen.queryByTestId("cloud-sign-out-error")).toBeNull();
    expect(signOutAttempts).toBe(2);
  });

  it("resets a pre-job refusal after signing in so the user can retry", async () => {
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(input, init);
      const url = new URL(request.url);
      if (url.pathname === "/api/auth/get-session") {
        return Response.json(null);
      }
      if (url.pathname === "/api/auth/sign-in/email") {
        return Response.json(
          {
            redirect: false,
            token: "session",
            user: { id: "u1", email: "ada@example.com", name: "Ada" },
          },
          { headers: { "set-auth-jwt": jwtFor("u1") } },
        );
      }
      if (url.pathname === "/v1/capabilities") {
        return Response.json(CAPABILITIES);
      }
      if (url.pathname === "/v1/uploads") {
        return Response.json(
          { error: { code: "quota_exceeded", message: "daily quota exhausted" } },
          { status: 429 },
        );
      }
      return Response.json({ error: { code: "not_found", message: "no" } }, { status: 404 });
    }) as typeof fetch;

    render(
      <CloudPanel
        config={CONFIG}
        file={new File(["# Hi\n"], "notes.md", { type: "text/markdown" })}
        sourceFormat="markdown"
        preferredTarget="docx"
        profile="editable"
        onUseDevice={vi.fn()}
        cleanup={{ current: null }}
        onServerCopy={vi.fn()}
        apiOptions={{ fetch: fetchImpl, operations: {} }}
      />,
    );

    const start = await screen.findByRole("button", { name: "Upload and convert" });
    await waitFor(() => expect(start.getAttribute("aria-disabled")).toBeNull());
    fireEvent.click(start);

    const errorBox = await screen.findByTestId("cloud-error");
    expect(errorBox.textContent).toContain("Cloud allowance used up");

    fireEvent.click(within(errorBox).getByRole("button", { name: "Sign in" }));
    const dialog = screen.getByTestId("account-dialog");
    fireEvent.change(within(dialog).getByLabelText("Email"), {
      target: { value: "ada@example.com" },
    });
    fireEvent.change(within(dialog).getByLabelText("Password"), {
      target: { value: "secret123" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Sign in" }));

    await screen.findByText("Signed in as ada@example.com");
    expect(screen.queryByTestId("cloud-error")).toBeNull();
    const retryUpload = await screen.findByRole("button", { name: "Upload and convert" });
    expect(retryUpload.getAttribute("aria-disabled")).toBeNull();
  });

  it("does not restore credentials when an in-flight /token response arrives after sign-out", async () => {
    let resolveToken: ((response: Response) => void) | undefined;
    const uploadAuthHeaders: (string | null)[] = [];
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(input, init);
      const url = new URL(request.url);
      if (url.pathname === "/api/auth/get-session") {
        return Response.json({ user: { id: "u1", email: "ada@example.com", name: "Ada" } });
      }
      if (url.pathname === "/api/auth/token") {
        return new Promise<Response>((resolve) => {
          resolveToken = resolve;
        });
      }
      if (url.pathname === "/api/auth/sign-out") {
        return Response.json({ success: true });
      }
      if (url.pathname === "/v1/capabilities") {
        return Response.json(CAPABILITIES);
      }
      if (url.pathname === "/v1/uploads") {
        uploadAuthHeaders.push(request.headers.get("authorization"));
        return Response.json(
          { error: { code: "quota_exceeded", message: "daily quota exhausted" } },
          { status: 429 },
        );
      }
      return Response.json({ error: { code: "not_found", message: "no" } }, { status: 404 });
    }) as typeof fetch;

    render(
      <CloudPanel
        config={CONFIG}
        file={new File(["# Hi\n"], "notes.md", { type: "text/markdown" })}
        sourceFormat="markdown"
        preferredTarget="docx"
        profile="editable"
        onUseDevice={vi.fn()}
        cleanup={{ current: null }}
        onServerCopy={vi.fn()}
        apiOptions={{ fetch: fetchImpl, operations: {} }}
      />,
    );

    await screen.findByText("Signed in as ada@example.com");
    await waitFor(() => expect(resolveToken).toBeTypeOf("function"));

    fireEvent.click(screen.getByRole("button", { name: "Sign out" }));
    await screen.findByRole("button", { name: "Sign in" });

    resolveToken?.(Response.json({ token: "jwt-stale" }));
    const start = await screen.findByRole("button", { name: "Upload and convert" });
    await waitFor(() => expect(start.getAttribute("aria-disabled")).toBeNull());
    fireEvent.click(start);

    await screen.findByTestId("cloud-error");
    expect(uploadAuthHeaders).toEqual([null]);
  });

  it("blocks upload and syncs user state when /token or a refreshed session returns 401 for a signed-in user", async () => {
    const tokenRequests: string[] = [];
    const tokenFetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(input, init);
      const url = new URL(request.url);
      tokenRequests.push(`${request.method} ${url.pathname}`);
      if (url.pathname === "/api/auth/get-session") {
        return Response.json(null);
      }
      if (url.pathname === "/api/auth/sign-in/email") {
        return Response.json({
          redirect: false,
          token: "session",
          user: { id: "u1", email: "ada@example.com", name: "Ada" },
        });
      }
      if (url.pathname === "/api/auth/token") {
        return new Response("expired", { status: 401 });
      }
      if (url.pathname === "/v1/capabilities") {
        return Response.json(CAPABILITIES);
      }
      if (url.pathname === "/v1/uploads") {
        return Response.json(
          { error: { code: "quota_exceeded", message: "exhausted" } },
          { status: 429 },
        );
      }
      return Response.json({ error: { code: "not_found", message: "no" } }, { status: 404 });
    }) as typeof fetch;

    const { unmount } = render(
      <CloudPanel
        config={CONFIG}
        file={new File(["# Hi\n"], "notes.md", { type: "text/markdown" })}
        sourceFormat="markdown"
        preferredTarget="docx"
        profile="editable"
        onUseDevice={vi.fn()}
        cleanup={{ current: null }}
        onServerCopy={vi.fn()}
        apiOptions={{ fetch: tokenFetch, operations: {} }}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Sign in" }));
    const dialog = screen.getByTestId("account-dialog");
    fireEvent.change(within(dialog).getByLabelText("Email"), {
      target: { value: "ada@example.com" },
    });
    fireEvent.change(within(dialog).getByLabelText("Password"), {
      target: { value: "secret123" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Sign in" }));

    await screen.findByText("Signed in as ada@example.com");
    const start = await screen.findByRole("button", { name: "Upload and convert" });
    await waitFor(() => expect(start.getAttribute("aria-disabled")).toBeNull());

    fireEvent.click(start);
    const errorBox = await screen.findByTestId("cloud-error");
    expect(errorBox.textContent).toContain("Your session has ended");
    expect(screen.queryByText("Signed in as ada@example.com")).toBeNull();
    expect(tokenRequests.filter((entry) => entry === "POST /v1/uploads")).toHaveLength(0);

    fireEvent.click(within(errorBox).getByRole("button", { name: "Continue without an account" }));
    expect(screen.queryByTestId("cloud-error")).toBeNull();
    unmount();

    let sessionCalls = 0;
    const sessionRequests: string[] = [];
    const sessionFetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(input, init);
      const url = new URL(request.url);
      sessionRequests.push(`${request.method} ${url.pathname}`);
      if (url.pathname === "/api/auth/get-session") {
        sessionCalls += 1;
        if (sessionCalls === 1) {
          return Response.json(
            { user: { id: "u1", email: "ada@example.com", name: "Ada" } },
            { headers: { "set-auth-jwt": jwtFor("u1") } },
          );
        }
        return new Response("expired", { status: 401 });
      }
      if (url.pathname === "/v1/capabilities") {
        return Response.json(CAPABILITIES);
      }
      return Response.json({ error: { code: "not_found", message: "no" } }, { status: 404 });
    }) as typeof fetch;

    render(
      <CloudPanel
        config={CONFIG}
        file={new File(["# Hi\n"], "notes.md", { type: "text/markdown" })}
        sourceFormat="markdown"
        preferredTarget="docx"
        profile="editable"
        onUseDevice={vi.fn()}
        cleanup={{ current: null }}
        onServerCopy={vi.fn()}
        apiOptions={{ fetch: sessionFetch, operations: {} }}
      />,
    );

    await screen.findByText("Signed in as ada@example.com");
    const uploadButton = await screen.findByRole("button", { name: "Upload and convert" });
    await waitFor(() => expect(uploadButton.getAttribute("aria-disabled")).toBeNull());

    fireEvent.focus(window);
    const expiredAlert = await screen.findByTestId("cloud-session-expired");
    expect(expiredAlert.textContent).toContain("Your session has ended");
    expect(screen.queryByText("Signed in as ada@example.com")).toBeNull();
    expect(uploadButton.getAttribute("aria-disabled")).toBe("true");

    fireEvent.click(uploadButton);
    expect(sessionRequests.filter((entry) => entry === "POST /v1/uploads")).toHaveLength(0);

    fireEvent.click(
      within(expiredAlert).getByRole("button", { name: "Continue without an account" }),
    );
    expect(screen.queryByTestId("cloud-session-expired")).toBeNull();
    expect(uploadButton.getAttribute("aria-disabled")).toBeNull();
  });

  it("clears the previous account's JWT when focus refresh switches to another user without set-auth-jwt", async () => {
    let sessionCalls = 0;
    const uploadAuthHeaders: (string | null)[] = [];
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(input, init);
      const url = new URL(request.url);
      if (url.pathname === "/api/auth/get-session") {
        sessionCalls += 1;
        if (sessionCalls === 1) {
          return Response.json(
            { user: { id: "a", email: "ada@example.com", name: "Ada" } },
            { headers: { "set-auth-jwt": jwtFor("a") } },
          );
        }
        return Response.json({ user: { id: "b", email: "bob@example.com", name: "Bob" } });
      }
      if (url.pathname === "/api/auth/token") {
        return Response.json({ token: jwtFor("b") });
      }
      if (url.pathname === "/v1/capabilities") {
        return Response.json(CAPABILITIES);
      }
      if (url.pathname === "/v1/uploads") {
        uploadAuthHeaders.push(request.headers.get("authorization"));
        return Response.json(
          { error: { code: "quota_exceeded", message: "daily quota exhausted" } },
          { status: 429 },
        );
      }
      return Response.json({ error: { code: "not_found", message: "no" } }, { status: 404 });
    }) as typeof fetch;

    render(
      <CloudPanel
        config={CONFIG}
        file={new File(["# Hi\n"], "notes.md", { type: "text/markdown" })}
        sourceFormat="markdown"
        preferredTarget="docx"
        profile="editable"
        onUseDevice={vi.fn()}
        cleanup={{ current: null }}
        onServerCopy={vi.fn()}
        apiOptions={{ fetch: fetchImpl, operations: {} }}
      />,
    );

    await screen.findByText("Signed in as ada@example.com");
    const start = await screen.findByRole("button", { name: "Upload and convert" });
    await waitFor(() => expect(start.getAttribute("aria-disabled")).toBeNull());

    fireEvent.focus(window);
    await screen.findByText("Signed in as bob@example.com");

    fireEvent.click(start);
    await screen.findByTestId("cloud-error");
    expect(uploadAuthHeaders).toEqual([`Bearer ${jwtFor("b")}`]);
  });

  it("stays anonymous when the user cancels a pending sign-in and chooses Continue without an account", async () => {
    let sessionCalls = 0;
    let resolveSignIn: ((response: Response) => void) | undefined;
    const uploadAuthHeaders: (string | null)[] = [];
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(input, init);
      const url = new URL(request.url);
      if (url.pathname === "/api/auth/get-session") {
        sessionCalls += 1;
        if (sessionCalls === 1) {
          return Response.json(
            { user: { id: "u1", email: "ada@example.com", name: "Ada" } },
            { headers: { "set-auth-jwt": jwtFor("u1") } },
          );
        }
        return new Response("expired", { status: 401 });
      }
      if (url.pathname === "/api/auth/sign-in/email") {
        return new Promise<Response>((resolve) => {
          resolveSignIn = resolve;
        });
      }
      if (url.pathname === "/v1/capabilities") {
        return Response.json(CAPABILITIES);
      }
      if (url.pathname === "/v1/uploads") {
        uploadAuthHeaders.push(request.headers.get("authorization"));
        return Response.json(
          { error: { code: "quota_exceeded", message: "daily quota exhausted" } },
          { status: 429 },
        );
      }
      return Response.json({ error: { code: "not_found", message: "no" } }, { status: 404 });
    }) as typeof fetch;

    render(
      <CloudPanel
        config={CONFIG}
        file={new File(["# Hi\n"], "notes.md", { type: "text/markdown" })}
        sourceFormat="markdown"
        preferredTarget="docx"
        profile="editable"
        onUseDevice={vi.fn()}
        cleanup={{ current: null }}
        onServerCopy={vi.fn()}
        apiOptions={{ fetch: fetchImpl, operations: {} }}
      />,
    );

    await screen.findByText("Signed in as ada@example.com");
    fireEvent.focus(window);
    const expiredAlert = await screen.findByTestId("cloud-session-expired");

    fireEvent.click(within(expiredAlert).getByRole("button", { name: "Sign in" }));
    const dialog = screen.getByTestId("account-dialog");
    fireEvent.change(within(dialog).getByLabelText("Email"), {
      target: { value: "ada@example.com" },
    });
    fireEvent.change(within(dialog).getByLabelText("Password"), {
      target: { value: "secret123" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Sign in" }));
    await waitFor(() => expect(resolveSignIn).toBeTypeOf("function"));

    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByTestId("account-dialog")).toBeNull();

    fireEvent.click(
      within(expiredAlert).getByRole("button", { name: "Continue without an account" }),
    );
    expect(screen.queryByTestId("cloud-session-expired")).toBeNull();

    resolveSignIn?.(
      Response.json(
        {
          redirect: false,
          token: "session",
          user: { id: "u1", email: "ada@example.com", name: "Ada" },
        },
        { headers: { "set-auth-jwt": jwtFor("u1", "late") } },
      ),
    );
    await Promise.resolve();

    expect(screen.queryByText("Signed in as ada@example.com")).toBeNull();
    expect(screen.getByRole("button", { name: "Sign in" })).toBeDefined();

    const start = screen.getByRole("button", { name: "Upload and convert" });
    await waitFor(() => expect(start.getAttribute("aria-disabled")).toBeNull());
    fireEvent.click(start);

    await screen.findByTestId("cloud-error");
    expect(uploadAuthHeaders).toEqual([null]);
  });

  it("drops a foreign-cookie /token JWT, updates the panel to the real user, and does not send the upload", async () => {
    let cookieUser: "a" | "b" | null = null;
    const uploadAuthHeaders: (string | null)[] = [];
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(input, init);
      const url = new URL(request.url);
      if (url.pathname === "/api/auth/get-session") {
        if (cookieUser === "b") {
          return Response.json(
            { user: { id: "b", email: "bob@example.com", name: "Bob" } },
            { headers: { "set-auth-jwt": jwtFor("b") } },
          );
        }
        return Response.json(null);
      }
      if (url.pathname === "/api/auth/sign-in/email") {
        cookieUser = "a";
        return Response.json({
          redirect: false,
          token: "session",
          user: { id: "a", email: "ada@example.com", name: "Ada" },
        });
      }
      if (url.pathname === "/api/auth/token") {
        return Response.json({ token: jwtFor(cookieUser ?? "a") });
      }
      if (url.pathname === "/v1/capabilities") {
        return Response.json(CAPABILITIES);
      }
      if (url.pathname === "/v1/uploads") {
        uploadAuthHeaders.push(request.headers.get("authorization"));
        return Response.json(
          { error: { code: "quota_exceeded", message: "daily quota exhausted" } },
          { status: 429 },
        );
      }
      return Response.json({ error: { code: "not_found", message: "no" } }, { status: 404 });
    }) as typeof fetch;

    render(
      <CloudPanel
        config={CONFIG}
        file={new File(["# Hi\n"], "notes.md", { type: "text/markdown" })}
        sourceFormat="markdown"
        preferredTarget="docx"
        profile="editable"
        onUseDevice={vi.fn()}
        cleanup={{ current: null }}
        onServerCopy={vi.fn()}
        apiOptions={{ fetch: fetchImpl, operations: {} }}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Sign in" }));
    const dialog = screen.getByTestId("account-dialog");
    fireEvent.change(within(dialog).getByLabelText("Email"), {
      target: { value: "ada@example.com" },
    });
    fireEvent.change(within(dialog).getByLabelText("Password"), {
      target: { value: "secret123" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Sign in" }));

    await screen.findByText("Signed in as ada@example.com");
    const start = await screen.findByRole("button", { name: "Upload and convert" });
    await waitFor(() => expect(start.getAttribute("aria-disabled")).toBeNull());

    // Another tab switches the shared auth cookie from A to B before /token runs.
    cookieUser = "b";
    fireEvent.click(start);

    await screen.findByText("Signed in as bob@example.com");
    expect(screen.queryByText("Signed in as ada@example.com")).toBeNull();
    expect(uploadAuthHeaders).toEqual([]);
  });

  it("keeps showing the switched user when a stale sign-out resolves after a focus session refresh", async () => {
    let sessionCalls = 0;
    let resolveSignOut: ((response: Response) => void) | undefined;
    const uploadAuthHeaders: (string | null)[] = [];
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(input, init);
      const url = new URL(request.url);
      if (url.pathname === "/api/auth/get-session") {
        sessionCalls += 1;
        if (sessionCalls === 1) {
          return Response.json(
            { user: { id: "a", email: "ada@example.com", name: "Ada" } },
            { headers: { "set-auth-jwt": jwtFor("a") } },
          );
        }
        return Response.json(
          { user: { id: "b", email: "bob@example.com", name: "Bob" } },
          { headers: { "set-auth-jwt": jwtFor("b") } },
        );
      }
      if (url.pathname === "/api/auth/sign-out") {
        return new Promise<Response>((resolve) => {
          resolveSignOut = resolve;
        });
      }
      if (url.pathname === "/v1/capabilities") {
        return Response.json(CAPABILITIES);
      }
      if (url.pathname === "/v1/uploads") {
        uploadAuthHeaders.push(request.headers.get("authorization"));
        return Response.json(
          { error: { code: "quota_exceeded", message: "daily quota exhausted" } },
          { status: 429 },
        );
      }
      return Response.json({ error: { code: "not_found", message: "no" } }, { status: 404 });
    }) as typeof fetch;

    render(
      <CloudPanel
        config={CONFIG}
        file={new File(["# Hi\n"], "notes.md", { type: "text/markdown" })}
        sourceFormat="markdown"
        preferredTarget="docx"
        profile="editable"
        onUseDevice={vi.fn()}
        cleanup={{ current: null }}
        onServerCopy={vi.fn()}
        apiOptions={{ fetch: fetchImpl, operations: {} }}
      />,
    );

    await screen.findByText("Signed in as ada@example.com");
    const start = await screen.findByRole("button", { name: "Upload and convert" });
    await waitFor(() => expect(start.getAttribute("aria-disabled")).toBeNull());

    fireEvent.click(screen.getByRole("button", { name: "Sign out" }));
    await waitFor(() => expect(resolveSignOut).toBeTypeOf("function"));

    fireEvent.focus(window);
    await screen.findByText("Signed in as bob@example.com");

    resolveSignOut?.(Response.json({ success: true }));
    await waitFor(() => {
      const signOutBtn = screen.queryByRole("button", { name: "Sign out" });
      expect(signOutBtn === null || !signOutBtn.hasAttribute("disabled")).toBe(true);
    });

    expect(screen.getByText("Signed in as bob@example.com")).toBeDefined();
    expect(screen.queryByRole("button", { name: "Sign in" })).toBeNull();

    fireEvent.click(start);
    await screen.findByTestId("cloud-error");
    expect(uploadAuthHeaders).toEqual([`Bearer ${jwtFor("b")}`]);
  });

  it("retries capabilities and unblocks upload when the cookie switches from A to B between initial session resolve and the capabilities token fetch", async () => {
    let sessionCalls = 0;
    const uploadAuthHeaders: (string | null)[] = [];
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(input, init);
      const url = new URL(request.url);
      if (url.pathname === "/api/auth/get-session") {
        sessionCalls += 1;
        if (sessionCalls === 1) {
          return Response.json({ user: { id: "a", email: "ada@example.com", name: "Ada" } });
        }
        return Response.json({ user: { id: "b", email: "bob@example.com", name: "Bob" } });
      }
      if (url.pathname === "/api/auth/token") {
        return Response.json({ token: jwtFor("b") });
      }
      if (url.pathname === "/v1/capabilities") {
        return Response.json(CAPABILITIES);
      }
      if (url.pathname === "/v1/uploads") {
        uploadAuthHeaders.push(request.headers.get("authorization"));
        return Response.json(
          { error: { code: "quota_exceeded", message: "daily quota exhausted" } },
          { status: 429 },
        );
      }
      return Response.json({ error: { code: "not_found", message: "no" } }, { status: 404 });
    }) as typeof fetch;

    render(
      <CloudPanel
        config={CONFIG}
        file={new File(["# Hi\n"], "notes.md", { type: "text/markdown" })}
        sourceFormat="markdown"
        preferredTarget="docx"
        profile="editable"
        onUseDevice={vi.fn()}
        cleanup={{ current: null }}
        onServerCopy={vi.fn()}
        apiOptions={{ fetch: fetchImpl, operations: {} }}
      />,
    );

    await screen.findByText("Signed in as bob@example.com");
    expect(screen.queryByText("Signed in as ada@example.com")).toBeNull();
    const start = await screen.findByRole("button", { name: "Upload and convert" });
    await waitFor(() => expect(start.getAttribute("aria-disabled")).toBeNull());
    expect(screen.queryByTestId("cloud-blocked")).toBeNull();

    fireEvent.click(start);
    await screen.findByTestId("cloud-error");
    expect(uploadAuthHeaders).toEqual([`Bearer ${jwtFor("b")}`]);
  });
});
