import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "../src/app";

const { ipc, links } = vi.hoisted(() => ({
  ipc: vi.fn(),
  links: { current: null as string[] | null },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => null) }));
vi.mock("@tauri-apps/plugin-deep-link", () => ({
  getCurrent: async () => links.current,
  onOpenUrl: async () => () => {},
}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
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
afterEach(cleanup);
describe("desktop workflow", () => {
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
