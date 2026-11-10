// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import {
  type EditApplyParams,
  FontSubstitutionAlert,
  type ImageObject,
  NOTO_VIETNAMESE_STACK,
  PageEditSession,
  type PageInfo,
  PageModeEditor,
  type PageObject,
  type PathObject,
  resolveFontForEdit,
  type TextRunObject,
} from "@/editor/page";

afterEach(cleanup);

// A glyph set standing in for the `vi-diacritics-subset-font.pdf` fixture: the subset only keeps
// the unaccented letters the source document used, so Vietnamese diacritics are missing.
const SUBSET_GLYPHS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 .,";
const VIETNAMESE = "Tiếng Việt đầy đủ dấu";

const heading: TextRunObject = {
  kind: "text",
  id: "p1_obj1",
  page: 1,
  bbox: [72, 700, 272, 720],
  text: "Tieng Viet",
  fontName: "ABCDEF+Helvetica",
  fontSize: 18,
  color: "#112233",
  embeddedGlyphs: SUBSET_GLYPHS,
  isSubset: true,
};
const body: TextRunObject = {
  kind: "text",
  id: "p1_obj2",
  page: 1,
  bbox: [72, 600, 372, 616],
  text: "Plain body text",
  fontName: "Times-Roman",
  fontSize: 12,
  color: "#000000",
  isSubset: false,
};
const logo: ImageObject = {
  kind: "image",
  id: "p1_obj3",
  page: 1,
  bbox: [72, 400, 172, 460],
  width: 200,
  height: 120,
  dataUrl: "data:image/png;base64,AAAA",
};
const rule: PathObject = {
  kind: "path",
  id: "p1_obj4",
  page: 1,
  bbox: [72, 380, 372, 382],
  strokeColor: "#000000",
  fillColor: "#000000",
};
const second: TextRunObject = { ...body, id: "p2_obj1", page: 2, text: "Second page" };

const PAGES: PageInfo[] = [
  { page: 1, widthPt: 612, heightPt: 792 },
  { page: 2, widthPt: 612, heightPt: 792 },
];
const OBJECTS: PageObject[] = [heading, body, logo, rule, second];

function open(extra: Partial<React.ComponentProps<typeof PageModeEditor>> = {}) {
  const onApply = vi.fn<(params: EditApplyParams) => void>();
  const view = render(
    <PageModeEditor pages={PAGES} objects={OBJECTS} onApply={onApply} {...extra} />,
  );
  return { ...view, onApply };
}

const objectButton = (name: RegExp | string) => screen.getByRole("button", { name });
const readout = (name: string) => screen.getByTestId(`readout-${name}`).textContent;
const lastApplied = (onApply: ReturnType<typeof vi.fn>): EditApplyParams =>
  onApply.mock.calls.at(-1)?.[0] as EditApplyParams;

function pointer(target: Element | Window, type: string, x: number, y: number) {
  fireEvent(target, new MouseEvent(type, { bubbles: true, clientX: x, clientY: y, button: 0 }));
}

describe("resolveFontForEdit", () => {
  it("keeps the embedded font when the font is not a subset", () => {
    const result = resolveFontForEdit(body, VIETNAMESE);
    expect(result.tier).toBe("embedded-subset");
    expect(result.substituted).toBe(false);
    expect(result.missingChars).toEqual([]);
  });

  it("keeps the embedded subset while every glyph is still covered", () => {
    const result = resolveFontForEdit(heading, "Viet Tieng 2026");
    expect(result.tier).toBe("embedded-subset");
    expect(result.substituted).toBe(false);
    expect(result.resolvedFont).toBe("ABCDEF+Helvetica");
  });

  it("falls back to a matching system family for missing Latin-1 characters", () => {
    const result = resolveFontForEdit(heading, "Café");
    expect(result.tier).toBe("system-match");
    expect(result.resolvedFont).toBe("Liberation Sans");
    expect(result.missingChars).toEqual(["é"]);
    expect(result.substituted).toBe(true);

    const serif = resolveFontForEdit({ ...heading, fontName: "XYZABC+Times-Roman" }, "Café");
    expect(serif.resolvedFont).toBe("Liberation Serif");
    const mono = resolveFontForEdit({ ...heading, fontName: "XYZABC+Courier" }, "Café");
    expect(mono.resolvedFont).toBe("Liberation Mono");
  });

  it("falls back to Noto Sans for Vietnamese diacritics missing from the subset", () => {
    const result = resolveFontForEdit(heading, VIETNAMESE);
    expect(result.tier).toBe("noto-vietnamese");
    expect(result.resolvedFont).toBe(NOTO_VIETNAMESE_STACK);
    expect(result.resolvedLabel).toBe("Noto Sans (Vietnamese coverage)");
    expect(result.originalLabel).toBe("Subset Helvetica");
    expect(result.substituted).toBe(true);
    expect(result.missingChars).toEqual(expect.arrayContaining(["ế", "ệ", "đ", "ầ", "ủ", "ấ"]));
    expect(result.missingChars).not.toContain(" ");
    expect(new Set(result.missingChars).size).toBe(result.missingChars.length);
  });

  it("normalises decomposed Vietnamese to NFC before checking coverage", () => {
    const decomposed = "Tiếng"; // e + circumflex + acute
    const result = resolveFontForEdit(heading, decomposed);
    expect(result.text).toBe("Tiếng");
    expect(result.text).toBe(decomposed.normalize("NFC"));
    expect(result.missingChars).toEqual(["ế"]);
  });

  it("treats a subset without a glyph list as covering nothing", () => {
    const result = resolveFontForEdit({ ...heading, embeddedGlyphs: undefined }, "Abc");
    expect(result.substituted).toBe(true);
  });
});

describe("PageEditSession", () => {
  it("edits text in place with NFC normalisation and ignores no-op edits", () => {
    const session = new PageEditSession(OBJECTS);
    expect(session.editText("p1_obj1", "Tiếng")).toBe(true);
    expect((session.get("p1_obj1") as TextRunObject).text).toBe("Tiếng");
    expect(session.editText("p1_obj1", "Tiếng")).toBe(false);
    expect(session.editText("p1_obj1", "")).toBe(false);
    expect(session.editText("p1_obj3", "not text")).toBe(false);
    expect(session.canUndo).toBe(true);
  });

  it("moves, resizes, restyles and deletes objects", () => {
    const session = new PageEditSession(OBJECTS);
    session.moveObject("p1_obj3", 10, -5);
    expect(session.get("p1_obj3")?.bbox).toEqual([82, 395, 182, 455]);

    session.resizeObject("p1_obj3", [82, 395, 232, 455]);
    expect(session.get("p1_obj3")?.bbox).toEqual([82, 395, 232, 455]);

    // Text scales uniformly: doubling the height doubles the size and the width.
    session.resizeObject("p1_obj2", [72, 600, 372, 632]);
    const scaled = session.get("p1_obj2") as TextRunObject;
    expect(scaled.fontSize).toBe(24);
    expect(scaled.bbox).toEqual([72, 600, 672, 632]);

    session.editTextStyle("p1_obj1", { fontSize: 20, color: "#ff0000", fontName: "Noto Sans" });
    expect(session.get("p1_obj1")).toMatchObject({
      fontSize: 20,
      color: "#ff0000",
      fontName: "Noto Sans",
    });
    // Invalid values never reach the document.
    expect(session.editTextStyle("p1_obj1", { fontSize: -3, color: "red" })).toBe(false);

    session.deleteObject("p1_obj4");
    expect(session.get("p1_obj4")).toBeUndefined();
    expect(session.deleteObject("missing")).toBe(false);
  });

  it("refuses to resize paths and source images without pixel data", () => {
    const bare: ImageObject = { ...logo, dataUrl: undefined };
    const session = new PageEditSession([rule, bare]);
    expect(session.resizeObject("p1_obj4", [72, 380, 400, 400])).toBe(false);
    expect(session.resizeObject("p1_obj3", [72, 400, 300, 500])).toBe(false);
    expect(session.moveObject("p1_obj4", 5, 0)).toBe(true);
  });

  it("adds text boxes and images", () => {
    const session = new PageEditSession(OBJECTS);
    const textId = session.addTextBox({ page: 1, x: 100, y: 300, text: "Xin chào" });
    expect(textId).toMatch(/^new\d+$/);
    expect(session.get(textId as string)).toMatchObject({ kind: "text", text: "Xin chào" });
    expect(session.addTextBox({ page: 1, x: 0, y: 0, text: "" })).toBeUndefined();

    const imageId = session.addImage({
      page: 1,
      bbox: [10, 10, 110, 60],
      dataUrl: "data:image/png;base64,QUJD",
      width: 100,
      height: 50,
    });
    expect(session.get(imageId as string)).toMatchObject({ kind: "image" });
    expect(
      session.addImage({ page: 1, bbox: [10, 10, 10, 60], dataUrl: "x", width: 1, height: 1 }),
    ).toBeUndefined();
  });

  it("undoes and redoes every operation, and a new edit clears the redo stack", () => {
    const session = new PageEditSession(OBJECTS);
    expect(session.canUndo).toBe(false);
    expect(session.canRedo).toBe(false);

    session.editText("p1_obj2", "Changed");
    session.moveObject("p1_obj2", 5, 5);
    session.deleteObject("p1_obj1");
    expect(session.undo()).toBe(true);
    expect(session.get("p1_obj1")).toBeDefined();
    expect(session.canRedo).toBe(true);
    expect(session.redo()).toBe(true);
    expect(session.get("p1_obj1")).toBeUndefined();

    session.undo();
    session.undo();
    expect((session.get("p1_obj2") as TextRunObject).text).toBe("Changed");
    expect(session.get("p1_obj2")?.bbox).toEqual(body.bbox);

    session.moveObject("p1_obj2", 1, 1);
    expect(session.canRedo).toBe(false);

    session.undo();
    session.undo();
    expect(session.undo()).toBe(false);
    expect(session.hasChanges).toBe(false);
    expect(session.objects).toBe(OBJECTS);
  });

  it("resets every change at once", () => {
    const session = new PageEditSession(OBJECTS);
    session.editText("p1_obj2", "Changed");
    session.deleteObject("p1_obj4");
    session.resetChanges();
    expect(session.objects).toBe(OBJECTS);
    expect(session.canUndo).toBe(false);
    expect(session.canRedo).toBe(false);
    expect(session.toEditPageParams().ops).toEqual([]);
  });

  it("notifies subscribers once per committed change", () => {
    const session = new PageEditSession(OBJECTS);
    const listener = vi.fn();
    const stop = session.subscribe(listener);
    session.moveObject("p1_obj1", 1, 0);
    session.undo();
    session.moveObject("p1_obj1", 0, 0);
    expect(listener).toHaveBeenCalledTimes(2);
    stop();
    session.redo();
    expect(listener).toHaveBeenCalledTimes(2);
  });

  it("serialises the net edits to edit.apply parameters", () => {
    const session = new PageEditSession(OBJECTS);
    session.editText("p1_obj1", VIETNAMESE);
    session.editTextStyle("p1_obj1", { fontSize: 20, color: "#ff0000" });
    session.moveObject("p1_obj1", 4, -2.5);
    session.deleteObject("p1_obj4");
    session.resizeObject("p1_obj3", [72, 400, 272, 520]);
    const noteId = session.addTextBox({
      page: 2,
      x: 50,
      y: 60,
      text: "Ghi chú",
      fontSize: 10,
      color: "#00ff00",
    });
    const imageId = session.addImage({
      page: 1,
      bbox: [10, 20, 110, 70],
      dataUrl: "data:image/png;base64,QUJD",
      width: 100,
      height: 50,
    });
    session.deleteObject(imageId as string);
    expect(noteId).toBeDefined();

    expect(session.toEditPageParams()).toEqual({
      save: "full",
      ops: [
        { op: "replace_text", page: 1, object_id: "p1_obj1", text: VIETNAMESE },
        { op: "set_text_style", page: 1, object_id: "p1_obj1", font_size: 20, color: "#ff0000" },
        { op: "move_object", page: 1, object_id: "p1_obj1", dx_pt: 4, dy_pt: -2.5 },
        // The engine cannot resize an image: the resized copy replaces the original.
        { op: "delete_object", page: 1, object_id: "p1_obj3" },
        { op: "delete_object", page: 1, object_id: "p1_obj4" },
        { op: "add_image", page: 1, bbox: [72, 400, 272, 520], image_base64: "AAAA" },
        {
          op: "add_text",
          page: 2,
          text: "Ghi chú",
          x_pt: 50,
          y_pt: 60,
          font_size: 10,
          color: "#00ff00",
        },
      ],
    });
    expect(session.toEditPageParams("incremental").save).toBe("incremental");
  });

  it("emits no operation for an edit that was reverted by hand", () => {
    const session = new PageEditSession(OBJECTS);
    session.moveObject("p1_obj1", 5, 5);
    session.moveObject("p1_obj1", -5, -5);
    session.editText("p1_obj2", "Other");
    session.editText("p1_obj2", body.text);
    expect(session.toEditPageParams().ops).toEqual([]);
  });

  it("reports the substitution of an edited run and drops it after an explicit font choice", () => {
    const session = new PageEditSession(OBJECTS);
    session.editText("p1_obj1", VIETNAMESE);
    expect(session.substitutions().map((entry) => entry.id)).toEqual(["p1_obj1"]);
    expect(session.substitutionFor("p1_obj2")).toBeUndefined();
    session.editTextStyle("p1_obj1", { fontName: "Noto Sans" });
    expect(session.substitutions()).toEqual([]);
  });
});

describe("<FontSubstitutionAlert />", () => {
  it("states the substituted font and the characters that triggered it", () => {
    render(<FontSubstitutionAlert resolution={resolveFontForEdit(heading, VIETNAMESE)} />);
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("Font substituted");
    expect(alert.textContent).toContain("Subset Helvetica → Noto Sans (Vietnamese coverage)");
    expect(alert.textContent).toContain("ế");
  });

  it("renders nothing when no substitution happened", () => {
    const { container } = render(
      <FontSubstitutionAlert resolution={resolveFontForEdit(body, "Anything")} />,
    );
    expect(container.textContent).toBe("");
  });

  it("accepts localised copy", () => {
    render(
      <FontSubstitutionAlert
        resolution={resolveFontForEdit(heading, VIETNAMESE)}
        labels={{ substitutionTitle: "Đã thay phông chữ" }}
      />,
    );
    expect(screen.getByText("Đã thay phông chữ")).toBeTruthy();
  });
});

describe("<PageModeEditor />", () => {
  it("lists the active page's objects and the toolbar tools", () => {
    open();
    expect(screen.getByRole("toolbar", { name: "Page editing tools" })).toBeTruthy();
    for (const name of [
      "Select and move",
      "Edit text",
      "Add text box",
      "Add image",
      "Delete selected",
      "Undo",
      "Redo",
      "Apply page edits",
    ]) {
      expect(screen.getByRole("button", { name })).toBeTruthy();
    }
    expect(objectButton("Text: Tieng Viet")).toBeTruthy();
    expect(objectButton("Text: Plain body text")).toBeTruthy();
    expect(objectButton("Image: 200×120")).toBeTruthy();
    expect(objectButton("Shape: p1_obj4")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Text: Second page" })).toBeNull();
    expect(screen.getByText("Page 1 of 2")).toBeTruthy();
    expect(
      (screen.getByRole("button", { name: "Apply page edits" }) as HTMLButtonElement).disabled,
    ).toBe(true);
  });

  it("selects an object and shows its live coordinates", () => {
    open();
    expect(screen.getByText(/Select an object on the page/)).toBeTruthy();
    fireEvent.click(objectButton("Image: 200×120"));
    expect(objectButton("Image: 200×120").getAttribute("aria-pressed")).toBe("true");
    expect([readout("x"), readout("y"), readout("w"), readout("h")]).toEqual([
      "72",
      "400",
      "100",
      "60",
    ]);
  });

  it("moves with arrow keys, 10pt with Shift, and resizes with Alt+Arrow", () => {
    const { onApply } = open();
    const button = objectButton("Image: 200×120");
    fireEvent.click(button);
    fireEvent.keyDown(button, { key: "ArrowRight" });
    fireEvent.keyDown(button, { key: "ArrowUp", shiftKey: true });
    expect([readout("x"), readout("y")]).toEqual(["73", "410"]);

    fireEvent.keyDown(button, { key: "ArrowRight", altKey: true, shiftKey: true });
    fireEvent.keyDown(button, { key: "ArrowDown", altKey: true });
    expect([readout("w"), readout("h")]).toEqual(["110", "59"]);

    fireEvent.click(screen.getByRole("button", { name: "Apply page edits" }));
    const { ops } = lastApplied(onApply);
    expect(ops).toContainEqual({ op: "delete_object", page: 1, object_id: "p1_obj3" });
    expect(ops).toContainEqual({
      op: "add_image",
      page: 1,
      bbox: [73, 410, 183, 469],
      image_base64: "AAAA",
    });
  });

  it("does not resize a path but still moves it", () => {
    open();
    const button = objectButton("Shape: p1_obj4");
    fireEvent.click(button);
    fireEvent.keyDown(button, { key: "ArrowRight", altKey: true });
    expect(readout("w")).toBe("300");
    fireEvent.keyDown(button, { key: "ArrowLeft" });
    expect(readout("x")).toBe("71");
  });

  it("moves by dragging, converting screen pixels to PDF points", () => {
    const { onApply } = open({ scale: 2 });
    const button = objectButton("Text: Plain body text");
    pointer(button, "pointerdown", 100, 100);
    pointer(window, "pointermove", 140, 80);
    // 40px right and 20px up at 2px per point: +20pt in x and +10pt in y.
    expect([readout("x"), readout("y")]).toEqual(["92", "610"]);
    pointer(window, "pointerup", 140, 80);
    fireEvent.click(screen.getByRole("button", { name: "Apply page edits" }));
    expect(lastApplied(onApply).ops).toEqual([
      { op: "move_object", page: 1, object_id: "p1_obj2", dx_pt: 20, dy_pt: 10 },
    ]);
  });

  it("resizes an image with its handle", () => {
    const { container, onApply } = open();
    fireEvent.click(objectButton("Image: 200×120"));
    const handle = container.querySelector('[data-slot="resize-handle"]') as HTMLElement;
    pointer(handle, "pointerdown", 200, 200);
    pointer(window, "pointermove", 230, 220);
    pointer(window, "pointerup", 230, 220);
    expect([readout("w"), readout("h")]).toEqual(["130", "80"]);
    fireEvent.click(screen.getByRole("button", { name: "Apply page edits" }));
    expect(lastApplied(onApply).ops).toContainEqual(
      expect.objectContaining({ op: "add_image", bbox: [72, 380, 202, 460] }),
    );
  });

  it("deletes with the Delete key and the toolbar button, and Escape deselects", () => {
    open();
    const deleteButton = screen.getByRole("button", {
      name: "Delete selected",
    }) as HTMLButtonElement;
    expect(deleteButton.disabled).toBe(true);

    const rule = objectButton("Shape: p1_obj4");
    fireEvent.click(rule);
    expect(deleteButton.disabled).toBe(false);
    fireEvent.keyDown(rule, { key: "Delete" });
    expect(screen.queryByRole("button", { name: "Shape: p1_obj4" })).toBeNull();

    const image = objectButton("Image: 200×120");
    fireEvent.click(image);
    fireEvent.click(deleteButton);
    expect(screen.queryByRole("button", { name: "Image: 200×120" })).toBeNull();

    const text = objectButton("Text: Plain body text");
    fireEvent.click(text);
    fireEvent.keyDown(text, { key: "Escape" });
    expect(text.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(text);
    fireEvent.keyDown(text, { key: "Backspace" });
    expect(screen.queryByRole("button", { name: "Text: Plain body text" })).toBeNull();
  });

  it("edits text in place and applies an NFC-normalised replacement", () => {
    const { onApply } = open();
    fireEvent.doubleClick(objectButton("Text: Plain body text"));
    const editor = screen.getByLabelText("Edit text in place") as HTMLTextAreaElement;
    expect(editor.value).toBe("Plain body text");
    fireEvent.change(editor, { target: { value: "Xin chào" } });
    expect(editor.value).toBe("Xin chào");
    fireEvent.keyDown(editor, { key: "Enter" });

    expect(screen.queryByLabelText("Edit text in place")).toBeNull();
    expect(objectButton("Text: Xin chào")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Apply page edits" }));
    expect(lastApplied(onApply)).toEqual({
      save: "full",
      ops: [{ op: "replace_text", page: 1, object_id: "p1_obj2", text: "Xin chào" }],
    });
  });

  it("opens the editor with the Edit text tool and from the keyboard", () => {
    open();
    fireEvent.click(screen.getByRole("button", { name: "Edit text" }));
    fireEvent.click(objectButton("Text: Tieng Viet"));
    expect(screen.getByLabelText("Edit text in place")).toBeTruthy();
    fireEvent.keyDown(screen.getByLabelText("Edit text in place"), { key: "Escape" });
    expect(screen.queryByLabelText("Edit text in place")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Select and move" }));
    fireEvent.keyDown(objectButton("Text: Plain body text"), { key: "Enter" });
    expect(screen.getByLabelText("Edit text in place")).toBeTruthy();
  });

  it("cancels an edit with Escape and commits it on blur", () => {
    open();
    fireEvent.doubleClick(objectButton("Text: Plain body text"));
    fireEvent.change(screen.getByLabelText("Edit text in place"), { target: { value: "Dropped" } });
    fireEvent.keyDown(screen.getByLabelText("Edit text in place"), { key: "Escape" });
    expect(objectButton("Text: Plain body text")).toBeTruthy();

    fireEvent.doubleClick(objectButton("Text: Plain body text"));
    fireEvent.change(screen.getByLabelText("Edit text in place"), { target: { value: "Kept" } });
    fireEvent.blur(screen.getByLabelText("Edit text in place"));
    expect(objectButton("Text: Kept")).toBeTruthy();
  });

  it("warns about the substituted font while typing Vietnamese into a subset font", () => {
    const { onApply } = open();
    expect(screen.queryByRole("alert")).toBeNull();

    fireEvent.doubleClick(objectButton("Text: Tieng Viet"));
    // Typing plain ASCII keeps the embedded subset.
    fireEvent.change(screen.getByLabelText("Edit text in place"), {
      target: { value: "Tieng Viet 2" },
    });
    expect(screen.queryByRole("alert")).toBeNull();

    fireEvent.change(screen.getByLabelText("Edit text in place"), {
      target: { value: VIETNAMESE },
    });
    const live = screen.getByRole("alert");
    expect(live.textContent).toContain("Subset Helvetica → Noto Sans (Vietnamese coverage)");
    expect(live.textContent).toContain("ế");
    expect((screen.getByLabelText("Edit text in place") as HTMLElement).style.fontFamily).toContain(
      "Noto Sans",
    );

    fireEvent.keyDown(screen.getByLabelText("Edit text in place"), { key: "Enter" });
    // The warning stays after the edit is committed, until the edit is undone.
    const committed = screen.getByRole("alert");
    expect(within(committed).getByText("Font substituted")).toBeTruthy();
    expect(objectButton(`Text: ${VIETNAMESE}`)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Apply page edits" }));
    expect(lastApplied(onApply).ops).toEqual([
      { op: "replace_text", page: 1, object_id: "p1_obj1", text: VIETNAMESE },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("changes font size, colour and family from the inspector", () => {
    const { onApply } = open();
    fireEvent.click(objectButton("Text: Plain body text"));
    fireEvent.change(screen.getByLabelText("Font size (pt)"), { target: { value: "16" } });
    fireEvent.change(screen.getByLabelText("Text colour"), { target: { value: "#336699" } });
    fireEvent.change(screen.getByLabelText("Font family"), { target: { value: "Noto Sans" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply page edits" }));
    expect(lastApplied(onApply).ops).toEqual([
      {
        op: "set_text_style",
        page: 1,
        object_id: "p1_obj2",
        font_name: "Noto Sans",
        font_size: 16,
        color: "#336699",
      },
    ]);
  });

  it("adds a text box where the page is clicked", () => {
    const { onApply } = open();
    fireEvent.click(screen.getByRole("button", { name: "Add text box" }));
    const surface = screen.getAllByRole("button", { name: "Add text box" }).at(-1) as HTMLElement;
    fireEvent.click(surface, { clientX: 120, clientY: 92, detail: 1 });

    // The new box opens in the in-place editor straight away.
    const editor = screen.getByLabelText("Edit text in place") as HTMLTextAreaElement;
    expect(editor.value).toBe("New text");
    fireEvent.change(editor, { target: { value: "Ghi chú mới" } });
    fireEvent.keyDown(editor, { key: "Enter" });
    expect(objectButton("Text: Ghi chú mới")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Select and move" }).getAttribute("aria-pressed"),
    ).toBe("true");

    fireEvent.click(screen.getByRole("button", { name: "Apply page edits" }));
    expect(lastApplied(onApply).ops).toEqual([
      expect.objectContaining({
        op: "add_text",
        page: 1,
        text: "Ghi chú mới",
        x_pt: 120,
        y_pt: 700,
      }),
    ]);
  });

  it("adds an image chosen from a file", async () => {
    const readImage = vi.fn(async () => ({
      dataUrl: "data:image/png;base64,QUJD",
      width: 400,
      height: 200,
    }));
    const { onApply } = open({ readImage });
    const input = screen.getByTestId("page-image-input");
    await act(async () => {
      fireEvent.change(input, {
        target: { files: [new File(["x"], "logo.png", { type: "image/png" })] },
      });
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Image: 400×200" })).toBeTruthy(),
    );
    expect(readImage).toHaveBeenCalledTimes(1);
    // Centred, 200pt wide, 100pt tall on a 612×792 page.
    expect([readout("x"), readout("y"), readout("w"), readout("h")]).toEqual([
      "206",
      "346",
      "200",
      "100",
    ]);

    fireEvent.click(screen.getByRole("button", { name: "Apply page edits" }));
    expect(lastApplied(onApply).ops).toEqual([
      { op: "add_image", page: 1, bbox: [206, 346, 406, 446], image_base64: "QUJD" },
    ]);
  });

  it("undoes and redoes from the toolbar and with keyboard shortcuts", () => {
    open();
    const undo = screen.getByRole("button", { name: "Undo" }) as HTMLButtonElement;
    const redo = screen.getByRole("button", { name: "Redo" }) as HTMLButtonElement;
    expect([undo.disabled, redo.disabled]).toEqual([true, true]);

    const image = objectButton("Image: 200×120");
    fireEvent.click(image);
    fireEvent.keyDown(image, { key: "ArrowRight" });
    fireEvent.keyDown(image, { key: "ArrowRight" });
    expect(readout("x")).toBe("74");
    expect([undo.disabled, redo.disabled]).toEqual([false, true]);

    fireEvent.click(undo);
    expect(readout("x")).toBe("73");
    expect(redo.disabled).toBe(false);
    fireEvent.click(redo);
    expect(readout("x")).toBe("74");

    fireEvent.keyDown(objectButton("Image: 200×120"), { key: "z", ctrlKey: true });
    expect(readout("x")).toBe("73");
    fireEvent.keyDown(objectButton("Image: 200×120"), { key: "z", ctrlKey: true, shiftKey: true });
    expect(readout("x")).toBe("74");
    fireEvent.keyDown(objectButton("Image: 200×120"), { key: "z", metaKey: true });
    fireEvent.keyDown(objectButton("Image: 200×120"), { key: "y", ctrlKey: true });
    expect(readout("x")).toBe("74");
  });

  it("switches pages and applies edits across pages in one request", () => {
    const { onApply } = open();
    fireEvent.click(screen.getByRole("button", { name: "Delete selected" }));
    fireEvent.click(objectButton("Shape: p1_obj4"));
    fireEvent.click(screen.getByRole("button", { name: "Delete selected" }));

    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    expect(screen.getByText("Page 2 of 2")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Text: Plain body text" })).toBeNull();
    const text = objectButton("Text: Second page");
    fireEvent.click(text);
    fireEvent.keyDown(text, { key: "ArrowLeft", shiftKey: true });
    expect((screen.getByRole("button", { name: "Next page" }) as HTMLButtonElement).disabled).toBe(
      true,
    );

    fireEvent.click(screen.getByRole("button", { name: "Apply page edits" }));
    expect(lastApplied(onApply).ops).toEqual([
      { op: "delete_object", page: 1, object_id: "p1_obj4" },
      { op: "move_object", page: 2, object_id: "p2_obj1", dx_pt: -10, dy_pt: 0 },
    ]);

    fireEvent.click(screen.getByRole("button", { name: "Previous page" }));
    expect(screen.getByText("Page 1 of 2")).toBeTruthy();
  });

  it("keeps the page surface left-to-right and uses logical classes for layout", () => {
    const { container } = open({ renderPageBackground: (page) => <i data-bg={page.page} /> });
    const surface = container.querySelector('[data-slot="page-surface"]') as HTMLElement;
    expect(surface.getAttribute("dir")).toBe("ltr");
    expect(surface.style.width).toBe("612px");
    expect(container.querySelector("[data-bg='1']")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Apply page edits" }).className).toContain("ms-auto");
  });

  it("accepts localised toolbar copy", () => {
    open({ labels: { undo: "Hoàn tác", pageOf: "Trang {page}/{count}" } });
    expect(screen.getByRole("button", { name: "Hoàn tác" })).toBeTruthy();
    expect(screen.getByText("Trang 1/2")).toBeTruthy();
  });
});
