import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
} from "../src/components/command";

function Palette(props: {
  onSelect?: (value: string) => void;
  loop?: boolean;
  search?: string;
  onSearchChange?: (search: string) => void;
}) {
  const { onSelect, ...rest } = props;
  return (
    <Command {...rest}>
      <CommandInput placeholder="Search" />
      <CommandList>
        <CommandEmpty>No results</CommandEmpty>
        <CommandGroup heading="Tools">
          <CommandItem onSelect={onSelect} keywords={["combine"]}>
            Merge PDFs
            <CommandShortcut>⌘M</CommandShortcut>
          </CommandItem>
          <CommandItem onSelect={onSelect}>Split PDF</CommandItem>
          <CommandItem disabled onSelect={onSelect}>
            Sign
          </CommandItem>
        </CommandGroup>
        <CommandSeparator />
        <CommandGroup heading="Recipes">
          <CommandItem value="scan" onSelect={onSelect}>
            Scan to searchable PDF
          </CommandItem>
        </CommandGroup>
      </CommandList>
    </Command>
  );
}

const input = () => screen.getByRole("combobox");
const selected = () =>
  screen.getAllByRole("option").filter((o) => o.getAttribute("aria-selected") === "true");

function Controlled() {
  const [search, setSearch] = useState("split");
  return <Palette search={search} onSearchChange={setSearch} />;
}

function Dynamic() {
  const [extra, setExtra] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setExtra(true)}>
        Add
      </button>
      <Command>
        <CommandInput />
        <CommandList>
          <CommandItem>Alpha</CommandItem>
          {extra && <CommandItem>Beta</CommandItem>}
        </CommandList>
      </Command>
    </>
  );
}

function Host() {
  const [open, setOpen] = useState(true);
  return (
    <CommandDialog open={open} onOpenChange={setOpen} title="Palette">
      <CommandInput placeholder="Search" />
      <CommandList>
        <CommandItem>Compress</CommandItem>
      </CommandList>
    </CommandDialog>
  );
}

describe("Command", () => {
  it("exposes a combobox controlling a listbox of options and groups", () => {
    render(<Palette />);
    const listbox = screen.getByRole("listbox");
    expect(input().getAttribute("aria-controls")).toBe(listbox.id);
    expect(screen.getAllByRole("option")).toHaveLength(4);
    expect(screen.getAllByRole("group")).toHaveLength(2);
    expect(screen.getByRole("group", { name: "Tools" })).toBeTruthy();
  });

  it("activates the first enabled option and mirrors it in aria-activedescendant", () => {
    render(<Palette />);
    const [first] = selected();
    expect(first?.textContent).toContain("Merge PDFs");
    expect(input().getAttribute("aria-activedescendant")).toBe(first?.id);
  });

  it("moves with ArrowDown and ArrowUp, skipping disabled items, and stops at the ends", () => {
    render(<Palette />);
    fireEvent.keyDown(input(), { key: "ArrowDown" });
    expect(selected()[0]?.textContent).toBe("Split PDF");
    fireEvent.keyDown(input(), { key: "ArrowDown" });
    expect(selected()[0]?.textContent).toBe("Scan to searchable PDF");
    fireEvent.keyDown(input(), { key: "ArrowDown" });
    expect(selected()[0]?.textContent).toBe("Scan to searchable PDF");
    fireEvent.keyDown(input(), { key: "ArrowUp" });
    expect(selected()[0]?.textContent).toBe("Split PDF");
  });

  it("wraps around with `loop`", () => {
    render(<Palette loop />);
    fireEvent.keyDown(input(), { key: "ArrowUp" });
    expect(selected()[0]?.textContent).toBe("Scan to searchable PDF");
    fireEvent.keyDown(input(), { key: "ArrowDown" });
    expect(selected()[0]?.textContent).toContain("Merge PDFs");
  });

  it("jumps with Home and End", () => {
    render(<Palette />);
    fireEvent.keyDown(input(), { key: "End" });
    expect(selected()[0]?.textContent).toBe("Scan to searchable PDF");
    fireEvent.keyDown(input(), { key: "Home" });
    expect(selected()[0]?.textContent).toContain("Merge PDFs");
  });

  it("runs the active item on Enter with its value", () => {
    const onSelect = vi.fn();
    render(<Palette onSelect={onSelect} />);
    fireEvent.keyDown(input(), { key: "ArrowDown" });
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(onSelect).toHaveBeenCalledWith("Split PDF");
    fireEvent.keyDown(input(), { key: "End" });
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(onSelect).toHaveBeenLastCalledWith("scan");
  });

  it("leaves shortcut hints out of an item's value", () => {
    const onSelect = vi.fn();
    render(<Palette onSelect={onSelect} />);
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(onSelect).toHaveBeenCalledWith("Merge PDFs");
    fireEvent.change(input(), { target: { value: "⌘M" } });
    expect(screen.queryAllByRole("option")).toHaveLength(0);
  });

  it("runs an item on click but never a disabled one", () => {
    const onSelect = vi.fn();
    render(<Palette onSelect={onSelect} />);
    fireEvent.click(screen.getByRole("option", { name: "Sign" }));
    expect(onSelect).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("option", { name: /Split PDF/ }));
    expect(onSelect).toHaveBeenCalledWith("Split PDF");
  });

  it("follows the pointer", () => {
    render(<Palette />);
    fireEvent.pointerMove(screen.getByRole("option", { name: /Split PDF/ }));
    expect(selected()[0]?.textContent).toBe("Split PDF");
  });

  it("filters by text and keywords, hides empty groups and separators, and keeps the first match active", () => {
    render(<Palette />);
    fireEvent.change(input(), { target: { value: "combine" } });
    expect(screen.getAllByRole("option")).toHaveLength(1);
    expect(screen.getByRole("option").textContent).toContain("Merge PDFs");
    expect(screen.queryByRole("group", { name: "Recipes" })).toBeNull();
    expect(screen.queryByRole("separator")).toBeNull();
    expect(selected()).toHaveLength(1);

    fireEvent.change(input(), { target: { value: "scan" } });
    expect(screen.getByRole("option").textContent).toBe("Scan to searchable PDF");
    expect(selected()[0]?.textContent).toBe("Scan to searchable PDF");
  });

  it("shows the empty state when nothing matches and restores items when cleared", () => {
    render(<Palette />);
    expect(screen.queryByText("No results")).toBeNull();
    fireEvent.change(input(), { target: { value: "zzz" } });
    expect(screen.getByText("No results")).toBeTruthy();
    expect(screen.queryAllByRole("option")).toHaveLength(0);
    fireEvent.change(input(), { target: { value: "" } });
    expect(screen.queryByText("No results")).toBeNull();
    expect(screen.getAllByRole("option")).toHaveLength(4);
  });

  it("does not run anything on Enter when no item matches", () => {
    const onSelect = vi.fn();
    render(<Palette onSelect={onSelect} />);
    fireEvent.change(input(), { target: { value: "zzz" } });
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("supports a controlled search", () => {
    render(<Controlled />);
    expect((input() as HTMLInputElement).value).toBe("split");
    expect(screen.getAllByRole("option")).toHaveLength(1);
    fireEvent.change(input(), { target: { value: "merge" } });
    expect(screen.getByRole("option").textContent).toContain("Merge PDFs");
  });

  it("matches items added after the first render", async () => {
    render(<Dynamic />);
    act(() => screen.getByRole("button", { name: "Add" }).click());
    fireEvent.change(input(), { target: { value: "bet" } });
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(1));
  });

  it("throws a clear error outside <Command>", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => undefined);
    expect(() => render(<CommandInput />)).toThrow(/inside <Command>/);
    spy.mockRestore();
  });
});

describe("CommandDialog", () => {
  it("renders the palette in a named dialog and closes on Escape", async () => {
    render(<Host />);
    const dialog = await screen.findByRole("dialog", { name: "Palette" });
    expect(dialog.querySelector('[role="listbox"]')).toBeTruthy();
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });
});
