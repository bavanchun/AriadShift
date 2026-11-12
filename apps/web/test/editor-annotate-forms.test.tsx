// @vitest-environment jsdom

import { readFileSync } from "node:fs";
import path from "node:path";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import {
  AnnotateFormsSurface,
  type AnnotationDraft,
  type AnnotationStore,
  type AnnotationsParams,
  AnnotationToolbar,
  createAnnotationStore,
  createFormDesignStore,
  createFormFillStore,
  type EditorToolCall,
  FormDesignerView,
  type FormFieldInfo,
  FormFillerView,
  handleAnnotationShortcut,
  planAnnotationCalls,
  toAnnotateParams,
  toAnnotationRemoveParams,
  toFormCreateParams,
  toFormFillParams,
  toFormFlattenParams,
} from "@/editor/annotate";
import {
  type AnnotationTool,
  draftFromGesture,
  type Point,
} from "@/editor/annotate/annotation-store";
import { designIssues } from "@/editor/annotate/form-model";

afterEach(cleanup);

// ---------------------------------------------------------------------------------------
// Schema conformance: a small JSON Schema checker over `schemas/tools.v1.json`, so the
// emitted parameters are compared with the published contract and not with a copy of it.
// ---------------------------------------------------------------------------------------

interface Schema {
  $ref?: string;
  anyOf?: Schema[];
  enum?: unknown[];
  type?: string | string[];
  properties?: Record<string, Schema>;
  additionalProperties?: boolean | Schema;
  required?: string[];
  items?: Schema;
  minItems?: number;
  maxItems?: number;
  minimum?: number;
  $defs?: Record<string, Schema>;
}

const toolsSchema = JSON.parse(
  readFileSync(path.resolve(import.meta.dirname, "../../../schemas/tools.v1.json"), "utf8"),
) as { tools: { id: string; params_schema: Schema }[] };

const paramsSchemaOf = (toolId: string): Schema => {
  const tool = toolsSchema.tools.find((entry) => entry.id === toolId);
  if (!tool) throw new Error(`${toolId} is not in schemas/tools.v1.json`);
  return tool.params_schema;
};

const isType = (type: string, value: unknown): boolean => {
  switch (type) {
    case "null":
      return value === null;
    case "boolean":
      return typeof value === "boolean";
    case "string":
      return typeof value === "string";
    case "number":
      return typeof value === "number" && Number.isFinite(value);
    case "integer":
      return Number.isInteger(value);
    case "array":
      return Array.isArray(value);
    case "object":
      return typeof value === "object" && value !== null && !Array.isArray(value);
    default:
      return false;
  }
};

function check(schema: Schema, value: unknown, root: Schema, at: string): string[] {
  if (schema.$ref) {
    const target = root.$defs?.[schema.$ref.replace("#/$defs/", "")];
    return target ? check(target, value, root, at) : [`${at}: unresolved ${schema.$ref}`];
  }
  if (schema.anyOf) {
    const attempts = schema.anyOf.map((option) => check(option, value, root, at));
    return attempts.some((errors) => errors.length === 0) ? [] : [`${at}: matches no alternative`];
  }
  const errors: string[] = [];
  if (schema.enum && !schema.enum.includes(value))
    errors.push(`${at}: ${String(value)} not in enum`);
  const types = ([] as string[]).concat(schema.type ?? []);
  if (types.length > 0 && !types.some((type) => isType(type, value)))
    return [...errors, `${at}: expected ${types.join("|")}`];
  if (typeof value === "number" && schema.minimum !== undefined && value < schema.minimum)
    errors.push(`${at}: below minimum`);
  if (Array.isArray(value)) {
    if (schema.minItems !== undefined && value.length < schema.minItems)
      errors.push(`${at}: too few items`);
    if (schema.maxItems !== undefined && value.length > schema.maxItems)
      errors.push(`${at}: too many items`);
    if (schema.items)
      for (const [index, entry] of value.entries())
        errors.push(...check(schema.items, entry, root, `${at}[${index}]`));
  } else if (isType("object", value)) {
    const record = value as Record<string, unknown>;
    for (const key of schema.required ?? [])
      if (!(key in record)) errors.push(`${at}: missing ${key}`);
    for (const [key, entry] of Object.entries(record)) {
      const known = schema.properties?.[key];
      if (known) errors.push(...check(known, entry, root, `${at}.${key}`));
      else if (schema.additionalProperties === false) errors.push(`${at}: unknown key ${key}`);
      else if (typeof schema.additionalProperties === "object")
        errors.push(...check(schema.additionalProperties, entry, root, `${at}.${key}`));
    }
  }
  return errors;
}

const conforms = (toolId: string, params: unknown) => {
  const schema = paramsSchemaOf(toolId);
  return check(schema, params, schema, "params");
};

// ---------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------

const PAGE = { width: 612, height: 792 };

/** Places an annotation the way a pointer gesture would, with the store's current settings. */
function place(
  store: AnnotationStore,
  tool: AnnotationTool,
  gesture: Parameters<typeof draftFromGesture>[1],
  page = 1,
): string {
  const state = store.getState();
  state.setTool(tool);
  const draft = draftFromGesture({ ...store.getState().settings, page }, gesture);
  if (!draft) throw new Error(`no annotation for ${tool}`);
  return state.add(draft as AnnotationDraft);
}

const drag = (from: Point, to: Point) => ({ type: "drag", from, to }) as const;

// ---------------------------------------------------------------------------------------
// Annotation model
// ---------------------------------------------------------------------------------------

describe("annotation store and pdf.annotations parameters", () => {
  it("emits every annotation type in the shape the schema defines", () => {
    const store = createAnnotationStore();
    place(store, "highlight", drag([100, 500], [300, 514]));
    place(store, "underline", drag([100, 480], [300, 494]));
    place(store, "strike", drag([100, 460], [300, 474]));
    store.getState().configure({ author: "Ana" });
    const noteId = place(store, "note", drag([50, 700], [50, 700]));
    store.getState().update(noteId, { contents: "Check this figure" });
    place(store, "ink", {
      type: "stroke",
      points: [
        [10, 10],
        [20, 25],
        [30, 10],
      ],
    });
    store.getState().configure({ shape: "rectangle", fill: "#FFFF00" });
    place(store, "shape", drag([50, 50], [150, 100]));
    store.getState().configure({ shape: "ellipse", fill: null });
    place(store, "shape", drag([200, 50], [260, 90]));
    store.getState().configure({ shape: "line" });
    place(store, "shape", drag([10, 10], [90, 60]));
    store.getState().configure({ shape: "arrow" });
    place(store, "shape", drag([20, 20], [120, 20]));
    store.getState().configure({ stamp: "APPROVED" });
    place(store, "stamp", drag([300, 600], [440, 644]));
    store.getState().configure({ stamp: "VOID" });
    place(store, "stamp", drag([300, 500], [440, 544]));

    const params = toAnnotateParams(store.getState().items);
    expect(conforms("pdf.annotations", params)).toEqual([]);
    expect(params.action).toBe("add");
    expect(params.items).toEqual([
      {
        type: "highlight",
        page: 1,
        color: "#FFEB3B",
        opacity: 0.4,
        quads: [[100, 514, 300, 514, 100, 500, 300, 500]],
      },
      {
        type: "underline",
        page: 1,
        color: "#E53935",
        opacity: 1,
        quads: [[100, 494, 300, 494, 100, 480, 300, 480]],
      },
      {
        type: "strikeout",
        page: 1,
        color: "#E53935",
        opacity: 1,
        quads: [[100, 474, 300, 474, 100, 460, 300, 460]],
      },
      {
        type: "note",
        page: 1,
        color: "#FFD54F",
        opacity: 1,
        rect: [50, 676, 74, 700],
        contents: "Check this figure",
        author: "Ana",
      },
      {
        type: "ink",
        page: 1,
        color: "#1E88E5",
        opacity: 1,
        line_width: 2,
        strokes: [
          [
            [10, 10],
            [20, 25],
            [30, 10],
          ],
        ],
      },
      {
        type: "rectangle",
        page: 1,
        color: "#E53935",
        opacity: 1,
        line_width: 2,
        rect: [50, 50, 150, 100],
        fill: "#FFFF00",
      },
      {
        type: "ellipse",
        page: 1,
        color: "#E53935",
        opacity: 1,
        line_width: 2,
        rect: [200, 50, 260, 90],
      },
      {
        type: "line",
        page: 1,
        color: "#E53935",
        opacity: 1,
        line_width: 2,
        points: [
          [10, 10],
          [90, 60],
        ],
      },
      {
        type: "arrow",
        page: 1,
        color: "#E53935",
        opacity: 1,
        line_width: 2,
        points: [
          [20, 20],
          [120, 20],
        ],
      },
      {
        type: "stamp",
        page: 1,
        color: "#C62828",
        opacity: 1,
        rect: [300, 600, 440, 644],
        stamp: "approved",
      },
      {
        type: "stamp",
        page: 1,
        color: "#C62828",
        opacity: 1,
        rect: [300, 500, 440, 544],
        text: "VOID",
      },
    ]);
  });

  it("gives a click the default size of the tool and keeps the page number", () => {
    const store = createAnnotationStore();
    place(store, "highlight", drag([306, 396], [306, 396]), 3);
    const [item] = toAnnotateParams(store.getState().items).items ?? [];
    expect(item?.page).toBe(3);
    expect(item?.quads).toEqual([[246, 403, 366, 403, 246, 389, 366, 389]]);
    expect(
      draftFromGesture({ ...store.getState().settings, tool: "ink" }, drag([1, 1], [2, 2])),
    ).toBeNull();
    expect(
      draftFromGesture({ ...store.getState().settings, tool: "select" }, drag([1, 1], [9, 9])),
    ).toBeNull();
  });

  it("edits, moves, deletes, undoes and redoes across pages", () => {
    const store = createAnnotationStore();
    const first = place(store, "highlight", drag([100, 500], [200, 514]), 1);
    const second = place(store, "stamp", drag([100, 100], [240, 144]), 2);
    expect(store.getState().items.map((item) => item.page)).toEqual([1, 2]);

    store.getState().update(first, { color: "#42A5F5", opacity: 0.6 });
    store.getState().move(second, 10, -5);
    const [, stamp] = toAnnotateParams(store.getState().items).items ?? [];
    expect(stamp?.rect).toEqual([110, 95, 250, 139]);
    expect(store.getState().items[0]).toMatchObject({ color: "#42A5F5", opacity: 0.6 });

    const depth = store.getState().past.length;
    store.getState().update(first, { color: "#42A5F5" });
    expect(store.getState().past).toHaveLength(depth);

    store.getState().remove(first);
    expect(store.getState().items.map((item) => item.id)).toEqual([second]);
    store.getState().undo();
    expect(store.getState().items.map((item) => item.id)).toEqual([first, second]);
    store.getState().undo();
    store.getState().undo();
    expect(store.getState().items[1]).toMatchObject({ rect: [100, 100, 240, 144] });
    expect(store.getState().items[0]).toMatchObject({ color: "#FFEB3B" });
    store.getState().redo();
    store.getState().redo();
    store.getState().redo();
    expect(store.getState().items.map((item) => item.id)).toEqual([second]);
    expect(store.getState().future).toHaveLength(0);

    place(store, "highlight", drag([1, 1], [50, 20]));
    store.getState().undo();
    store.getState().redo();
    expect(store.getState().items).toHaveLength(2);
    store.getState().reset();
    expect(planAnnotationCalls(store.getState())).toEqual([]);
  });

  it("removes annotations already stored in the PDF with a delete call", () => {
    const store = createAnnotationStore();
    store.getState().setExisting([
      { id: "A1", page: 1, type: "highlight" },
      { id: "A2", page: 2, type: "note" },
    ]);
    store.getState().remove("A1");
    place(store, "underline", drag([10, 10], [60, 24]));
    expect(planAnnotationCalls(store.getState()).map((call) => call.params)).toEqual([
      toAnnotateParams(store.getState().items),
      { action: "delete", ids: ["A1"] },
    ]);
    expect(conforms("pdf.annotations", toAnnotationRemoveParams(["A1", "A2"]))).toEqual([]);
    store.getState().undo();
    expect(store.getState().removed).toEqual(["A1"]);
    store.getState().undo();
    expect(store.getState().removed).toEqual([]);
    store.getState().redo();
    store.getState().restoreExisting("A1");
    expect(store.getState().removed).toEqual([]);
  });
});

// ---------------------------------------------------------------------------------------
// Annotate surface
// ---------------------------------------------------------------------------------------

describe("<AnnotationToolbar /> and the annotate mode", () => {
  it("selects tools with buttons and keyboard shortcuts and shows the options of each", () => {
    const store = createAnnotationStore();
    render(
      // biome-ignore lint/a11y/noStaticElementInteractions: mirrors how the surface routes key events
      <div onKeyDown={(event) => handleAnnotationShortcut(event, store)}>
        <AnnotationToolbar store={store} />
      </div>,
    );
    const pressed = (name: string) =>
      screen.getByRole("button", { name }).getAttribute("aria-pressed");

    fireEvent.click(screen.getByRole("button", { name: "Highlight" }));
    expect(pressed("Highlight")).toBe("true");
    expect(pressed("Select")).toBe("false");
    fireEvent.click(screen.getByRole("button", { name: "Green colour" }));
    expect(store.getState().settings.color).toBe("#66BB6A");
    fireEvent.change(screen.getByRole("slider", { name: "Opacity" }), { target: { value: "0.7" } });
    expect(store.getState().settings.opacity).toBe(0.7);

    fireEvent.click(screen.getByRole("button", { name: "Shapes" }));
    fireEvent.click(screen.getByRole("button", { name: "Arrow" }));
    expect(store.getState().settings.shape).toBe("arrow");
    expect(screen.getByRole("slider", { name: "Stroke width" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Stamp" }));
    fireEvent.click(screen.getByRole("button", { name: "CONFIDENTIAL" }));
    expect(store.getState().settings.stamp).toBe("CONFIDENTIAL");
    fireEvent.change(screen.getByRole("textbox", { name: "Custom stamp text" }), {
      target: { value: "PAID" },
    });
    expect(store.getState().settings.stamp).toBe("PAID");

    fireEvent.click(screen.getByRole("button", { name: "Sticky note" }));
    fireEvent.change(screen.getByLabelText("Note icon"), { target: { value: "key" } });
    expect(store.getState().settings.icon).toBe("key");

    // Shortcuts work from a focused control, but not while typing in a field.
    const ink = screen.getByRole("button", { name: "Freehand ink" });
    fireEvent.keyDown(ink, { key: "u" });
    expect(store.getState().settings.tool).toBe("underline");
    fireEvent.keyDown(screen.getByLabelText("Opacity"), { key: "s" });
    expect(store.getState().settings.tool).toBe("underline");
  });
});

describe("<AnnotateFormsSurface /> annotate mode", () => {
  const renderSurface = (props: Partial<Parameters<typeof AnnotateFormsSurface>[0]> = {}) => {
    const onApply = vi.fn<(calls: EditorToolCall[]) => void>();
    render(<AnnotateFormsSurface pageCount={3} pageSize={PAGE} onApply={onApply} {...props} />);
    return onApply;
  };

  it("places, edits, deletes and undoes annotations, then applies them as one call", async () => {
    const onApply = renderSurface();
    const apply = screen.getByRole("button", { name: "Apply changes" }) as HTMLButtonElement;
    expect(apply.disabled).toBe(true);
    expect(screen.getByText("No annotations yet.")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Highlight" }));
    fireEvent.click(screen.getByRole("button", { name: "Place highlight at centre" }));
    expect(screen.getByText("1 pending change")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Highlight 1 on page 1" })).toBeTruthy();
    expect(apply.disabled).toBe(false);

    // The new annotation is selected: its inspector edits the comment fields of a note.
    fireEvent.click(screen.getByRole("button", { name: "Sticky note" }));
    fireEvent.click(screen.getByRole("button", { name: "Place sticky note at centre" }));
    fireEvent.change(screen.getByLabelText("Comment"), { target: { value: "Please review" } });
    expect((screen.getByLabelText("Comment") as HTMLTextAreaElement).value).toBe("Please review");

    // Page 2 gets a stamp; the page picker moves between pages.
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    expect(screen.getByText("Page 2 of 3")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Stamp" }));
    fireEvent.click(screen.getByRole("button", { name: "Place stamp at centre" }));
    expect(screen.getByRole("button", { name: "Stamp 3 on page 2" })).toBeTruthy();
    expect(screen.getByText("3 pending changes")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Delete Highlight 1" }));
    expect(screen.queryByRole("button", { name: "Highlight 1 on page 1" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(screen.getByRole("button", { name: /Highlight \d on page 1/ })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Redo" }));
    expect(screen.queryByRole("button", { name: /Highlight \d on page 1/ })).toBeNull();
    fireEvent.keyDown(screen.getByRole("button", { name: "Undo" }), { key: "z", ctrlKey: true });
    expect(screen.getByRole("button", { name: /Highlight \d on page 1/ })).toBeTruthy();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Apply changes" }));
    });
    expect(onApply).toHaveBeenCalledTimes(1);
    const calls = onApply.mock.calls[0]?.[0] ?? [];
    expect(calls).toHaveLength(1);
    const call = calls[0];
    expect(call?.toolId).toBe("pdf.annotations");
    expect(conforms("pdf.annotations", call?.params)).toEqual([]);
    const params = call?.params as AnnotationsParams;
    const items = params.items ?? [];
    expect(items.map((item) => [item.type, item.page])).toEqual([
      ["highlight", 1],
      ["note", 1],
      ["stamp", 2],
    ]);
    expect(items[1]?.contents).toBe("Please review");
    // The applied changes are cleared.
    expect(screen.getByText("No annotations yet.")).toBeTruthy();
  });

  it("draws freehand ink with the pointer and moves a selection with the arrow keys", () => {
    renderSurface();
    fireEvent.click(screen.getByRole("button", { name: "Freehand ink" }));
    const canvas = screen.getByRole("group", { name: "Page 1 drawing area" });
    fireEvent.pointerDown(canvas, { clientX: 10, clientY: 782, button: 0, pointerId: 1 });
    fireEvent.pointerMove(canvas, { clientX: 20, clientY: 767, pointerId: 1 });
    fireEvent.pointerMove(canvas, { clientX: 30, clientY: 782, pointerId: 1 });
    fireEvent.pointerUp(canvas, { clientX: 30, clientY: 782, pointerId: 1 });
    expect(screen.getByRole("button", { name: "Freehand ink 1 on page 1" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Select" }));
    const stroke = canvas.querySelector('[data-kind="ink"]') as SVGGElement;
    expect(stroke.querySelector("polyline")?.getAttribute("points")).toBe("10,782 20,767 30,782");
    fireEvent.keyDown(stroke, { key: "ArrowRight" });
    fireEvent.keyDown(stroke, { key: "ArrowUp", shiftKey: true });
    expect(canvas.querySelector('[data-kind="ink"] polyline')?.getAttribute("points")).toBe(
      "14,762 24,747 34,762",
    );
    fireEvent.keyDown(canvas.querySelector('[data-kind="ink"]') as SVGGElement, { key: "Delete" });
    expect(screen.getByText("No annotations yet.")).toBeTruthy();
  });

  it("marks annotations already in the PDF for deletion and applies a delete call", async () => {
    const onApply = renderSurface({
      existingAnnotations: [{ id: "A1", page: 1, type: "highlight" }],
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Delete highlight on page 1 (in the PDF)" }),
    );
    expect(screen.getByText("Will be deleted")).toBeTruthy();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Apply changes" }));
    });
    expect(onApply).toHaveBeenCalledWith([
      { toolId: "pdf.annotations", params: { action: "delete", ids: ["A1"] } },
    ]);
  });

  it("switches between annotate, fill and design without losing pending annotations", () => {
    renderSurface();
    const mode = (name: string) => screen.getByRole("button", { name });
    expect(mode("Annotate").getAttribute("aria-pressed")).toBe("true");

    fireEvent.click(screen.getByRole("button", { name: "Highlight" }));
    fireEvent.click(screen.getByRole("button", { name: "Place highlight at centre" }));

    fireEvent.click(mode("Fill form"));
    expect(mode("Fill form").getAttribute("aria-pressed")).toBe("true");
    expect(mode("Annotate").getAttribute("aria-pressed")).toBe("false");
    expect(screen.getByText("This PDF has no fillable form fields.")).toBeTruthy();
    expect(screen.queryByRole("toolbar", { name: "Annotation tools" })).toBeNull();

    fireEvent.click(mode("Design form"));
    expect(screen.getByRole("button", { name: "Add text field" })).toBeTruthy();
    expect(
      (screen.getByRole("button", { name: "Create fields" }) as HTMLButtonElement).disabled,
    ).toBe(true);

    fireEvent.click(mode("Annotate"));
    expect(screen.getByRole("button", { name: "Highlight 1 on page 1" })).toBeTruthy();
  });
});

// ---------------------------------------------------------------------------------------
// Form filling
// ---------------------------------------------------------------------------------------

const FIELDS: FormFieldInfo[] = [
  {
    name: "notes",
    type: "text",
    value: "",
    flags: { multiline: true },
    page: 1,
    rect: [72, 560, 400, 620],
    tab_order: 3,
    tooltip: "Notes",
  },
  {
    name: "full_name",
    type: "text",
    value: "",
    flags: { required: true },
    page: 1,
    rect: [72, 700, 272, 724],
    tab_order: 0,
    tooltip: "Full name",
  },
  {
    name: "country",
    type: "combo",
    value: null,
    options: [
      { export: "vn", display: "Vietnam" },
      { export: "us", display: "United States" },
    ],
    page: 1,
    rect: [72, 620, 272, 644],
    tab_order: 2,
    tooltip: "Country",
  },
  {
    name: "agree",
    type: "checkbox",
    value: false,
    flags: { required: true },
    page: 1,
    rect: [72, 660, 90, 678],
    tab_order: 1,
    tooltip: "I agree",
  },
  {
    name: "plan",
    type: "radio",
    value: null,
    widgets: [{ on_state: "basic" }, { on_state: "pro" }],
    page: 2,
    rect: [72, 700, 90, 718],
    tab_order: 4,
    tooltip: "Plan",
  },
  {
    name: "signature",
    type: "signature",
    value: null,
    flags: { required: true },
    page: 2,
    rect: [72, 100, 272, 148],
    tab_order: 5,
    tooltip: "Signature",
  },
];

describe("<FormFillerView />", () => {
  it("lists fields in tab order and counts the empty required fields", () => {
    const store = createFormFillStore(FIELDS);
    render(<FormFillerView store={store} idPrefix="t" />);
    const list = screen.getByRole("list", { name: "Form fields in tab order" });
    expect(
      within(list)
        .getAllByRole("listitem")
        .map((item) => item.getAttribute("data-field")),
    ).toEqual(["full_name", "agree", "country", "notes", "plan", "signature"]);

    // The unsigned signature is not counted: it is added with the Sign tool.
    expect(screen.getByRole("status").textContent).toBe("2 required fields remaining");
    expect(list.querySelectorAll('[data-empty-required="true"]')).toHaveLength(2);
    expect(screen.getByText("Add this signature with the Sign tool.")).toBeTruthy();
  });

  it("fills text, checkbox, select and radio fields and emits pdf.forms_fill parameters", () => {
    const store = createFormFillStore(FIELDS);
    render(<FormFillerView store={store} idPrefix="t" />);
    expect(toFormFillParams(store.getState())).toEqual({ values: {} });

    fireEvent.change(screen.getByLabelText(/^Full name/), { target: { value: "Nguyễn Văn A" } });
    expect(screen.getByRole("status").textContent).toBe("1 required field remaining");
    fireEvent.click(screen.getByLabelText(/^I agree/));
    expect(screen.getByRole("status").textContent).toBe("All required fields are filled");
    fireEvent.change(screen.getByLabelText("Country"), { target: { value: "vn" } });
    fireEvent.change(screen.getByLabelText("Notes"), { target: { value: "Line one\nLine two" } });
    fireEvent.click(screen.getByLabelText("pro"));

    const params = toFormFillParams(store.getState());
    expect(params).toEqual({
      values: {
        full_name: "Nguyễn Văn A",
        agree: true,
        country: "vn",
        notes: "Line one\nLine two",
        plan: "pro",
      },
    });
    expect(conforms("pdf.forms_fill", params)).toEqual([]);

    // Clearing a field sends null, and flattening is a toggle on the same call.
    fireEvent.change(screen.getByLabelText("Country"), { target: { value: "" } });
    fireEvent.click(screen.getByLabelText(/Flatten after filling/));
    const flattened = toFormFillParams(store.getState());
    expect(flattened.flatten).toBe(true);
    expect(flattened.values.country).toBeUndefined();
    expect(conforms("pdf.forms_fill", flattened)).toEqual([]);
    expect(conforms("pdf.forms_flatten", toFormFlattenParams(["full_name"]))).toEqual([]);
    expect(toFormFlattenParams()).toEqual({});

    fireEvent.click(screen.getByRole("button", { name: "Reset answers" }));
    expect(screen.getByRole("status").textContent).toBe("2 required fields remaining");
  });

  it("moves focus to the next empty required field", () => {
    const store = createFormFillStore(FIELDS);
    render(<FormFillerView store={store} idPrefix="t" />);
    const next = screen.getByRole("button", { name: "Go to next required field" });
    fireEvent.click(next);
    expect(document.activeElement).toBe(screen.getByLabelText(/^Full name/));
    fireEvent.click(next);
    expect(document.activeElement).toBe(screen.getByLabelText(/^I agree/));
    fireEvent.click(next);
    expect(document.activeElement).toBe(screen.getByLabelText(/^Full name/));
    fireEvent.change(screen.getByLabelText(/^Full name/), { target: { value: "A" } });
    fireEvent.click(screen.getByLabelText(/^I agree/));
    expect((next as HTMLButtonElement).disabled).toBe(true);
  });

  it("applies the fill from the surface, with the overlay marking required fields", async () => {
    const onApply = vi.fn<(calls: EditorToolCall[]) => void>();
    render(
      <AnnotateFormsSurface pageCount={2} pageSize={PAGE} formFields={FIELDS} onApply={onApply} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Fill form" }));
    const apply = screen.getByRole("button", { name: "Apply changes" }) as HTMLButtonElement;
    expect(apply.disabled).toBe(true);
    const marker = screen.getByRole("button", { name: "Field 1: Full name" });
    expect(marker.getAttribute("data-empty-required")).toBe("true");
    fireEvent.click(marker);
    expect(document.activeElement).toBe(screen.getByLabelText(/^Full name/));

    fireEvent.change(screen.getByLabelText(/^Full name/), { target: { value: "Lê Thị B" } });
    expect(apply.disabled).toBe(false);
    await act(async () => {
      fireEvent.click(apply);
    });
    expect(onApply).toHaveBeenCalledWith([
      { toolId: "pdf.forms_fill", params: { values: { full_name: "Lê Thị B" } } },
    ]);
  });
});

// ---------------------------------------------------------------------------------------
// Form designer
// ---------------------------------------------------------------------------------------

describe("<FormDesignerView />", () => {
  const renderDesigner = () => {
    const store = createFormDesignStore();
    render(<FormDesignerView store={store} page={1} pageCount={2} pageSize={PAGE} />);
    return store;
  };
  const order = () =>
    within(screen.getByRole("list", { name: "Fields in tab order" }))
      .getAllByRole("listitem")
      .map((item) => item.getAttribute("data-field-key"));
  const badges = () =>
    Array.from(document.querySelectorAll("[data-tab-order]"), (node) => node.textContent);

  it("adds fields, edits them, reorders the tab order and emits pdf.forms_create parameters", () => {
    const store = renderDesigner();
    expect(screen.getByText("Add a field to start designing a fillable form.")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Add text field" }));
    fireEvent.click(screen.getByRole("button", { name: "Add checkbox field" }));
    fireEvent.click(screen.getByRole("button", { name: "Add dropdown field" }));
    fireEvent.click(screen.getByRole("button", { name: "Add signature field" }));
    expect(badges()).toEqual(["1", "2", "3", "4"]);
    expect(store.getState().fields.map((field) => field.name)).toEqual([
      "text_1",
      "checkbox_2",
      "dropdown_3",
      "signature_4",
    ]);

    // Edit the dropdown, which was selected when it was added then deselected by the signature.
    fireEvent.click(screen.getByRole("button", { name: "Edit dropdown_3" }));
    fireEvent.change(screen.getByLabelText("Field name"), { target: { value: "country" } });
    fireEvent.change(screen.getByLabelText("Label"), { target: { value: "Country" } });
    fireEvent.change(screen.getByLabelText("Choices (one per line)"), {
      target: { value: "Vietnam\nUnited States" },
    });
    fireEvent.change(screen.getByLabelText("Default value"), { target: { value: "Vietnam" } });
    fireEvent.click(screen.getByLabelText("Required"));
    expect(store.getState().fields[2]?.required).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Edit text_1" }));
    fireEvent.click(screen.getByLabelText("Multiple lines"));
    fireEvent.change(screen.getByLabelText("Default value"), { target: { value: "Hello" } });
    fireEvent.change(screen.getByLabelText("Left"), { target: { value: "100" } });
    fireEvent.change(screen.getByLabelText("Right"), { target: { value: "300" } });
    fireEvent.click(screen.getByRole("button", { name: "Edit checkbox_2" }));
    fireEvent.click(screen.getByLabelText("Checked by default"));

    const keys = order();
    fireEvent.click(screen.getByRole("button", { name: "Move country up" }));
    fireEvent.click(screen.getByRole("button", { name: "Move country up" }));
    expect(order()).toEqual([keys[2], keys[0], keys[1], keys[3]]);
    expect(badges()).toEqual(["1", "2", "3", "4"]);
    expect(
      (screen.getByRole("button", { name: "Move country up" }) as HTMLButtonElement).disabled,
    ).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Move signature_4 down" }));
    expect(
      (screen.getByRole("button", { name: "Move signature_4 down" }) as HTMLButtonElement).disabled,
    ).toBe(true);

    const params = toFormCreateParams(store.getState().fields);
    expect(conforms("pdf.forms_create", params)).toEqual([]);
    expect(params.fields).toEqual([
      {
        type: "combo",
        name: "country",
        page: 1,
        rect: [72, 624, 252, 648],
        required: true,
        tab_order: 0,
        tooltip: "Country",
        options: [
          { export: "Vietnam", display: "Vietnam" },
          { export: "United States", display: "United States" },
        ],
        default: "Vietnam",
      },
      {
        type: "text",
        name: "text_1",
        page: 1,
        rect: [100, 696, 300, 720],
        required: false,
        tab_order: 1,
        multiline: true,
        default: "Hello",
      },
      {
        type: "checkbox",
        name: "checkbox_2",
        page: 1,
        rect: [72, 666, 90, 684],
        required: false,
        tab_order: 2,
        export_value: "Yes",
        default: true,
      },
      {
        type: "signature",
        name: "signature_4",
        page: 1,
        rect: [72, 564, 272, 612],
        required: false,
        tab_order: 3,
      },
    ]);
  });

  it("explains problems and blocks creation until fields are valid", async () => {
    const onApply = vi.fn<(calls: EditorToolCall[]) => void>();
    const designStore = createFormDesignStore();
    render(
      <AnnotateFormsSurface
        pageCount={2}
        pageSize={PAGE}
        designStore={designStore}
        onApply={onApply}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Design form" }));
    fireEvent.click(screen.getByRole("button", { name: "Add text field" }));
    fireEvent.click(screen.getByRole("button", { name: "Add text field" }));
    const create = screen.getByRole("button", { name: "Create fields" }) as HTMLButtonElement;
    expect(create.disabled).toBe(false);

    fireEvent.change(screen.getByLabelText("Field name"), { target: { value: "text_1" } });
    expect(screen.getByRole("alert").textContent).toContain(
      "Another field already uses this name.",
    );
    expect(create.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("Field name"), { target: { value: "a.b" } });
    expect(screen.getByRole("alert").textContent).toContain("cannot contain a dot");
    fireEvent.change(screen.getByLabelText("Field name"), { target: { value: "" } });
    expect(screen.getByRole("alert").textContent).toContain("Give the field a name.");
    fireEvent.change(screen.getByLabelText("Field name"), { target: { value: "email" } });
    fireEvent.change(screen.getByLabelText("Right"), { target: { value: "10" } });
    expect(screen.getByRole("alert").textContent).toContain("positive width and height");
    expect(designIssues(designStore.getState().fields).size).toBe(1);
    fireEvent.change(screen.getByLabelText("Right"), { target: { value: "252" } });
    expect(create.disabled).toBe(false);

    await act(async () => {
      fireEvent.click(create);
    });
    expect(onApply).toHaveBeenCalledTimes(1);
    const call = onApply.mock.calls[0]?.[0]?.[0];
    expect(call?.toolId).toBe("pdf.forms_create");
    expect(conforms("pdf.forms_create", call?.params)).toEqual([]);
    expect(designStore.getState().fields).toHaveLength(0);
  });
});
