// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { FileTray } from "@/components/workspace/file-tray";
import { pdfFile, workspaceFile } from "./workspace-fixtures";

afterEach(cleanup);

const props = () => ({
  activeId: null,
  onAddFiles: vi.fn(),
  onRemove: vi.fn(),
  onMove: vi.fn(),
  onSelect: vi.fn(),
});

function dragEvent(type: string, files: File[]) {
  const event = new Event(type, { cancelable: true });
  Object.assign(event, { dataTransfer: { types: ["Files"], files } });
  return event;
}

describe("<FileTray />", () => {
  it("starts as a drop zone and always states that nothing is uploaded", () => {
    const actions = props();
    const { container } = render(<FileTray files={[]} {...actions} />);
    expect(screen.getByText(/Nothing is uploaded/)).toBeTruthy();
    expect(screen.getByText("On this device · 0 bytes uploaded")).toBeTruthy();
    const input = container.querySelector("input[type=file]") as HTMLInputElement;
    const file = pdfFile("a.pdf");
    fireEvent.change(input, { target: { files: [file] } });
    expect(actions.onAddFiles).toHaveBeenCalledWith([file]);
  });

  it("lists each file with its size and page count, and keeps the privacy line", () => {
    render(
      <FileTray
        files={[
          workspaceFile("a.pdf", { size: 1536, pageCount: 12 }),
          workspaceFile("b.docx", { size: 100, pageCount: null }),
        ]}
        storage="opfs"
        {...props()}
      />,
    );
    const rows = within(screen.getByRole("list", { name: "Files in this workspace" })).getAllByRole(
      "listitem",
    );
    expect(rows).toHaveLength(2);
    expect(rows[0]?.textContent).toContain("1.5 KB · 12 pages");
    expect(rows[1]?.textContent).toContain("100 bytes");
    expect(rows[1]?.textContent).not.toContain("pages");
    expect(screen.getByText("On this device · 0 bytes uploaded")).toBeTruthy();
    expect(screen.getByText(/Stored in browser file storage/)).toBeTruthy();
  });

  it("selects, reorders and removes files, disabling moves at the ends", () => {
    const actions = props();
    const a = workspaceFile("a.pdf");
    const b = workspaceFile("b.pdf");
    render(<FileTray files={[a, b]} {...actions} activeId={a.id} />);
    expect(
      (screen.getByRole("button", { name: "Move a.pdf up" }) as HTMLButtonElement).disabled,
    ).toBe(true);
    expect(
      (screen.getByRole("button", { name: "Move b.pdf down" }) as HTMLButtonElement).disabled,
    ).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Move a.pdf down" }));
    expect(actions.onMove).toHaveBeenCalledWith(a.id, 1);
    fireEvent.click(screen.getByRole("button", { name: "Work on b.pdf" }));
    expect(actions.onSelect).toHaveBeenCalledWith(b.id);
    fireEvent.click(screen.getByRole("button", { name: "Remove a.pdf" }));
    expect(actions.onRemove).toHaveBeenCalledWith(a.id);
    expect(screen.getByRole("button", { name: "Work on a.pdf" }).getAttribute("aria-current")).toBe(
      "true",
    );
  });

  it("adds more files from the picker", () => {
    const actions = props();
    const { container } = render(<FileTray files={[workspaceFile("a.pdf")]} {...actions} />);
    const input = container.querySelector("input[type=file]") as HTMLInputElement;
    expect(input.multiple).toBe(true);
    const extra = pdfFile("c.pdf");
    fireEvent.change(input, { target: { files: [extra] } });
    expect(actions.onAddFiles).toHaveBeenCalledWith([extra]);
    expect(screen.getByRole("button", { name: "Add files" })).toBeTruthy();
  });

  it("shows the overlay while files are dragged over the window and adds a dropped file", () => {
    const actions = props();
    render(<FileTray files={[workspaceFile("a.pdf")]} {...actions} />);
    const file = pdfFile("dropped.pdf");
    expect(screen.queryByText("Drop files to add them")).toBeNull();
    act(() => {
      window.dispatchEvent(dragEvent("dragenter", [file]));
    });
    expect(screen.getByText("Drop files to add them")).toBeTruthy();
    act(() => {
      window.dispatchEvent(dragEvent("drop", [file]));
    });
    expect(screen.queryByText("Drop files to add them")).toBeNull();
    expect(actions.onAddFiles).toHaveBeenCalledWith([file]);
  });

  it("ignores drags that carry no files, and does not accept drops while busy", () => {
    const actions = props();
    const { rerender } = render(<FileTray files={[workspaceFile("a.pdf")]} {...actions} />);
    const text = new Event("dragenter", { cancelable: true });
    Object.assign(text, { dataTransfer: { types: ["text/plain"], files: [] } });
    act(() => {
      window.dispatchEvent(text);
    });
    expect(screen.queryByText("Drop files to add them")).toBeNull();
    rerender(<FileTray files={[workspaceFile("a.pdf")]} busy {...actions} />);
    act(() => {
      window.dispatchEvent(dragEvent("drop", [pdfFile("x.pdf")]));
    });
    expect(actions.onAddFiles).not.toHaveBeenCalled();
    expect(screen.getByText("Reading files…")).toBeTruthy();
  });

  it("surfaces a file the converter could not read", () => {
    render(
      <FileTray files={[workspaceFile("bad.bin", { problem: "Unknown format" })]} {...props()} />,
    );
    expect(screen.getByRole("alert").textContent).toBe("Unknown format");
  });
});
