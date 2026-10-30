import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App, noticeTimeout } from "../src/app";
import type { JobView } from "../src/bindings/types";
import { finishedToasts, toastTimeout } from "../src/feedback";
import { isSupportedPath, partitionPaths } from "../src/formats";
import { newlyFinished, nextSettled, orderJobs, statusMap, summarize } from "../src/queue";
import { ariaShortcut, matchShortcut, shortcutLabel } from "../src/shortcuts";

type DragPayload =
  | { type: "enter"; paths: string[] }
  | { type: "over" }
  | { type: "drop"; paths: string[] }
  | { type: "leave" };

const { drag, ipc, picker, reveal } = vi.hoisted(() => ({
  drag: { handler: undefined as ((event: { payload: DragPayload }) => void) | undefined },
  ipc: vi.fn(),
  picker: vi.fn(),
  reveal: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc }));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: picker }));
vi.mock("@tauri-apps/plugin-deep-link", () => ({
  getCurrent: async () => null,
  onOpenUrl: async () => () => {},
}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async (handler: (event: { payload: DragPayload }) => void) => {
      drag.handler = handler;
      return () => {
        drag.handler = undefined;
      };
    },
  }),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: reveal }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: async () => null }));
vi.mock("@ariadshift/ui/components/theme-toggle", () => ({ ThemeToggle: () => null }));

const settings = {
  profile: "private",
  output_policy: "adjacent",
  output_folder: null,
  overwrite: false,
  telemetry: false,
  concurrency: 2,
};
const job = (id: string, status: JobView["status"], extra: Partial<JobView> = {}): JobView => ({
  id,
  input: `/docs/${id}.md`,
  output: `/docs/${id}.html`,
  target: "html",
  profile: "private",
  status,
  stage: status,
  error: null,
  warnings: [],
  ...extra,
});
let jobs: JobView[] = [];

beforeEach(() => {
  jobs = [];
  drag.handler = undefined;
  picker.mockReset().mockResolvedValue(null);
  reveal.mockReset().mockResolvedValue(undefined);
  ipc.mockReset().mockImplementation(async (name: string, args: { request: unknown }) => {
    if (name === "snapshot")
      return { jobs, watches: [], settings, pack_progress: "", pack_busy: false };
    if (name === "inspect")
      return { summary: "Markdown", targets: ["html"], explanation: "md → html", warnings: [] };
    if (name === "settings") return args.request;
    return [];
  });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const press = (key: string, init: KeyboardEventInit = {}) =>
  fireEvent.keyDown(window, { key, ctrlKey: true, ...init });
const isDisabled = (element: HTMLElement) =>
  element.hasAttribute("disabled") || element.getAttribute("aria-disabled") === "true";
const ready = () => waitFor(() => expect(ipc).toHaveBeenCalledWith("snapshot", { request: {} }));

describe("helpers", () => {
  it("accepts supported document extensions and skips folders and other types", () => {
    expect(isSupportedPath("/a/Report.DOCX")).toBe(true);
    expect(isSupportedPath("C:\\docs\\scan.tiff")).toBe(true);
    expect(isSupportedPath("/a/folder")).toBe(false);
    expect(isSupportedPath("/a/.md")).toBe(false);
    expect(partitionPaths(["/a/x.md", "/a/y.zip", "/a/dir"])).toEqual({
      supported: ["/a/x.md"],
      skipped: ["/a/y.zip", "/a/dir"],
    });
  });

  it("maps platform shortcuts without stealing plain typing", () => {
    const key = (k: string, init: Partial<KeyboardEvent> = {}) => ({
      key: k,
      metaKey: false,
      ctrlKey: false,
      altKey: false,
      shiftKey: false,
      isComposing: false,
      ...init,
    });
    expect(matchShortcut(key("2", { ctrlKey: true }), 6, false)).toEqual({ kind: "tab", index: 1 });
    expect(matchShortcut(key("2", { metaKey: true }), 6, true)).toEqual({ kind: "tab", index: 1 });
    expect(matchShortcut(key("2", { ctrlKey: true }), 6, true)).toBeNull();
    expect(matchShortcut(key("7", { ctrlKey: true }), 6, false)).toBeNull();
    expect(matchShortcut(key(",", { ctrlKey: true }), 6, false)).toEqual({ kind: "settings" });
    expect(matchShortcut(key("O", { ctrlKey: true }), 6, false)).toEqual({ kind: "open" });
    expect(matchShortcut(key("Enter", { ctrlKey: true }), 6, false)).toEqual({ kind: "convert" });
    expect(matchShortcut(key("Escape"), 6, false)).toEqual({ kind: "dismiss" });
    expect(matchShortcut(key("o"), 6, false)).toBeNull();
    expect(matchShortcut(key("o", { ctrlKey: true, isComposing: true }), 6, false)).toBeNull();
    expect(matchShortcut(key("o", { ctrlKey: true, shiftKey: true }), 6, false)).toBeNull();
    expect(shortcutLabel("o", true)).toBe("⌘O");
    expect(shortcutLabel("Enter", false)).toBe("Ctrl+Enter");
    expect(ariaShortcut("o", false)).toBe("Control+O");
    expect(ariaShortcut(",", true)).toBe("Meta+,");
  });

  it("measures batch progress only for work started since the queue was idle", () => {
    const history = [job("old", "succeeded")];
    const settled = nextSettled(history, new Set());
    const batch = [...history, job("a", "succeeded"), job("b", "running"), job("c", "queued")];
    expect(nextSettled(batch, settled)).toBe(settled);
    expect(summarize(batch, settled)).toMatchObject({
      active: 2,
      running: 1,
      queued: 1,
      succeeded: 2,
      batchTotal: 3,
      batchDone: 1,
    });
  });

  it("reports only jobs that finished since the previous snapshot", () => {
    const before = [job("a", "running"), job("b", "succeeded")];
    const after = [job("a", "failed"), job("b", "succeeded"), job("c", "succeeded")];
    expect(newlyFinished(undefined, after)).toEqual([]);
    expect(newlyFinished(statusMap(before), after).map((j) => j.id)).toEqual(["a", "c"]);
  });

  it("lists failures first, then running, waiting and finished work, newest first", () => {
    const listed = [
      job("old-done", "succeeded"),
      job("waiting", "queued"),
      job("old-fail", "failed"),
      job("running", "running"),
      job("new-done", "succeeded"),
      job("new-fail", "failed"),
    ];
    expect(orderJobs(listed).map((j) => j.id)).toEqual([
      "new-fail",
      "old-fail",
      "running",
      "waiting",
      "new-done",
      "old-done",
    ]);
  });

  it("collapses a large batch into one summary toast", () => {
    const handlers = { reveal: vi.fn(), retry: vi.fn(), viewQueue: vi.fn() };
    const many = ["a", "b", "c", "d"].map((id) => job(id, id === "d" ? "failed" : "succeeded"));
    const [summary, ...rest] = finishedToasts(many, handlers);
    expect(rest).toEqual([]);
    expect(summary).toMatchObject({
      tone: "error",
      text: "4 conversions finished: 3 converted, 1 failed.",
    });
    const [failed] = finishedToasts([job("x", "failed", { error: "Pandoc missing" })], handlers);
    expect(failed?.text).toBe("Could not convert x.md: Pandoc missing");
    failed?.actions[0]?.run();
    expect(handlers.retry).toHaveBeenCalledWith(expect.objectContaining({ id: "x" }));
  });
});

describe("desktop polish", () => {
  it("previews a drag, adds supported files and explains what it skipped", async () => {
    render(<App />);
    await waitFor(() => expect(drag.handler).toBeDefined());
    act(() => drag.handler?.({ payload: { type: "enter", paths: ["/d/a.md", "/d/b.zip"] } }));
    expect(screen.getByText("Drop to add 1 document")).toBeTruthy();
    expect(screen.getByText("1 item will be skipped.")).toBeTruthy();
    expect(screen.getByRole("region", { name: "Drop documents" }).dataset.dragging).toBe("true");
    act(() => drag.handler?.({ payload: { type: "drop", paths: ["/d/a.md", "/d/b.zip"] } }));
    await screen.findByText("a.md");
    expect(screen.queryByText("Drop to add 1 document")).toBeNull();
    expect(
      screen.getByText("Skipped 1 item that is not a supported document: b.zip."),
    ).toBeTruthy();
    act(() => drag.handler?.({ payload: { type: "enter", paths: ["/d/folder"] } }));
    expect(screen.getByText("These items can’t be converted")).toBeTruthy();
    act(() => drag.handler?.({ payload: { type: "leave" } }));
    expect(screen.queryByText("These items can’t be converted")).toBeNull();
  });

  it("switches tabs from the keyboard and moves focus to the page heading", async () => {
    render(<App />);
    await ready();
    press("2");
    const heading = await screen.findByRole("heading", { level: 1, name: "Queue" });
    expect(document.activeElement).toBe(heading);
    press(",");
    await screen.findByRole("heading", { level: 1, name: "Settings" });
    expect(screen.getByRole("button", { name: /^Queue/ }).getAttribute("aria-keyshortcuts")).toBe(
      "Control+2",
    );
  });

  it("opens the picker and starts conversion from the keyboard", async () => {
    picker.mockResolvedValue(["/docs/keyboard.md"]);
    render(<App />);
    await ready();
    press("o");
    await screen.findByText("keyboard.md");
    press("Enter");
    await waitFor(() =>
      expect(ipc).toHaveBeenCalledWith("enqueue", {
        request: expect.objectContaining({ paths: ["/docs/keyboard.md"] }),
      }),
    );
  });

  it("dismisses the latest message with Escape and expires info notices", async () => {
    render(<App />);
    await ready();
    act(() => drag.handler?.({ payload: { type: "drop", paths: ["/d/x.zip"] } }));
    await screen.findByText(/Skipped 1 item/);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByText(/Skipped 1 item/)).toBeNull();

    vi.useFakeTimers({ shouldAdvanceTime: true });
    fireEvent.click(screen.getByRole("button", { name: /^Settings$/ }));
    fireEvent.click(await screen.findByLabelText("Opt in to telemetry"));
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await screen.findByText("Settings saved");
    // Flush the asynchronous save and its notice effect while advancing the clock.
    await act(async () => vi.advanceTimersByTimeAsync(noticeTimeout + 100));
    expect(screen.queryByText("Settings saved")).toBeNull();
  });

  it("keeps Save disabled until a setting changes and can discard edits", async () => {
    render(<App />);
    await ready();
    fireEvent.click(screen.getByRole("button", { name: /^Settings$/ }));
    const save = await screen.findByRole("button", { name: "Save settings" });
    expect(isDisabled(save)).toBe(true);
    fireEvent.click(screen.getByLabelText("Opt in to telemetry"));
    expect(screen.getByText("Unsaved changes")).toBeTruthy();
    expect(isDisabled(save)).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Discard changes" }));
    expect((screen.getByLabelText("Opt in to telemetry") as HTMLInputElement).checked).toBe(false);
    expect(screen.queryByText("Unsaved changes")).toBeNull();
    expect(screen.getByRole("heading", { name: "Keyboard shortcuts" })).toBeTruthy();
  });

  it("offers a next step in empty states", async () => {
    picker.mockResolvedValue(["/docs/empty.md"]);
    render(<App />);
    await ready();
    fireEvent.click(screen.getByRole("button", { name: /^Queue/ }));
    await screen.findByRole("heading", { name: "No conversions yet" });
    fireEvent.click(screen.getByRole("button", { name: "Choose files" }));
    await screen.findByText("empty.md");
    expect(screen.getByRole("heading", { level: 1, name: "Convert" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Watch folders" }));
    expect(screen.getByRole("heading", { name: "No folders are watched yet" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Engine packs" }));
    await screen.findByRole("heading", { name: "No engine packs listed" });
  });

  it("summarizes the queue, retries failed jobs and labels status with text", async () => {
    jobs = [job("done", "succeeded"), job("bad", "failed", { error: "No writer" })];
    render(<App />);
    await ready();
    fireEvent.click(screen.getByRole("button", { name: /^Queue/ }));
    await screen.findByText("All done · 1 converted · 1 failed");
    expect(screen.getByText("Converted")).toBeTruthy();
    expect(screen.getByText("Failed")).toBeTruthy();
    expect(isDisabled(screen.getByRole("button", { name: "Cancel pending jobs" }))).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Retry failed (1)" }));
    await waitFor(() => expect(ipc).toHaveBeenCalledWith("retry", { request: { id: "bad" } }));
    jobs = [...jobs, job("next", "running"), job("later", "queued")];
    await screen.findByText("1 converting · 1 waiting · 0 of 2 finished");
    expect(screen.getByRole("progressbar", { name: "Batch progress" })).toBeTruthy();
    press(".");
    await waitFor(() => expect(ipc).toHaveBeenCalledWith("cancel", { request: { id: "later" } }));
    expect(ipc).toHaveBeenCalledWith("cancel", { request: { id: "next" } });
    expect(ipc).not.toHaveBeenCalledWith("cancel", { request: { id: "done" } });
  });

  it("copies a failed job's error for a bug report", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    jobs = [job("bad", "failed", { error: "Pandoc 3.12 or newer was not found" })];
    render(<App />);
    await ready();
    fireEvent.click(screen.getByRole("button", { name: /^Queue/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Copy error" }));
    expect(writeText).toHaveBeenCalledWith("Pandoc 3.12 or newer was not found");
    await screen.findByText("Error copied");
  });

  it("announces finished conversions outside the Queue with a follow-up action", async () => {
    jobs = [job("report", "running")];
    render(<App />);
    await ready();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    jobs = [job("report", "succeeded")];
    const toast = await screen.findByText("Converted report.md to HTML.");
    expect(toast.closest("section")?.getAttribute("aria-live")).toBe("polite");
    fireEvent.click(screen.getByRole("button", { name: "Show output" }));
    expect(reveal).toHaveBeenCalledWith("/docs/report.html");
    expect(screen.queryByText("Converted report.md to HTML.")).toBeNull();

    // A job that appears and fails between two snapshots is still reported.
    jobs = [job("report", "succeeded"), job("broken", "failed", { error: "Pandoc missing" })];
    await screen.findByText("Could not convert broken.md: Pandoc missing");
    await act(async () => vi.advanceTimersByTime(toastTimeout + 1000));
    expect(screen.getByText("Could not convert broken.md: Pandoc missing")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss notification" }));
    expect(screen.queryByText("Could not convert broken.md: Pandoc missing")).toBeNull();
  });
});
