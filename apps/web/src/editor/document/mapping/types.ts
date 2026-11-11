/** JSON contracts mirror ariad-core/src/ir, including extension data we do not edit. */
export interface IrFields {
  [key: string]: unknown;
}

export interface FontRef extends IrFields {
  family: string;
  fallback?: string[];
}

export interface CharacterStyle extends IrFields {
  font?: FontRef;
  size_pt?: number;
  weight?: number;
  italic?: boolean;
  underline?: boolean;
  strike?: boolean;
  color?: string;
  highlight?: string;
  baseline_shift?: number;
  letter_spacing?: number;
  small_caps?: boolean;
}

export interface ParagraphStyle extends IrFields {
  based_on?: string;
  font?: FontRef;
  size_pt?: number;
  line_height?: number;
  space_before_pt?: number;
  space_after_pt?: number;
  align?: "left" | "center" | "right" | "justify";
  indent_first_pt?: number;
  indent_left_pt?: number;
  indent_right_pt?: number;
  keep_with_next?: boolean;
  page_break_before?: boolean;
  color?: string;
  background?: string;
}

export interface StyleSheet extends IrFields {
  paragraph?: Record<string, ParagraphStyle>;
  character?: Record<string, CharacterStyle>;
  table?: Record<string, IrFields>;
  defaults?: IrFields;
}

export type AssetRef = { type: "asset"; id: string } | { type: "url"; href: string };

export interface IrInline extends IrFields {
  type: string;
  text?: string;
  content?: IrInline[];
  style?: string;
  direct?: CharacterStyle;
  url?: string;
  title?: string | null;
  target?: AssetRef;
  alt?: string;
  tex?: string;
  display?: boolean;
  id?: string;
  format?: string;
}

export interface IrListItem extends IrFields {
  checked: boolean | null;
  blocks: IrBlock[];
}

export interface IrTableCell extends IrFields {
  rowspan: number;
  colspan: number;
  header?: boolean;
  blocks: IrBlock[];
}

export interface IrBlock extends IrFields {
  type: string;
  id: string;
  style?: string;
  direct?: ParagraphStyle;
  content?: IrInline[];
  blocks?: IrBlock[];
  items?: IrListItem[];
  head?: IrTableCell[][];
  body?: IrTableCell[][];
  caption?: IrInline[] | null;
  columns?: { align: "default" | "left" | "center" | "right"; [key: string]: unknown }[];
  footnotes?: IrInline[];
  level?: number;
  lang?: string | null;
  text?: string;
  tex?: string;
  display?: boolean;
  ordered?: boolean;
  start?: number | null;
  tight?: boolean;
  asset?: AssetRef;
  note_id?: string;
  format?: string;
}

export interface BlockPosition extends IrFields {
  page_no: number;
  bbox: { left: number; top: number; right: number; bottom: number };
}

export interface IrDocument extends IrFields {
  version: "ariad-ir/2";
  meta: IrFields;
  body: IrBlock[];
  assets?: Record<string, IrFields>;
  furniture?: IrBlock[];
  styles?: StyleSheet;
  layout?: {
    pages?: { page_no: number; width: number; height: number }[];
    blocks?: Record<string, BlockPosition>;
    [key: string]: unknown;
  } | null;
  source_map?: Record<string, BlockPosition[]>;
  sections?: IrFields[];
}

export interface TiptapMark {
  type: string;
  attrs?: Record<string, unknown>;
}

export interface TiptapNode {
  type: string;
  attrs?: Record<string, unknown>;
  content?: TiptapNode[];
  marks?: TiptapMark[];
  text?: string;
}

export interface TiptapDoc extends TiptapNode {
  type: "doc";
  content: TiptapNode[];
}

export interface EditorSnapshot {
  original: IrFields;
  node: TiptapNode;
}

/** Plain JSON, safe to persist or send to a worker. `original` owns out-of-band data. */
export interface EditorDocumentState {
  doc: TiptapDoc;
  original: IrDocument;
  snapshots: Record<string, EditorSnapshot>;
}

export interface EditorChangeItem {
  kind: "added" | "modified" | "deleted" | "style" | "document";
  blockId?: string;
  message: string;
}

export interface EditorChangeSummary {
  addedBlocks: string[];
  modifiedBlocks: string[];
  deletedBlocks: string[];
  /** Block IDs for local styles; `styles` for a changed stylesheet. */
  styleChanges: string[];
  /** Structural retention estimate in [0, 1], not a rendered export benchmark. */
  estimatedFidelityScore: number;
  changeItems: EditorChangeItem[];
}
