// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { CloudPanelProps } from "@/components/cloud/cloud-panel";
import { ConverterApp } from "@/components/converter/converter-app";
import { ConverterClient } from "@/converter/client";
import type { RequestEnvelope, WorkerResponse } from "@/converter/protocol";
import type { DiscardOutcome, ServerCopy } from "@/lib/cloud/cloud-job";

vi.mock("@/lib/cloud/config", () => ({
  CLOUD: {
    apiUrl: "https://api.example.com",
    apiHost: "api.example.com",
    authUrl: null,
    turnstileSiteKey: null,
    operator: null,
  },
}));

// The panel's own deletion is covered in cloud-job.test.ts; here it only hands the
// page the outcome a failed or unsupported deletion produces.
const panel = vi.hoisted(() => ({
  discard: null as null | (() => Promise<DiscardOutcome>),
}));
vi.mock("@/components/cloud/cloud-panel", () => ({
  default: ({ cleanup }: CloudPanelProps) => {
    cleanup.current = panel.discard;
    return <p data-testid="cloud-panel">Cloud panel</p>;
  },
}));

class FakeWorker extends EventTarget {
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
          ? {
              name: "page.rtf",
              size: 13,
              inspection: {
                format: "rtf",
                detected_by: "extension",
                reachable: [],
                reachable_elsewhere: ["markdown"],
                estimated_size_bytes: 13,
                confidence: "exact",
                counts: null,
                warnings: [],
              },
              recycle: false,
            }
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

function kept(reason: ServerCopy["reason"], retry: ServerCopy["retry"]): ServerCopy {
  return {
    status: "kept",
    id: "5f0e8d3a-2c4b-4e71-8b6a-9d1c3e2f4a57",
    fileName: "page.rtf",
    reason,
    expiresAt: "2026-10-25T12:00:00Z",
    retry,
  };
}

async function startOverFromCloud() {
  const client = new ConverterClient(() => new FakeWorker() as unknown as Worker);
  render(<ConverterApp client={client} />);
  const input = document.querySelector("input[type=file]") as HTMLInputElement;
  fireEvent.change(input, {
    target: { files: [new File(["{\\rtf1 Hello}"], "page.rtf", { type: "application/rtf" })] },
  });
  fireEvent.click(await screen.findByRole("button", { name: "Process in Cloud" }));
  await screen.findByTestId("cloud-panel");
  await waitFor(() =>
    expect(HTMLElement.prototype.scrollIntoView).toHaveBeenCalledWith({ block: "nearest" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Start over and delete files" }));
}

describe("Start over after a cloud job", () => {
  beforeEach(() => {
    // jsdom has no scrolling implementation; assert the requested browser action above.
    HTMLElement.prototype.scrollIntoView = vi.fn();
  });

  it("keeps a failed server deletion visible and retryable", async () => {
    const retry = vi.fn(async (): Promise<DiscardOutcome> => ({ status: "deleted" }));
    panel.discard = vi.fn(async () => kept("failed", retry));
    await startOverFromCloud();

    const notice = await screen.findByTestId("server-copy");
    expect(panel.discard).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("cloud-panel")).toBeNull();
    expect(notice.textContent).toContain("page.rtf is still on api.example.com");
    expect(notice.textContent).toContain("api.example.com deletes it automatically at");
    await waitFor(() =>
      expect(screen.getByRole("status").textContent).toBe(
        "Files were removed from this device. page.rtf is still on the server.",
      ),
    );

    fireEvent.click(screen.getByRole("button", { name: "Delete from server" }));
    await waitFor(() => expect(screen.queryByTestId("server-copy")).toBeNull());
    expect(retry).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status").textContent).toBe("page.rtf was deleted from the server.");
  });

  it("keeps the notice when the retry fails again", async () => {
    const retry: ServerCopy["retry"] = vi.fn(async () => kept("failed", retry));
    panel.discard = vi.fn(async () => kept("failed", retry));
    await startOverFromCloud();

    fireEvent.click(await screen.findByRole("button", { name: "Delete from server" }));
    await waitFor(() => expect(retry).toHaveBeenCalledTimes(1));
    expect(screen.getAllByTestId("server-copy")).toHaveLength(1);
    expect(
      screen.getByRole("button", { name: "Delete from server" }).hasAttribute("disabled"),
    ).toBe(false);
  });

  it("names the retention when the server cannot delete an upload on request", async () => {
    const retry = vi.fn(async (): Promise<DiscardOutcome> => ({ status: "none" }));
    panel.discard = vi.fn(async () => kept("unsupported", retry));
    await startOverFromCloud();

    const notice = await screen.findByTestId("server-copy");
    expect(notice.textContent).toContain("This server cannot delete an upload on request.");
    expect(notice.textContent).toContain("deletes it automatically at");
    expect(screen.queryByRole("button", { name: "Delete from server" })).toBeNull();
  });
});
