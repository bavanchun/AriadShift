// Page mode copy. The editor takes a partial override so the host can localise it; the English
// defaults keep the component usable on its own.

export interface PageModeLabels {
  toolbar: string;
  select: string;
  editText: string;
  addTextBox: string;
  addImage: string;
  deleteSelected: string;
  undo: string;
  redo: string;
  previousPage: string;
  nextPage: string;
  /** `{page}` and `{count}` are replaced. */
  pageOf: string;
  applyEdits: string;
  canvas: string;
  /** `{page}` is replaced. */
  pageSurface: string;
  textRun: string;
  image: string;
  path: string;
  /** `{kind}` and `{summary}` are replaced. */
  objectLabel: string;
  newTextBoxText: string;
  textEditor: string;
  editHint: string;
  inspector: string;
  noSelection: string;
  fontFamily: string;
  fontFamilyOriginal: string;
  fontSize: string;
  color: string;
  position: string;
  x: string;
  y: string;
  w: string;
  h: string;
  substitutionTitle: string;
  /** `{from}` and `{to}` are replaced. */
  substitutionBody: string;
  /** `{chars}` is replaced. */
  substitutionChars: string;
  applyFailed: string;
}

export const DEFAULT_PAGE_MODE_LABELS: PageModeLabels = {
  toolbar: "Page editing tools",
  select: "Select and move",
  editText: "Edit text",
  addTextBox: "Add text box",
  addImage: "Add image",
  deleteSelected: "Delete selected",
  undo: "Undo",
  redo: "Redo",
  previousPage: "Previous page",
  nextPage: "Next page",
  pageOf: "Page {page} of {count}",
  applyEdits: "Apply page edits",
  canvas: "Page objects",
  pageSurface: "Page {page}",
  textRun: "Text",
  image: "Image",
  path: "Shape",
  objectLabel: "{kind}: {summary}",
  newTextBoxText: "New text",
  textEditor: "Edit text in place",
  editHint: "Enter to apply, Escape to cancel",
  inspector: "Object details",
  noSelection: "Select an object on the page to see its details.",
  fontFamily: "Font family",
  fontFamilyOriginal: "Keep original font",
  fontSize: "Font size (pt)",
  color: "Text colour",
  position: "Position and size (pt)",
  x: "x",
  y: "y",
  w: "w",
  h: "h",
  substitutionTitle: "Font substituted",
  substitutionBody: "{from} → {to}",
  substitutionChars: "These characters are not in the embedded font: {chars}",
  applyFailed: "These edits cannot be applied.",
};

export function formatLabel(template: string, values: Record<string, string | number>): string {
  return template.replace(/\{(\w+)\}/g, (match, key: string) =>
    key in values ? String(values[key]) : match,
  );
}
