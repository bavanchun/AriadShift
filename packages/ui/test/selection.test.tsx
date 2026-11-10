import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Slider } from "../src/components/slider";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../src/components/tabs";
import { ToggleGroup, ToggleGroupItem } from "../src/components/toggle-group";
import { DirectionProvider } from "../src/lib/direction";

function ToolTabs(props: { dir?: "ltr" | "rtl"; onValueChange?: (value: unknown) => void }) {
  return (
    <Tabs defaultValue="merge" {...props}>
      <TabsList aria-label="Tools">
        <TabsTrigger value="merge">Merge</TabsTrigger>
        <TabsTrigger value="split">Split</TabsTrigger>
        <TabsTrigger value="compress">Compress</TabsTrigger>
      </TabsList>
      <TabsContent value="merge">Merge panel</TabsContent>
      <TabsContent value="split">Split panel</TabsContent>
      <TabsContent value="compress">Compress panel</TabsContent>
    </Tabs>
  );
}

describe("Tabs", () => {
  it("exposes tablist, tab and tabpanel roles with selection state", () => {
    render(<ToolTabs />);
    expect(screen.getByRole("tablist", { name: "Tools" })).toBeTruthy();
    const merge = screen.getByRole("tab", { name: "Merge" });
    expect(merge.getAttribute("aria-selected")).toBe("true");
    expect(screen.getByRole("tab", { name: "Split" }).getAttribute("aria-selected")).toBe("false");
    expect(screen.getByText("Merge panel")).toBeTruthy();
  });

  it("moves with ArrowRight and ArrowLeft in LTR, with Home and End at the ends", async () => {
    render(<ToolTabs />);
    const merge = screen.getByRole("tab", { name: "Merge" });
    merge.focus();
    fireEvent.keyDown(merge, { key: "ArrowRight" });
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("tab", { name: "Split" })),
    );
    fireEvent.keyDown(document.activeElement as Element, { key: "End" });
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("tab", { name: "Compress" })),
    );
    fireEvent.keyDown(document.activeElement as Element, { key: "ArrowLeft" });
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("tab", { name: "Split" })),
    );
    fireEvent.keyDown(document.activeElement as Element, { key: "Home" });
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("tab", { name: "Merge" })),
    );
  });

  it("reverses the arrow keys in RTL", async () => {
    render(<ToolTabs dir="rtl" />);
    const merge = screen.getByRole("tab", { name: "Merge" });
    merge.focus();
    fireEvent.keyDown(merge, { key: "ArrowLeft" });
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("tab", { name: "Split" })),
    );
    fireEvent.keyDown(document.activeElement as Element, { key: "ArrowRight" });
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("tab", { name: "Merge" })),
    );
  });

  it("follows a surrounding DirectionProvider", async () => {
    render(
      <DirectionProvider direction="rtl">
        <ToolTabs />
      </DirectionProvider>,
    );
    const merge = screen.getByRole("tab", { name: "Merge" });
    merge.focus();
    fireEvent.keyDown(merge, { key: "ArrowLeft" });
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("tab", { name: "Split" })),
    );
  });

  it("activates a tab on click and switches the panel", () => {
    const onValueChange = vi.fn();
    render(<ToolTabs onValueChange={onValueChange} />);
    fireEvent.click(screen.getByRole("tab", { name: "Compress" }));
    expect(onValueChange).toHaveBeenCalledWith("compress", expect.anything());
    expect(screen.getByText("Compress panel")).toBeTruthy();
  });
});

describe("ToggleGroup", () => {
  it("toggles one item at a time by default", () => {
    render(
      <ToggleGroup aria-label="Zoom" defaultValue={["fit"]}>
        <ToggleGroupItem value="fit">Fit</ToggleGroupItem>
        <ToggleGroupItem value="width">Width</ToggleGroupItem>
      </ToggleGroup>,
    );
    const fit = screen.getByRole("button", { name: "Fit" });
    const width = screen.getByRole("button", { name: "Width" });
    expect(fit.getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(width);
    expect(width.getAttribute("aria-pressed")).toBe("true");
    expect(fit.getAttribute("aria-pressed")).toBe("false");
  });

  it("allows several pressed items with `multiple`", () => {
    const onValueChange = vi.fn();
    render(
      <ToggleGroup multiple aria-label="Format" onValueChange={onValueChange}>
        <ToggleGroupItem value="b">Bold</ToggleGroupItem>
        <ToggleGroupItem value="i">Italic</ToggleGroupItem>
      </ToggleGroup>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Bold" }));
    fireEvent.click(screen.getByRole("button", { name: "Italic" }));
    expect(onValueChange).toHaveBeenLastCalledWith(["b", "i"], expect.anything());
    expect(screen.getByRole("button", { name: "Bold" }).getAttribute("aria-pressed")).toBe("true");
  });

  it("is controllable and does not change without the parent", () => {
    render(
      <ToggleGroup value={["fit"]} onValueChange={() => undefined}>
        <ToggleGroupItem value="fit">Fit</ToggleGroupItem>
        <ToggleGroupItem value="width">Width</ToggleGroupItem>
      </ToggleGroup>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Width" }));
    expect(screen.getByRole("button", { name: "Fit" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByRole("button", { name: "Width" }).getAttribute("aria-pressed")).toBe(
      "false",
    );
  });

  it("does not toggle a disabled item", () => {
    render(
      <ToggleGroup>
        <ToggleGroupItem value="a" disabled>
          A
        </ToggleGroupItem>
      </ToggleGroup>,
    );
    fireEvent.click(screen.getByRole("button", { name: "A" }));
    expect(screen.getByRole("button", { name: "A" }).getAttribute("aria-pressed")).toBe("false");
  });
});

describe("Slider", () => {
  it("exposes slider semantics with min, max and value", () => {
    render(<Slider defaultValue={30} min={10} max={50} step={5} thumbLabel="Quality" />);
    const thumb = screen.getByLabelText("Quality");
    expect(thumb.getAttribute("min")).toBe("10");
    expect(thumb.getAttribute("max")).toBe("50");
    expect(thumb.getAttribute("step")).toBe("5");
    expect(thumb.getAttribute("aria-valuenow")).toBe("30");
  });

  it("steps with the arrow keys and jumps with Home and End", () => {
    const onValueChange = vi.fn();
    render(
      <Slider
        defaultValue={30}
        min={10}
        max={50}
        step={5}
        thumbLabel="Quality"
        onValueChange={onValueChange}
      />,
    );
    const thumb = screen.getByLabelText("Quality");
    thumb.focus();
    fireEvent.keyDown(thumb, { key: "ArrowRight" });
    expect(onValueChange).toHaveBeenLastCalledWith(35, expect.anything());
    fireEvent.keyDown(thumb, { key: "ArrowLeft" });
    fireEvent.keyDown(thumb, { key: "ArrowLeft" });
    expect(onValueChange).toHaveBeenLastCalledWith(25, expect.anything());
    fireEvent.keyDown(thumb, { key: "End" });
    expect(onValueChange).toHaveBeenLastCalledWith(50, expect.anything());
    fireEvent.keyDown(thumb, { key: "Home" });
    expect(onValueChange).toHaveBeenLastCalledWith(10, expect.anything());
  });

  it("renders one labelled thumb per range value", () => {
    render(<Slider defaultValue={[20, 70]} thumbLabel={["Start", "End"]} />);
    expect(screen.getByLabelText("Start").getAttribute("aria-valuenow")).toBe("20");
    expect(screen.getByLabelText("End").getAttribute("aria-valuenow")).toBe("70");
  });

  it("shows the value label and follows a controlled value", () => {
    const { rerender } = render(
      <Slider value={40} onValueChange={() => undefined} showValue thumbLabel="Quality" />,
    );
    expect(screen.getByText("40")).toBeTruthy();
    rerender(<Slider value={65} onValueChange={() => undefined} showValue thumbLabel="Quality" />);
    expect(screen.getByText("65")).toBeTruthy();
  });

  it("disables the thumb and marks the root while disabled", () => {
    const { container } = render(<Slider defaultValue={30} disabled thumbLabel="Quality" />);
    expect((screen.getByLabelText("Quality") as HTMLInputElement).disabled).toBe(true);
    expect(container.querySelector('[data-slot="slider"]')?.hasAttribute("data-disabled")).toBe(
      true,
    );
  });
});
