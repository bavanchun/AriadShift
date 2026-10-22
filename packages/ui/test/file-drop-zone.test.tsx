import { fireEvent, render, screen } from "@testing-library/react";
import { FileDropZone } from "../src/components/file-drop-zone";

function dataTransfer(files: File[]) {
  return { files, types: ["Files"], dropEffect: "none" };
}

describe("FileDropZone", () => {
  it("passes dropped files on without reading them", () => {
    const onFiles = vi.fn();
    render(<FileDropZone onFiles={onFiles}>Drop here</FileDropZone>);
    const zone = screen.getByRole("group", { name: "File drop area" });
    const file = new File(["# hi"], "notes.md", { type: "text/markdown" });

    fireEvent.dragEnter(zone, { dataTransfer: dataTransfer([file]) });
    expect(zone.dataset.dragging).toBe("true");
    fireEvent.drop(zone, { dataTransfer: dataTransfer([file]) });

    expect(onFiles).toHaveBeenCalledWith([file]);
    expect(zone.dataset.dragging).toBeUndefined();
  });

  it("offers a focusable picker button that opens the file input", () => {
    const onFiles = vi.fn();
    const { container } = render(<FileDropZone onFiles={onFiles} buttonLabel="Pick" />);
    const input = container.querySelector("input[type=file]") as HTMLInputElement;
    const click = vi.spyOn(input, "click");
    const button = screen.getByRole("button", { name: "Pick" });

    button.focus();
    expect(button).toBe(document.activeElement);
    fireEvent.click(button);
    expect(click).toHaveBeenCalled();

    const file = new File(["<p>x</p>"], "page.html");
    fireEvent.change(input, { target: { files: [file] } });
    expect(onFiles).toHaveBeenCalledWith([file]);
  });

  it("ignores drops while disabled", () => {
    const onFiles = vi.fn();
    render(
      <FileDropZone onFiles={onFiles} disabled>
        Busy
      </FileDropZone>,
    );
    const zone = screen.getByRole("group", { name: "File drop area" });
    fireEvent.drop(zone, { dataTransfer: dataTransfer([new File(["x"], "x.md")]) });
    expect(onFiles).not.toHaveBeenCalled();
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true);
  });
});
