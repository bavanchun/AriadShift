import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "../src/app";

const { ipc, links, picker, updateCheck } = vi.hoisted(() => ({
  ipc: vi.fn(),
  links: {
    current: null as string[] | null,
    handler: undefined as ((urls: string[]) => void) | undefined,
  },
  picker: vi.fn(),
  updateCheck: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: picker }));
vi.mock("@tauri-apps/plugin-deep-link", () => ({
  getCurrent: async () => links.current,
  onOpenUrl: async (handler: (urls: string[]) => void) => {
    links.handler = handler;
    return () => {
      links.handler = undefined;
    };
  },
}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: updateCheck }));
vi.mock("@ariadshift/ui/components/theme-toggle", () => ({ ThemeToggle: () => null }));
const settings = {
  profile: "private",
  output_policy: "adjacent",
  output_folder: null,
  overwrite: false,
  telemetry: false,
  concurrency: 2,
};
beforeEach(() => {
  ipc.mockReset();
  links.current = null;
  links.handler = undefined;
  picker.mockReset().mockResolvedValue(null);
  updateCheck.mockReset().mockResolvedValue(null);
  ipc.mockImplementation(async (name, args) => {
    if (name === "snapshot")
      return { jobs: [], watches: [], settings, pack_progress: "", pack_busy: false };
    if (name === "inspect")
      return {
        summary: "Markdown · 5 bytes",
        targets: ["html"],
        explanation: "md → IR → html · Runs locally",
        warnings: [],
      };
    if (name === "settings") return args.request;
    return [];
  });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
describe("desktop workflow", () => {
  it("preselects a deep-link route and profile, opens the picker and debounces delivery", async () => {
    const url = "ariadshift://open?route=md-html&profile=faithful";
    links.current = [url];
    picker.mockResolvedValue(["/tmp/linked.md"]);
    const original = ipc.getMockImplementation();
    ipc.mockImplementation(async (name, args) =>
      name === "parse_link"
        ? { from: "md", target: "html", profile: "faithful" }
        : original?.(name, args),
    );
    vi.spyOn(Date, "now").mockReturnValue(5000);
    render(<App />);
    await screen.findByText("linked.md");
    expect(picker).toHaveBeenCalledWith(
      expect.objectContaining({
        filters: [{ name: "Documents", extensions: ["md", "markdown", "mdown"] }],
      }),
    );
    await act(async () => links.handler?.([url]));
    expect(picker).toHaveBeenCalledTimes(1);
    vi.mocked(Date.now).mockReturnValue(6001);
    await act(async () => links.handler?.([url]));
    expect(picker).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("button", { name: "Start conversion" }));
    await waitFor(() =>
      expect(ipc).toHaveBeenCalledWith("enqueue", {
        request: {
          paths: ["/tmp/linked.md"],
          target: "html",
          profile: "faithful",
          output_folder: null,
          overwrite: false,
        },
      }),
    );
  });
  it("offers an update at startup and installs only after an explicit click", async () => {
    const install = vi.fn(async (progress: (event: { event: string }) => void) => {
      progress({ event: "Progress" });
    });
    updateCheck.mockResolvedValue({ version: "1.2.3", downloadAndInstall: install });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /^Settings$/ }));
    const button = await screen.findByRole("button", { name: "Install update 1.2.3" });
    expect(install).not.toHaveBeenCalled();
    fireEvent.click(button);
    await screen.findByText("Update installed. Close and reopen AriadShift.");
    expect(install).toHaveBeenCalledTimes(1);
  });
  it("keeps conversion available offline and reports a manual update check error", async () => {
    updateCheck.mockRejectedValue(new Error("Update server is offline"));
    render(<App />);
    await waitFor(() => expect(updateCheck).toHaveBeenCalledTimes(1));
    expect(screen.getByLabelText("File path")).toBeTruthy();
    expect(screen.queryByText("Update server is offline")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /^Settings$/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Check for updates" }));
    await screen.findByText("Update server is offline");
  });
  it("reports signature refusal without claiming installation succeeded", async () => {
    updateCheck.mockResolvedValue({
      version: "1.2.3",
      downloadAndInstall: vi.fn().mockRejectedValue(new Error("Signature verification failed")),
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /^Settings$/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Install update 1.2.3" }));
    await screen.findByText("Signature verification failed");
    expect(screen.queryByText("Update installed. Close and reopen AriadShift.")).toBeNull();
    expect(screen.getByRole("button", { name: "Install update 1.2.3" })).toBeTruthy();
  });
  it("queues the selected file with saved defaults and clears selection", async () => {
    render(<App />);
    await waitFor(() => expect(ipc).toHaveBeenCalledWith("snapshot", { request: {} }));
    fireEvent.change(screen.getByLabelText("File path"), { target: { value: "/tmp/document.md" } });
    fireEvent.click(screen.getByRole("button", { name: "Add file" }));
    await screen.findByText("Markdown · 5 bytes");
    fireEvent.click(screen.getByRole("button", { name: "Start conversion" }));
    await waitFor(() =>
      expect(ipc).toHaveBeenCalledWith("enqueue", {
        request: {
          paths: ["/tmp/document.md"],
          target: "html",
          profile: "private",
          output_folder: null,
          overwrite: false,
        },
      }),
    );
    await screen.findByRole("heading", { name: "Conversion queue" });
  });
  it("keeps selection and displays a native queue error", async () => {
    render(<App />);
    await waitFor(() => expect(ipc).toHaveBeenCalled());
    fireEvent.change(screen.getByLabelText("File path"), { target: { value: "/missing.md" } });
    fireEvent.click(screen.getByRole("button", { name: "Add file" }));
    await screen.findByText("Markdown · 5 bytes");
    ipc.mockImplementation(async (name) => {
      if (name === "enqueue") throw new Error("Choose a regular file");
      if (name === "snapshot")
        return { jobs: [], watches: [], settings, pack_progress: "", pack_busy: false };
      return [];
    });
    fireEvent.click(screen.getByRole("button", { name: "Start conversion" }));
    await screen.findByText("Choose a regular file");
    expect(screen.getByText("missing.md")).toBeTruthy();
  });
  it("persists telemetry as a local opt-in preference only", async () => {
    render(<App />);
    await waitFor(() => expect(ipc).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: /^Settings$/ }));
    fireEvent.click(await screen.findByLabelText("Opt in to telemetry"));
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await waitFor(() =>
      expect(ipc).toHaveBeenCalledWith("settings", { request: { ...settings, telemetry: true } }),
    );
  });
  it("saves the watch form's route and policies independently of Convert", async () => {
    render(<App />);
    await waitFor(() => expect(ipc).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "Watch folders" }));
    fireEvent.change(screen.getByLabelText("Input folder"), { target: { value: "/tmp/incoming" } });
    fireEvent.change(screen.getByLabelText("Output folder"), {
      target: { value: "/tmp/converted" },
    });
    fireEvent.change(screen.getByLabelText("Input format"), { target: { value: "pdf" } });
    fireEvent.change(screen.getByLabelText("Output format"), { target: { value: "docx" } });
    fireEvent.change(screen.getByLabelText("Profile"), { target: { value: "faithful" } });
    fireEvent.change(screen.getByLabelText("Polling interval (seconds)"), {
      target: { value: "3" },
    });
    fireEvent.click(screen.getByLabelText("Include subfolders"));
    fireEvent.click(screen.getByLabelText("Replace previous watch outputs"));
    fireEvent.click(screen.getByRole("button", { name: "Start watching" }));
    await waitFor(() =>
      expect(ipc).toHaveBeenCalledWith("watch", {
        request: {
          input: "/tmp/incoming",
          output: "/tmp/converted",
          from: "pdf",
          target: "docx",
          profile: "faithful",
          overwrite: true,
          interval_seconds: 3,
          recursive: true,
        },
      }),
    );
  });
});
