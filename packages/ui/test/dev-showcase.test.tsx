import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { UiDevShowcase } from "../src/components/dev-showcase";

const root = (container: HTMLElement) =>
  container.querySelector('[data-slot="ui-dev-showcase"]') as HTMLElement;

describe("UiDevShowcase", () => {
  it("renders every component family", () => {
    const { container } = render(<UiDevShowcase />);
    const sections = Array.from(container.querySelectorAll("[data-showcase-section]")).map((el) =>
      el.getAttribute("data-showcase-section"),
    );
    expect(sections).toEqual([
      "Buttons, badges and keys",
      "Dialog, popover and tooltip",
      "Tabs and toggle group",
      "Slider and progress",
      "Command palette",
      "Toast",
      "Skeleton",
      "Page thumbnails",
      "Drop overlay",
      "Status",
    ]);
    for (const slot of [
      "dialog-trigger",
      "popover-trigger",
      "tooltip-trigger",
      "tabs",
      "toggle-group",
      "slider",
      "progress",
      "command",
      "skeleton",
      "page-thumbnail",
      "kbd",
    ]) {
      expect(container.querySelector(`[data-slot="${slot}"]`), slot).toBeTruthy();
    }
  });

  it("starts in the requested theme and direction", () => {
    const { container } = render(<UiDevShowcase defaultTheme="dark" defaultDirection="rtl" />);
    const el = root(container);
    expect(el.classList.contains("dark")).toBe(true);
    expect(el.getAttribute("dir")).toBe("rtl");
    expect(el.getAttribute("data-theme")).toBe("dark");
  });

  it("switches theme and direction locally without touching <html>", () => {
    const { container } = render(<UiDevShowcase />);
    const el = root(container);
    expect(el.classList.contains("dark")).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Dark" }));
    expect(el.classList.contains("dark")).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "RTL" }));
    expect(el.getAttribute("dir")).toBe("rtl");
    fireEvent.click(screen.getByRole("button", { name: "Light" }));
    expect(el.classList.contains("dark")).toBe(false);
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    expect(document.documentElement.hasAttribute("dir")).toBe(false);
  });

  it("makes the showcase tabs follow the direction for arrow keys", async () => {
    render(<UiDevShowcase defaultDirection="rtl" />);
    const merge = screen.getByRole("tab", { name: "Merge" });
    merge.focus();
    fireEvent.keyDown(merge, { key: "ArrowLeft" });
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("tab", { name: "Split" })),
    );
  });

  it("opens the dialog inside the themed root so it inherits theme and direction", async () => {
    const { container } = render(<UiDevShowcase defaultTheme="dark" defaultDirection="rtl" />);
    fireEvent.click(screen.getByRole("button", { name: "Open dialog" }));
    const dialog = await screen.findByRole("dialog", { name: "Remove 3 pages?" });
    expect(root(container).contains(dialog)).toBe(true);
  });

  it("raises toasts of each status from the demo buttons", () => {
    render(<UiDevShowcase />);
    fireEvent.click(screen.getByRole("button", { name: "Success toast" }));
    expect(screen.getByText("Compressed to 1.2 MB")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Destructive toast" }));
    expect(
      screen.getByText("Password required").closest('[data-slot="toast"]')?.getAttribute("role"),
    ).toBe("alert");
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(screen.queryByText("Password required")).toBeNull();
  });

  it("toggles the drop overlay and opens the command palette dialog", async () => {
    render(<UiDevShowcase />);
    expect(screen.queryByText("On this device · 0 bytes uploaded")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Toggle overlay" }));
    expect(screen.getByText("On this device · 0 bytes uploaded")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Open palette dialog" }));
    expect(await screen.findByRole("dialog", { name: "Command palette" })).toBeTruthy();
  });

  it("rotates the first page thumbnail", () => {
    const { container } = render(<UiDevShowcase />);
    fireEvent.click(screen.getByRole("button", { name: "Rotate page 1" }));
    const page = container.querySelector('[data-slot="page-thumbnail-page"]') as HTMLElement;
    expect(page.getAttribute("data-rotation")).toBe("90");
  });

  it("runs the palette from the keyboard", () => {
    render(<UiDevShowcase />);
    const input = screen.getAllByRole("combobox")[0] as HTMLElement;
    fireEvent.keyDown(input, { key: "Enter" });
    expect(screen.getByText("Last command: Merge PDFs")).toBeTruthy();
  });
});
