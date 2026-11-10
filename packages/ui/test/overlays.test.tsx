import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "../src/components/dialog";
import { Popover, PopoverClose, PopoverContent, PopoverTrigger } from "../src/components/popover";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "../src/components/tooltip";

function Example({ onOpenChange }: { onOpenChange?: (open: boolean) => void }) {
  return (
    <Dialog onOpenChange={onOpenChange}>
      <DialogTrigger>Open</DialogTrigger>
      <DialogContent closeLabel="Close dialog">
        <DialogHeader>
          <DialogTitle>Remove pages</DialogTitle>
          <DialogDescription>This cannot be undone.</DialogDescription>
        </DialogHeader>
        <input aria-label="Reason" />
        <DialogFooter>
          <DialogClose>Cancel</DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Controlled() {
  const [open, setOpen] = useState(true);
  return (
    <>
      <button type="button" onClick={() => setOpen(false)}>
        External close
      </button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent showClose={false}>
          <DialogTitle>Controlled</DialogTitle>
        </DialogContent>
      </Dialog>
    </>
  );
}

describe("Dialog", () => {
  it("opens as a modal named and described by its title and description", async () => {
    render(<Example />);
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    const dialog = await screen.findByRole("dialog", { name: "Remove pages" });
    expect(dialog.getAttribute("aria-modal") ?? "true").toBe("true");
    expect(dialog.getAttribute("aria-describedby")).toBeTruthy();
    expect(screen.getByText("This cannot be undone.")).toBeTruthy();
  });

  it("moves focus into the dialog on open", async () => {
    render(<Example />);
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    const dialog = await screen.findByRole("dialog");
    await waitFor(() => expect(dialog.contains(document.activeElement)).toBe(true));
  });

  it("closes on Escape", async () => {
    const onOpenChange = vi.fn();
    render(<Example onOpenChange={onOpenChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(onOpenChange).toHaveBeenLastCalledWith(false, expect.anything());
  });

  it("closes from the footer close button and the corner button", async () => {
    render(<Example />);
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    fireEvent.click(await screen.findByRole("button", { name: "Close dialog" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("closes on a backdrop press", async () => {
    render(<Example />);
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    await screen.findByRole("dialog");
    const backdrop = document.querySelector('[data-slot="dialog-backdrop"]') as HTMLElement;
    fireEvent.pointerDown(backdrop);
    fireEvent.mouseDown(backdrop);
    fireEvent.pointerUp(backdrop);
    fireEvent.mouseUp(backdrop);
    fireEvent.click(backdrop);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("is controllable", async () => {
    render(<Controlled />);
    expect(await screen.findByRole("dialog", { name: "Controlled" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Close" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "External close", hidden: true }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });
});

function PopoverExample() {
  return (
    <Popover>
      <PopoverTrigger>Info</PopoverTrigger>
      <PopoverContent>
        <p>Runs in your browser</p>
        <PopoverClose>Got it</PopoverClose>
      </PopoverContent>
    </Popover>
  );
}

describe("Popover", () => {
  it("toggles from its trigger and reports expanded state", async () => {
    render(<PopoverExample />);
    const trigger = screen.getByRole("button", { name: "Info" });
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(trigger);
    expect(await screen.findByText("Runs in your browser")).toBeTruthy();
    expect(trigger.getAttribute("aria-expanded")).toBe("true");
  });

  it("closes from its close button and on Escape", async () => {
    render(<PopoverExample />);
    fireEvent.click(screen.getByRole("button", { name: "Info" }));
    fireEvent.click(await screen.findByRole("button", { name: "Got it" }));
    await waitFor(() => expect(screen.queryByText("Runs in your browser")).toBeNull());

    fireEvent.click(screen.getByRole("button", { name: "Info" }));
    const content = await screen.findByText("Runs in your browser");
    fireEvent.keyDown(content, { key: "Escape" });
    await waitFor(() => expect(screen.queryByText("Runs in your browser")).toBeNull());
  });
});

describe("Tooltip", () => {
  it("shows on keyboard focus and hides on blur", async () => {
    render(
      <TooltipProvider delay={0}>
        <Tooltip>
          <TooltipTrigger>Rotate</TooltipTrigger>
          <TooltipContent>Rotate every page</TooltipContent>
        </Tooltip>
      </TooltipProvider>,
    );
    const trigger = screen.getByRole("button", { name: "Rotate" });
    expect(screen.queryByRole("tooltip")).toBeNull();
    fireEvent.focus(trigger);
    const tooltip = await screen.findByRole("tooltip");
    expect(tooltip.textContent).toBe("Rotate every page");
    fireEvent.blur(trigger);
    await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());
  });
});
