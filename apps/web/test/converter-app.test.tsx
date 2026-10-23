// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ConverterApp } from "@/components/converter/converter-app";
import { ConverterClient } from "@/converter/client";
import type { RequestEnvelope, WorkerResponse } from "@/converter/protocol";

class FakeWorker extends EventTarget {
  readonly session = "test-session";
  readonly received: RequestEnvelope[] = [];
  terminated = false;
  reply: (request: RequestEnvelope) => WorkerResponse | null = (request) => this.ok(request);

  ok(request: RequestEnvelope, extra: object = {}): WorkerResponse {
    const result =
      request.type === "init"
        ? {
            version: "0.1.0",
            session: this.session,
            storage: "opfs",
            memoryLimitBytes: 64 * 1024 * 1024,
            inputLimitBytes: 16 * 1024 * 1024,
            resources: [],
            ...extra,
          }
        : request.type === "reset"
          ? null
          : extra;
    return { id: request.id, ok: true, result } as WorkerResponse;
  }

  postMessage(request: RequestEnvelope) {
    this.received.push(request);
    const response = this.reply(request);
    if (response) {
      queueMicrotask(() => this.dispatchEvent(new MessageEvent("message", { data: response })));
    }
  }

  terminate() {
    this.terminated = true;
  }
}

describe("ConverterApp", () => {
  it("renders the memory fallback notice displaying the effective limit", async () => {
    const worker = new FakeWorker();
    worker.reply = (request) => {
      if (request.type === "init") {
        return worker.ok(request, {
          storage: "memory",
          memoryLimitBytes: 64 * 1024 * 1024,
          inputLimitBytes: 16 * 1024 * 1024,
        });
      }
      if (request.type === "ingest") {
        return worker.ok(request, {
          name: "notes.md",
          size: 7,
          inspection: {
            format: "markdown",
            detected_by: "extension",
            reachable: ["html"],
            reachable_elsewhere: [],
            estimated_size_bytes: 7,
            confidence: "exact",
            counts: null,
            warnings: [],
          },
          recycle: false,
        });
      }
      return worker.ok(request);
    };
    const client = new ConverterClient(() => worker as unknown as Worker);

    render(<ConverterApp client={client} />);

    const input = document.querySelector("input[type=file]") as HTMLInputElement;
    const file = new File(["# Hello"], "notes.md", { type: "text/markdown" });
    fireEvent.change(input, { target: { files: [file] } });

    await waitFor(() => {
      expect(
        screen.getByText("On-device file storage is not available in this browser"),
      ).toBeDefined();
    });
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("inputs up to 16 MB are supported");
    expect(alert.textContent).not.toContain("64 MB");
  });

  it("clears the previous result card immediately when a new conversion starts", async () => {
    const worker = new FakeWorker();
    let convertCount = 0;

    worker.reply = (request) => {
      if (request.type === "init") {
        return worker.ok(request, { storage: "opfs" });
      }
      if (request.type === "ingest") {
        return worker.ok(request, {
          name: "notes.md",
          size: 7,
          inspection: {
            format: "markdown",
            detected_by: "extension",
            reachable: ["html", "ariad-ir+json"],
            reachable_elsewhere: [],
            estimated_size_bytes: 7,
            confidence: "exact",
            counts: null,
            warnings: [],
          },
          recycle: false,
        });
      }
      if (request.type === "plan") {
        return worker.ok(request, {
          steps: [{ from: "markdown", to: "html", engine: "ariad-core" }],
          score: { fidelity: 1, editability: 1, estimated_duration_ms: 10 },
          measured: true,
          alternatives: [],
        });
      }
      if (request.type === "convert") {
        convertCount++;
        if (convertCount === 1) {
          return worker.ok(request, {
            location: {
              kind: "memory",
              blob: new Blob(["<h1>Hello</h1>"], { type: "text/html" }),
            },
            name: "notes.html",
            size: 15,
            format: "html",
            mediaType: "text/html",
            route: [{ from: "markdown", to: "html", engine: "ariad-core" }],
            warnings: [],
            elapsedMs: 25,
            recycle: false,
          });
        }
        // Second conversion: leave pending to observe the clearing while converting
        return null;
      }
      return worker.ok(request);
    };

    const client = new ConverterClient(() => worker as unknown as Worker);
    render(<ConverterApp client={client} />);

    const fileInput = document.querySelector("input[type=file]") as HTMLInputElement;
    const file = new File(["# Hello"], "notes.md", { type: "text/markdown" });
    fireEvent.change(fileInput, { target: { files: [file] } });

    const convertButton = await screen.findByRole("button", { name: "Convert" });
    await waitFor(() => {
      expect((convertButton as HTMLButtonElement).disabled).toBe(false);
    });

    fireEvent.click(convertButton);

    await screen.findByTestId("result");
    expect(screen.getByRole("button", { name: "Download notes.html" })).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: "Convert" }));

    expect(screen.queryByTestId("result")).toBeNull();
    expect(screen.queryByRole("button", { name: "Download notes.html" })).toBeNull();
  });
});
