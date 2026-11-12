// TypeScript mirrors of the `schemas/tools.v1.json` parameter shapes this surface emits:
// `pdf.annotations`, `pdf.forms_fill`, `pdf.forms_create` and `pdf.forms_flatten`. Every shape
// is `additionalProperties: false` in the schema, so the builders emit only these keys.

/** 1-based page range selector, for example `all`, `3` or `2-5`. */
export type PageRange = string;

export type AnnotationType =
  | "highlight"
  | "underline"
  | "strikeout"
  | "squiggly"
  | "note"
  | "free_text"
  | "ink"
  | "line"
  | "arrow"
  | "rectangle"
  | "ellipse"
  | "polygon"
  | "polyline"
  | "stamp"
  | "link";

export type StampName =
  | "approved"
  | "experimental"
  | "not_approved"
  | "as_is"
  | "expired"
  | "not_for_public_release"
  | "confidential"
  | "final"
  | "sold"
  | "departmental"
  | "for_comment"
  | "top_secret"
  | "draft"
  | "for_public_release";

/** An annotation to add, in PDF points with a bottom-left origin and 1-based pages. */
export interface AnnotationItem {
  type: AnnotationType;
  page: number;
  /** `[left, bottom, right, top]`. */
  rect?: number[];
  /** Eight numbers per quad: top-left, top-right, bottom-left, bottom-right. */
  quads?: number[][];
  points?: number[][];
  strokes?: number[][][];
  /** `#rgb` or `#rrggbb`. */
  color?: string;
  fill?: string;
  opacity?: number;
  line_width?: number;
  font_size?: number;
  contents?: string;
  author?: string;
  subject?: string;
  text?: string;
  stamp?: StampName;
}

export interface AnnotationsParams {
  action: "list" | "add" | "update" | "delete" | "flatten";
  pages?: PageRange;
  items?: AnnotationItem[];
  ids?: string[];
  types?: AnnotationType[];
  all?: boolean;
}

export type FormValue = string | boolean | string[] | null;

export type FieldType = "text" | "checkbox" | "radio" | "combo" | "list" | "button" | "signature";

export interface ChoiceOption {
  export: string;
  display: string;
}

export interface FormsFillParams {
  values: Record<string, FormValue>;
  flatten?: boolean;
  strict?: boolean;
}

export interface NewField {
  type: FieldType;
  name: string;
  page: number;
  /** `[left, bottom, right, top]`. */
  rect: number[];
  default?: FormValue;
  export_value?: string;
  multiline?: boolean;
  options?: ChoiceOption[];
  required?: boolean;
  read_only?: boolean;
  tab_order?: number;
  tooltip?: string;
}

export interface FormsCreateParams {
  fields: NewField[];
  replace_existing?: boolean;
}

export interface FormsFlattenParams {
  fields?: string[] | null;
  pages?: PageRange;
  include_annotations?: boolean;
}

/** One `pdf.*` call the host runs through the tool runner. */
export type EditorToolCall =
  | { toolId: "pdf.annotations"; params: AnnotationsParams }
  | { toolId: "pdf.forms_fill"; params: FormsFillParams }
  | { toolId: "pdf.forms_create"; params: FormsCreateParams }
  | { toolId: "pdf.forms_flatten"; params: FormsFlattenParams };
