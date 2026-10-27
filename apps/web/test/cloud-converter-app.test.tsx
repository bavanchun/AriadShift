// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { type ComponentType, lazy } from "react";
import type { CloudPanelProps } from "@/components/cloud/cloud-panel";
import { ConverterApp } from "@/components/converter/converter-app";
import { ConverterClient } from "@/converter/client";
import type { RequestEnvelope, WorkerResponse } from "@/converter/protocol";

// Cloud mode is configured at build time; these tests build it in.
vi.mock("@/lib/cloud/config", () => ({
  CLOUD: {
    apiUrl: "https://api.example.com",
    apiHost: "api.example.com",
    authUrl: null,
    turnstileSiteKey: null,
    operator: null,
  },
}));

const requests: string[] = [];

class FakeWorker extends EventTarget {
  constructor(private readonly inspection: object) {
    super();
  }

  postMessage(request: RequestEnvelope) {
    const result =
      request.type === "init"
        ? {
            version: "0.1.0",
            session: "s",
            storage: "opfs",
            memoryLimitBytes: 64 * 1024 * 1024,
            inputLimitBytes: 16 * 1024 * 1024,
            resources: [],
          }
        : request.type === "ingest"
          ? { name: "page.rtf", size: 13, inspection: this.inspection, recycle: false }
          : request.type === "plan"
            ? { from: "markdown", to: "html", profile: "editable", steps: [], score: {} }
            : null;
    queueMicrotask(() =>
      this.dispatchEvent(
        new MessageEvent("message", {
          data: { id: request.id, ok: true, result } as WorkerResponse,
        }),
      ),
    );
  }

  terminate() {}
}

const RTF = {
  format: "rtf",
  detected_by: "extension",
  reachable: [],
  reachable_elsewhere: ["markdown"],
  estimated_size_bytes: 13,
  confidence: "exact",
  counts: null,
  warnings: [],
};

function renderApp(inspection: object = RTF, cloudPanel?: ComponentType<CloudPanelProps>) {
  const client = new ConverterClient(() => new FakeWorker(inspection) as unknown as Worker);
  render(<ConverterApp client={client} cloudPanel={cloudPanel} />);
  const input = document.querySelector("input[type=file]") as HTMLInputElement;
  fireEvent.change(input, {
    target: { files: [new File(["{\\rtf1 Hello}"], "page.rtf", { type: "application/rtf" })] },
  });
}

describe("ConverterApp with cloud mode configured", () => {
  beforeEach(() => {
    requests.length = 0;
    // jsdom has no scrolling API; verify the hand-off's browser action below.
    HTMLElement.prototype.scrollIntoView = vi.fn();
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const request = input instanceof Request ? input : new Request(input, init);
        requests.push(`${request.method} ${new URL(request.url).pathname}`);
        return Response.json({
          version: "ariad-capabilities/0",
          generated_at: "2026-10-25T11:00:00Z",
          packs: ["core", "office"],
          formats: [
            { id: "rtf", extensions: ["rtf"], media_type: "application/rtf" },
            { id: "markdown", extensions: ["md"], media_type: "text/markdown" },
          ],
          edges: [
            { from: "rtf", to: "markdown", engine: "pandoc", pack: "office", license: "GPL" },
          ],
        });
      }),
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("stays on this device until the user picks the cloud", async () => {
    renderApp();
    await screen.findByTestId("desktop-handoff");
    const device = screen.getByRole("radio", { name: /On this device/ });
    const cloud = screen.getByRole("radio", { name: /On api\.example\.com/ });
    expect(device).toHaveProperty("checked", true);
    expect(cloud).toHaveProperty("checked", false);
    expect(screen.getByTestId("device-indicator").textContent).toContain(
      "Processed on this device.",
    );
    expect(screen.queryByTestId("cloud-panel")).toBeNull();
    expect(requests).toEqual([]);
  });

  it("hands off to the cloud from the desktop notice without uploading", async () => {
    renderApp();
    fireEvent.click(await screen.findByRole("button", { name: "Process in Cloud" }));
    const panel = await screen.findByTestId("cloud-panel");
    expect(panel.textContent).toContain("This file will be uploaded to api.example.com");
    expect(screen.getByRole("radio", { name: /On api\.example\.com/ })).toHaveProperty(
      "checked",
      true,
    );
    expect(screen.getByTestId("device-indicator").textContent).toContain(
      "Cloud conversion selected.",
    );
    await waitFor(() => expect(requests).toContain("GET /v1/capabilities"));
    await waitFor(() =>
      expect(HTMLElement.prototype.scrollIntoView).toHaveBeenCalledWith({ block: "nearest" }),
    );
    expect(requests.some((request) => request.startsWith("POST"))).toBe(false);

    fireEvent.click(screen.getByRole("radio", { name: /On this device/ }));
    expect(screen.queryByTestId("cloud-panel")).toBeNull();
  });

  it("catches a failed lazy CloudPanel chunk load and offers Convert on this device", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      const BrokenCloudPanel = lazy<ComponentType<CloudPanelProps>>(() =>
        Promise.reject(new TypeError("Failed to fetch dynamically imported module")),
      );
      renderApp(RTF, BrokenCloudPanel);
      fireEvent.click(await screen.findByRole("button", { name: "Process in Cloud" }));

      const fallback = await screen.findByTestId("cloud-load-error");
      expect(fallback.textContent).toContain("Cloud mode could not load");
      fireEvent.click(screen.getByRole("button", { name: "Convert on this device" }));
      expect(screen.queryByTestId("cloud-load-error")).toBeNull();
      expect(screen.getByRole("radio", { name: /On this device/ })).toHaveProperty("checked", true);
    } finally {
      consoleError.mockRestore();
    }
  });
});
