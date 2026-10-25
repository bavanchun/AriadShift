// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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
});
