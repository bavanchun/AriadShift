// The form model of the Annotate surface: filling the fields of an existing AcroForm
// (`pdf.forms_fill`, `pdf.forms_flatten`) and designing new ones (`pdf.forms_create`).
import { createStore, type StoreApi } from "zustand/vanilla";
import type {
  ChoiceOption,
  EditorToolCall,
  FieldType,
  FormsCreateParams,
  FormsFillParams,
  FormsFlattenParams,
  FormValue,
  NewField,
} from "./tool-params";

/** `[left, bottom, right, top]` in PDF points. */
export type FieldRect = readonly [number, number, number, number];

/** A field as `pdf.forms_list` reports it, reduced to what the surface needs. */
export interface FormFieldInfo {
  name: string;
  type: FieldType;
  value: FormValue;
  options?: readonly ChoiceOption[];
  flags?: {
    required?: boolean;
    read_only?: boolean;
    multiline?: boolean;
    multi_select?: boolean;
  };
  /** One-based page number. */
  page: number;
  rect: FieldRect;
  tooltip?: string | null;
  tab_order?: number | null;
  /** Radio groups list one widget per choice, carrying the choice's value. */
  widgets?: readonly { on_state?: string }[];
}

export type FillKind = "text" | "multiline" | "checkbox" | "radio" | "select" | "signature";

export function fillKind(field: FormFieldInfo): FillKind {
  switch (field.type) {
    case "checkbox":
    case "radio":
    case "signature":
      return field.type;
    case "combo":
    case "list":
      return "select";
    default:
      return field.flags?.multiline ? "multiline" : "text";
  }
}

/** Fields the user can fill here. Signatures go through the sign tool; buttons hold no value. */
export const isFillable = (field: FormFieldInfo): boolean =>
  field.type !== "signature" && field.type !== "button" && !field.flags?.read_only;

/** The choices of a radio group or choice field, as stored and displayed values. */
export function choicesOf(field: FormFieldInfo): ChoiceOption[] {
  if (field.options && field.options.length > 0) return [...field.options];
  return (field.widgets ?? [])
    .map((widget) => widget.on_state)
    .filter((state): state is string => Boolean(state) && state !== "Off")
    .map((state) => ({ export: state, display: state }));
}

/**
 * The tab order of a form: fields with an explicit `tab_order` first, in that order, then the
 * rest by page, top to bottom and left to right. Ties keep their listed order.
 */
export function sortByTabOrder<T extends Pick<FormFieldInfo, "page" | "rect" | "tab_order">>(
  fields: readonly T[],
): T[] {
  return fields
    .map((field, index) => ({ field, index }))
    .sort((a, b) => {
      const left = a.field.tab_order ?? Number.POSITIVE_INFINITY;
      const right = b.field.tab_order ?? Number.POSITIVE_INFINITY;
      if (left !== right) return left < right ? -1 : 1;
      if (a.field.page !== b.field.page) return a.field.page - b.field.page;
      const topDelta = b.field.rect[3] - a.field.rect[3];
      if (topDelta !== 0) return topDelta;
      return a.field.rect[0] - b.field.rect[0] || a.index - b.index;
    })
    .map(({ field }) => field);
}

export function isEmptyValue(kind: FillKind, value: FormValue | undefined): boolean {
  if (kind === "checkbox") return value !== true && value !== "Yes" && !isOnState(value);
  if (value === null || value === undefined) return true;
  if (Array.isArray(value)) return value.length === 0;
  return value === "" || value === "Off";
}

const isOnState = (value: FormValue | undefined): boolean =>
  typeof value === "string" && value !== "" && value !== "Off";

export interface FormFillState {
  /** Fillable and read-only fields in tab order. */
  fields: readonly FormFieldInfo[];
  values: Readonly<Record<string, FormValue>>;
  flatten: boolean;
  setValue(name: string, value: FormValue): void;
  setFlatten(flatten: boolean): void;
  /** Restores every field to the value the PDF holds. */
  reset(): void;
}

export type FormFillStore = StoreApi<FormFillState>;

const valuesOf = (fields: readonly FormFieldInfo[]): Record<string, FormValue> =>
  Object.fromEntries(fields.map((field) => [field.name, field.value]));

export function createFormFillStore(input: readonly FormFieldInfo[]): FormFillStore {
  const fields = sortByTabOrder(input);
  return createStore<FormFillState>()((set) => ({
    fields,
    values: valuesOf(fields),
    flatten: false,
    setValue(name, value) {
      set((state) => ({ values: { ...state.values, [name]: value } }));
    },
    setFlatten(flatten) {
      set({ flatten });
    },
    reset() {
      set({ values: valuesOf(fields), flatten: false });
    },
  }));
}

/** Names of required, fillable fields that are still empty, in tab order. */
export function emptyRequiredFields(
  fields: readonly FormFieldInfo[],
  values: Readonly<Record<string, FormValue>>,
): string[] {
  return fields
    .filter(
      (field) =>
        field.flags?.required &&
        isFillable(field) &&
        isEmptyValue(fillKind(field), values[field.name]),
    )
    .map((field) => field.name);
}

const sameFormValue = (left: FormValue | undefined, right: FormValue | undefined) =>
  JSON.stringify(left ?? null) === JSON.stringify(right ?? null);

/** Whether any fillable field differs from the value the PDF holds. */
export function hasFillChanges(state: Pick<FormFillState, "fields" | "values">): boolean {
  return state.fields.some(
    (field) => isFillable(field) && !sameFormValue(state.values[field.name], field.value),
  );
}

/**
 * The `pdf.forms_fill` parameters: only the fields the user changed, with `flatten` when the
 * filled form should be baked into the page content.
 */
export function toFormFillParams(
  state: Pick<FormFillState, "fields" | "values" | "flatten">,
): FormsFillParams {
  const values: Record<string, FormValue> = {};
  for (const field of state.fields) {
    if (!isFillable(field)) continue;
    const next = state.values[field.name];
    if (!sameFormValue(next, field.value)) values[field.name] = next ?? null;
  }
  const params: FormsFillParams = { values };
  if (state.flatten) params.flatten = true;
  return params;
}

/** The standalone `pdf.forms_flatten` parameters, for a form that is already filled. */
export function toFormFlattenParams(fields?: readonly string[]): FormsFlattenParams {
  return fields && fields.length > 0 ? { fields: [...fields] } : {};
}

export function planFormFillCalls(
  state: Pick<FormFillState, "fields" | "values" | "flatten">,
): EditorToolCall[] {
  return [{ toolId: "pdf.forms_fill", params: toFormFillParams(state) }];
}

// ---------------------------------------------------------------------------------------
// Form designer
// ---------------------------------------------------------------------------------------

export type DesignFieldType = "text" | "checkbox" | "dropdown" | "signature";
export const DESIGN_FIELD_TYPES: readonly DesignFieldType[] = [
  "text",
  "checkbox",
  "dropdown",
  "signature",
];

export interface DesignField {
  key: string;
  type: DesignFieldType;
  /** The field name stored in the PDF; unique and free of dots. */
  name: string;
  /** Shown as the field's tooltip. */
  label: string;
  /** Default text of a text field, or the default choice of a dropdown. */
  defaultText: string;
  /** Default state of a checkbox. */
  checked: boolean;
  required: boolean;
  multiline: boolean;
  /** One choice per line, for a dropdown. */
  options: string;
  /** One-based page number. */
  page: number;
  rect: FieldRect;
}

export type DesignIssue = "name-empty" | "name-invalid" | "name-duplicate" | "rect" | "default";

export interface FormDesignState {
  fields: readonly DesignField[];
  selectedKey: string | null;
  /** Adds a field on a page with its top edge at `top`, and selects it. */
  add(type: DesignFieldType, page: number, top: number): string;
  update(key: string, patch: Partial<Omit<DesignField, "key">>): void;
  remove(key: string): void;
  /** Moves a field one position earlier (`-1`) or later (`1`) in the tab order. */
  move(key: string, offset: -1 | 1): void;
  select(key: string | null): void;
  reset(): void;
}

export type FormDesignStore = StoreApi<FormDesignState>;

const FIELD_SIZE: Readonly<Record<DesignFieldType, readonly [number, number]>> = {
  text: [180, 24],
  checkbox: [18, 18],
  dropdown: [180, 24],
  signature: [200, 48],
};
const LEFT_MARGIN = 72;

export function createFormDesignStore(initial: readonly DesignField[] = []): FormDesignStore {
  let counter = initial.length;
  return createStore<FormDesignState>()((set, get) => ({
    fields: initial,
    selectedKey: null,
    add(type, page, top) {
      counter += 1;
      const key = `design-field-${counter}`;
      const taken = new Set(get().fields.map((field) => field.name));
      let suffix = counter;
      while (taken.has(`${type}_${suffix}`)) suffix += 1;
      const [width, height] = FIELD_SIZE[type];
      const field: DesignField = {
        key,
        type,
        name: `${type}_${suffix}`,
        label: "",
        defaultText: "",
        checked: false,
        required: false,
        multiline: false,
        options: type === "dropdown" ? "Option 1\nOption 2" : "",
        page,
        rect: [LEFT_MARGIN, top - height, LEFT_MARGIN + width, top],
      };
      set((state) => ({ fields: [...state.fields, field], selectedKey: key }));
      return key;
    },
    update(key, patch) {
      set((state) => ({
        fields: state.fields.map((field) => (field.key === key ? { ...field, ...patch } : field)),
      }));
    },
    remove(key) {
      set((state) => ({
        fields: state.fields.filter((field) => field.key !== key),
        selectedKey: state.selectedKey === key ? null : state.selectedKey,
      }));
    },
    move(key, offset) {
      set((state) => {
        const from = state.fields.findIndex((field) => field.key === key);
        const to = from + offset;
        if (from < 0 || to < 0 || to >= state.fields.length) return state;
        const fields = [...state.fields];
        const [moved] = fields.splice(from, 1);
        fields.splice(to, 0, moved as DesignField);
        return { fields };
      });
    },
    select(key) {
      set({ selectedKey: key });
    },
    reset() {
      set({ fields: [], selectedKey: null });
    },
  }));
}

/** The dropdown choices of a field: its non-empty lines, each stored as shown. */
export function designOptions(field: Pick<DesignField, "options">): ChoiceOption[] {
  return field.options
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line !== "")
    .map((line) => ({ export: line, display: line }));
}

/** Problems that would make `pdf.forms_create` reject a field, keyed by field. */
export function designIssues(fields: readonly DesignField[]): Map<string, DesignIssue[]> {
  const counts = new Map<string, number>();
  for (const field of fields) counts.set(field.name, (counts.get(field.name) ?? 0) + 1);
  const issues = new Map<string, DesignIssue[]>();
  for (const field of fields) {
    const found: DesignIssue[] = [];
    if (field.name.trim() === "") found.push("name-empty");
    else if (field.name.includes(".")) found.push("name-invalid");
    if (field.name !== "" && (counts.get(field.name) ?? 0) > 1) found.push("name-duplicate");
    const [left, bottom, right, top] = field.rect;
    if (![left, bottom, right, top].every(Number.isFinite) || left >= right || bottom >= top)
      found.push("rect");
    if (
      field.type === "dropdown" &&
      field.defaultText !== "" &&
      !designOptions(field).some((option) => option.export === field.defaultText)
    )
      found.push("default");
    if (found.length > 0) issues.set(field.key, found);
  }
  return issues;
}

function toNewField(field: DesignField, index: number): NewField {
  const base: NewField = {
    type: field.type === "dropdown" ? "combo" : field.type,
    name: field.name,
    page: field.page,
    rect: [...field.rect],
    required: field.required,
    tab_order: index,
  };
  if (field.label.trim() !== "") base.tooltip = field.label.trim();
  switch (field.type) {
    case "text":
      if (field.multiline) base.multiline = true;
      if (field.defaultText !== "") base.default = field.defaultText;
      break;
    case "checkbox":
      base.export_value = "Yes";
      if (field.checked) base.default = true;
      break;
    case "dropdown":
      base.options = designOptions(field);
      if (field.defaultText !== "") base.default = field.defaultText;
      break;
    case "signature":
      break;
  }
  return base;
}

/** The `pdf.forms_create` parameters: every designed field, numbered by its tab position. */
export function toFormCreateParams(fields: readonly DesignField[]): FormsCreateParams {
  return { fields: fields.map(toNewField) };
}

export function planFormCreateCalls(fields: readonly DesignField[]): EditorToolCall[] {
  return [{ toolId: "pdf.forms_create", params: toFormCreateParams(fields) }];
}
