import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { DropOverlay } from "../src/components/drop-overlay";
import { Kbd, KbdGroup, keyLabel } from "../src/components/kbd";
import { PageThumbnail } from "../src/components/page-thumbnail";

function Controlled() {
  const [selected, setSelected] = useState(false);
  return <PageThumbnail page={2} selected={selected} onSelectedChange={setSelected} />;
}

describe("PageThumbnail", () => {
  it("is a named group with a page number badge", () => {
    const { container } = render(<PageThumbnail page={3} />);
    expect(screen.getByRole("group", { name: "Page 3" })).toBeTruthy();
    expect(container.querySelector('[data-slot="page-thumbnail-badge"]')?.textContent).toBe("3");
  });

  it("toggles selection uncontrolled and reports it", () => {
    const onSelectedChange = vi.fn();
    render(<PageThumbnail page={1} onSelectedChange={onSelectedChange} />);
    const box = screen.getByRole("checkbox", { name: "Select page 1" }) as HTMLInputElement;
    expect(box.checked).toBe(false);
    fireEvent.click(box);
    expect(box.checked).toBe(true);
    expect(onSelectedChange).toHaveBeenLastCalledWith(true);
    expect(screen.getByRole("group").getAttribute("data-selected")).toBe("true");
    fireEvent.click(box);
    expect(screen.getByRole("group").hasAttribute("data-selected")).toBe(false);
  });

  it("starts selected with defaultSelected and follows a controlled value", () => {
    const { rerender } = render(<PageThumbnail page={1} defaultSelected />);
    expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(true);
    rerender(<PageThumbnail page={1} selected={false} onSelectedChange={() => undefined} />);
    expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(false);
    rerender(<Controlled />);
    fireEvent.click(screen.getByRole("checkbox"));
    expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(true);
  });

  it("shows rotate and delete only when handlers are given and calls them", () => {
    const onRotate = vi.fn();
    const onDelete = vi.fn();
    const { rerender } = render(<PageThumbnail page={4} />);
    expect(screen.queryByRole("button", { name: "Rotate page 4" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Delete page 4" })).toBeNull();
    rerender(<PageThumbnail page={4} onRotate={onRotate} onDelete={onDelete} />);
    fireEvent.click(screen.getByRole("button", { name: "Rotate page 4" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete page 4" }));
    expect(onRotate).toHaveBeenCalledOnce();
    expect(onDelete).toHaveBeenCalledOnce();
  });

  it("keeps the quick actions reachable by keyboard and revealed on focus", () => {
    render(<PageThumbnail page={1} onRotate={() => undefined} />);
    const rotate = screen.getByRole("button", { name: "Rotate page 1" });
    expect(rotate.className).toContain("group-focus-within/page:opacity-100");
    rotate.focus();
    expect(document.activeElement).toBe(rotate);
  });

  it.each([
    [0, "rotate(0deg)"],
    [90, "rotate(90deg) scale(0.75)"],
    [180, "rotate(180deg)"],
    [270, "rotate(270deg) scale(0.75)"],
  ] as const)("previews a %s degree rotation", (rotation, transform) => {
    const { container } = render(<PageThumbnail page={1} rotation={rotation} />);
    const page = container.querySelector('[data-slot="page-thumbnail-page"]') as HTMLElement;
    expect(page.style.transform).toBe(transform);
    expect(page.getAttribute("data-rotation")).toBe(String(rotation));
  });

  it("never mirrors the page preview in RTL", () => {
    const { container } = render(
      <div dir="rtl">
        <PageThumbnail page={1} />
      </div>,
    );
    expect(
      container.querySelector('[data-slot="page-thumbnail-preview"]')?.getAttribute("dir"),
    ).toBe("ltr");
  });

  it("renders a drag handle only with handle props and marks the dragging state", () => {
    const onPointerDown = vi.fn();
    const { rerender } = render(<PageThumbnail page={2} />);
    expect(screen.queryByRole("button", { name: "Reorder page 2" })).toBeNull();
    rerender(<PageThumbnail page={2} dragging dragHandleProps={{ onPointerDown }} />);
    fireEvent.pointerDown(screen.getByRole("button", { name: "Reorder page 2" }));
    expect(onPointerDown).toHaveBeenCalledOnce();
    expect(screen.getByRole("group").getAttribute("data-dragging")).toBe("true");
  });

  it("flags blank and scanned pages", () => {
    const { rerender } = render(<PageThumbnail page={1} />);
    expect(screen.queryByText("Blank")).toBeNull();
    expect(screen.queryByText("Scanned")).toBeNull();
    rerender(<PageThumbnail page={1} blank scanned />);
    expect(screen.getByText("Blank")).toBeTruthy();
    expect(screen.getByText("Scanned")).toBeTruthy();
  });

  it("renders an image preview or custom children, and accepts translated labels", () => {
    const { container, rerender } = render(<PageThumbnail page={1} src="/p1.png" />);
    expect(container.querySelector("img")?.getAttribute("src")).toBe("/p1.png");
    rerender(
      <PageThumbnail
        page={5}
        blank
        labels={{ page: (n) => `Trang ${n}`, select: (n) => `Chọn trang ${n}`, blank: "Trống" }}
      >
        <canvas data-testid="canvas" />
      </PageThumbnail>,
    );
    expect(screen.getByTestId("canvas")).toBeTruthy();
    expect(screen.getByRole("group", { name: "Trang 5" })).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: "Chọn trang 5" })).toBeTruthy();
    expect(screen.getByText("Trống")).toBeTruthy();
  });
});

describe("DropOverlay", () => {
  it("renders nothing while inactive", () => {
    const { container } = render(<DropOverlay active={false} />);
    expect(container.firstChild).toBeNull();
  });

  it("announces itself and states that nothing leaves the device", () => {
    render(<DropOverlay active hints={["PDF", "DOCX"]} description="Release to open" />);
    const overlay = screen.getByRole("status");
    expect(overlay.textContent).toContain("Drop files to start");
    expect(overlay.textContent).toContain("On this device · 0 bytes uploaded");
    expect(screen.getByText("PDF")).toBeTruthy();
    expect(screen.getByText("DOCX")).toBeTruthy();
    expect(screen.getByText("Release to open")).toBeTruthy();
  });

  it("lets drag events pass through so the drop target keeps working", () => {
    render(<DropOverlay active />);
    expect(screen.getByRole("status").className).toContain("pointer-events-none");
  });

  it("covers a container by default and the viewport when fullscreen", () => {
    const { rerender } = render(<DropOverlay active />);
    expect(screen.getByRole("status").className).toContain("absolute");
    rerender(<DropOverlay active fullscreen />);
    expect(screen.getByRole("status").className).toContain("fixed");
    expect(screen.getByRole("status").getAttribute("data-fullscreen")).toBe("true");
  });

  it("animates the dashed border with a reduced-motion fallback and allows custom text", () => {
    const { container } = render(
      <DropOverlay active title="Lâche ici" privacyLabel="Données locales" />,
    );
    expect(container.querySelector("rect")?.getAttribute("class")).toContain(
      "motion-reduce:animate-none",
    );
    expect(container.querySelector("rect")?.getAttribute("stroke-dasharray")).toBe("8 4");
    expect(screen.getByText("Lâche ici")).toBeTruthy();
    expect(screen.getByText("Données locales")).toBeTruthy();
  });
});

describe("Kbd", () => {
  it("renders a kbd element", () => {
    render(<Kbd>Esc</Kbd>);
    expect(screen.getByText("Esc").tagName).toBe("KBD");
  });

  it("renders ⌘K on macOS with a spoken name for the symbol", () => {
    const { container } = render(<KbdGroup keys={["mod", "K"]} platform="mac" />);
    expect(container.querySelectorAll("kbd")).toHaveLength(2);
    expect(container.querySelector("kbd")?.textContent).toBe("⌘Command");
    expect(screen.getByText("Command").className).toContain("sr-only");
    expect(screen.getByText("K")).toBeTruthy();
    expect(container.textContent).not.toContain("+");
  });

  it("keeps the combination left-to-right inside RTL pages", () => {
    const { container } = render(<KbdGroup keys={["mod", "K"]} platform="other" />);
    expect(container.firstElementChild?.getAttribute("dir")).toBe("ltr");
  });

  it("renders Ctrl+K elsewhere", () => {
    const { container } = render(<KbdGroup keys={["mod", "K"]} platform="other" />);
    expect(container.textContent).toBe("Ctrl+K");
  });

  it("maps modifiers per platform and leaves unknown keys alone", () => {
    expect(keyLabel("shift", "mac")).toBe("⇧");
    expect(keyLabel("shift", "other")).toBe("Shift");
    expect(keyLabel("alt", "mac")).toBe("⌥");
    expect(keyLabel("F5", "other")).toBe("F5");
  });

  it("detects the platform when not overridden", () => {
    const original = Object.getOwnPropertyDescriptor(Navigator.prototype, "platform");
    Object.defineProperty(Navigator.prototype, "platform", {
      configurable: true,
      get: () => "MacIntel",
    });
    const { container, unmount } = render(<KbdGroup keys={["mod", "K"]} />);
    expect(container.firstElementChild?.getAttribute("data-platform")).toBe("mac");
    unmount();
    Object.defineProperty(Navigator.prototype, "platform", {
      configurable: true,
      get: () => "Linux x86_64",
    });
    const linux = render(<KbdGroup keys={["mod", "K"]} />);
    expect(linux.container.textContent).toBe("Ctrl+K");
    if (original) Object.defineProperty(Navigator.prototype, "platform", original);
  });
});
