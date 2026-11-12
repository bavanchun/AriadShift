/** Every user-visible string of Document mode. Hosts translate by passing `labels`. */
export interface DocumentModeLabels {
  editorLabel: string;
  canvasLabel: string;
  toolbarLabel: string;
  // History and clipboard
  undo: string;
  redo: string;
  // Block types
  blockType: string;
  paragraph: string;
  heading: string; // "Heading {level}"
  quote: string;
  codeBlock: string;
  bulletList: string;
  orderedList: string;
  // Styles
  namedStyle: string;
  noNamedStyle: string;
  fontFamily: string;
  fontSize: string;
  alignment: string;
  alignLeft: string;
  alignCenter: string;
  alignRight: string;
  alignJustify: string;
  // Inline marks
  bold: string;
  italic: string;
  underline: string;
  strikethrough: string;
  code: string;
  subscript: string;
  superscript: string;
  link: string;
  linkUrl: string;
  linkTitle: string;
  applyLink: string;
  removeLink: string;
  // Insert
  insertTable: string;
  insertImage: string;
  insertMath: string;
  insertRule: string;
  insertFootnote: string;
  // Block actions
  blockActions: string;
  moveUp: string;
  moveDown: string;
  duplicateBlock: string;
  deleteBlock: string;
  // Panels and actions
  stylesPanel: string;
  versionHistory: string;
  saveCheckpoint: string;
  export: string;
  close: string;
  // Autosave status
  autosaveIdle: string;
  autosavePending: string;
  autosaveSaving: string;
  autosaveSaved: string; // "Saved {time}"
  autosaveFailed: string;
  autosaveOff: string;
  autosaveRedacted: string;
  // Canvas
  emptyDocument: string;
  emptyBlock: string;
  figureCaption: string;
  imageUnavailable: string;
  tableCaption: string;
  footnote: string; // "Footnote {id}"
  footnoteReference: string; // "Footnote reference {id}"
  inlineMath: string;
  mathBlock: string;
  mathSource: string;
  mathDisplay: string;
  rawBlock: string; // "Raw {format} block"
  rawBlockNotice: string;
  unknownBlock: string;
  pageBreak: string;
  codeLanguage: string;
  taskItem: string;
  inlineAtomEditor: string;
  // Tables
  tableTools: string;
  addRow: string;
  addColumn: string;
  deleteRow: string;
  deleteColumn: string;
  toggleHeaderRow: string;
  rowSpan: string;
  columnSpan: string;
  tableOperationUnavailable: string;
  // Styles panel
  stylesTitle: string;
  selectedBlock: string;
  noBlockSelected: string;
  paragraphStyle: string;
  directFormatting: string;
  resetDirect: string;
  lineHeight: string;
  spaceBefore: string;
  spaceAfter: string;
  textColor: string;
  backgroundColor: string;
  keepWithNext: string;
  pageBreakBefore: string;
  characterFormatting: string;
  characterStyle: string;
  noCharacterStyle: string;
  documentStyles: string;
  editParagraphStyle: string;
  editCharacterStyle: string;
  newStyleName: string;
  createStyle: string;
  deleteStyle: string;
  basedOn: string;
  weight: string;
  italicStyle: string;
  underlineStyle: string;
  // Export dialog
  exportTitle: string;
  exportDescription: string;
  exportFormat: string;
  exportPdf: string;
  exportDocx: string;
  exportMarkdown: string;
  exportHtml: string;
  fidelityScore: string;
  fidelityNote: string;
  whatChanged: string;
  noChanges: string;
  addedBlocks: string;
  modifiedBlocks: string;
  deletedBlocks: string;
  styleChanges: string;
  moreChanges: string; // "and {count} more"
  confirmExport: string;
  cancel: string;
  exportFailed: string;
  // Version history
  historyTitle: string;
  historyEmpty: string;
  historyKindAutosave: string;
  historyKindManual: string;
  historySize: string; // "{bytes} bytes"
  restoreVersion: string;
  checkpointLabel: string;
  saveManualCheckpoint: string;
  historyError: string;
  purgeHistory: string;
  redactionPurged: string; // "{count}"
  redactionDisabledAutosave: string;
  versionRestored: string;
  // Live announcements
  announceMovedUp: string;
  announceMovedDown: string;
  announceDuplicated: string;
  announceDeleted: string;
  announceInserted: string;
  announceCheckpoint: string;
}

export const defaultDocumentModeLabels: DocumentModeLabels = {
  editorLabel: "Document editor",
  canvasLabel: "Document content",
  toolbarLabel: "Document formatting",
  undo: "Undo",
  redo: "Redo",
  blockType: "Block type",
  paragraph: "Paragraph",
  heading: "Heading {level}",
  quote: "Quote",
  codeBlock: "Code block",
  bulletList: "Bulleted list",
  orderedList: "Numbered list",
  namedStyle: "Paragraph style",
  noNamedStyle: "Normal",
  fontFamily: "Font family",
  fontSize: "Font size (pt)",
  alignment: "Alignment",
  alignLeft: "Align left",
  alignCenter: "Align center",
  alignRight: "Align right",
  alignJustify: "Justify",
  bold: "Bold",
  italic: "Italic",
  underline: "Underline",
  strikethrough: "Strikethrough",
  code: "Code",
  subscript: "Subscript",
  superscript: "Superscript",
  link: "Link",
  linkUrl: "Link URL",
  linkTitle: "Link title",
  applyLink: "Apply link",
  removeLink: "Remove link",
  insertTable: "Insert table",
  insertImage: "Insert image or figure",
  insertMath: "Insert math (TeX)",
  insertRule: "Insert horizontal rule",
  insertFootnote: "Insert footnote",
  blockActions: "Block actions",
  moveUp: "Move block up",
  moveDown: "Move block down",
  duplicateBlock: "Duplicate block",
  deleteBlock: "Delete block",
  stylesPanel: "Styles panel",
  versionHistory: "Version history",
  saveCheckpoint: "Save checkpoint",
  export: "Export",
  close: "Close",
  autosaveIdle: "No changes yet",
  autosavePending: "Unsaved changes",
  autosaveSaving: "Saving…",
  autosaveSaved: "Saved {time}",
  autosaveFailed: "Autosave failed",
  autosaveOff: "Autosave off",
  autosaveRedacted: "History purged",
  emptyDocument: "This document has no content yet.",
  emptyBlock: "Empty block",
  figureCaption: "Figure caption",
  imageUnavailable: "Image not shown: it is external or missing",
  tableCaption: "Table caption",
  footnote: "Footnote {id}",
  footnoteReference: "Footnote reference {id}",
  inlineMath: "Inline math",
  mathBlock: "Math block",
  mathSource: "TeX source",
  mathDisplay: "Display style",
  rawBlock: "Raw {format} block",
  rawBlockNotice: "Raw content is kept exactly as written and exported only to matching formats.",
  unknownBlock: "This block type cannot be edited here. It is preserved unchanged.",
  pageBreak: "Page break",
  codeLanguage: "Code language",
  taskItem: "Task item",
  inlineAtomEditor: "Edit inline element",
  tableTools: "Table tools",
  addRow: "Add row",
  addColumn: "Add column",
  deleteRow: "Delete row",
  deleteColumn: "Delete column",
  toggleHeaderRow: "Toggle header row",
  rowSpan: "Row span",
  columnSpan: "Column span",
  tableOperationUnavailable:
    "This change would cut through a merged cell or remove the last row or column.",
  stylesTitle: "Styles",
  selectedBlock: "Selected block",
  noBlockSelected: "Select a block to format it.",
  paragraphStyle: "Paragraph style",
  directFormatting: "Direct formatting",
  resetDirect: "Reset direct formatting",
  lineHeight: "Line height",
  spaceBefore: "Space before (pt)",
  spaceAfter: "Space after (pt)",
  textColor: "Text color",
  backgroundColor: "Background",
  keepWithNext: "Keep with next",
  pageBreakBefore: "Page break before",
  characterFormatting: "Selected text",
  characterStyle: "Character style",
  noCharacterStyle: "None",
  documentStyles: "Document styles",
  editParagraphStyle: "Edit paragraph style",
  editCharacterStyle: "Edit character style",
  newStyleName: "New style name",
  createStyle: "Create style",
  deleteStyle: "Delete style",
  basedOn: "Based on",
  weight: "Weight",
  italicStyle: "Italic",
  underlineStyle: "Underline",
  exportTitle: "Export document",
  exportDescription: "Review what changed before the document is converted.",
  exportFormat: "Export format",
  exportPdf: "PDF",
  exportDocx: "DOCX",
  exportMarkdown: "Markdown",
  exportHtml: "HTML",
  fidelityScore: "Fidelity score",
  fidelityNote:
    "A structural estimate of how much of the original is retained, not a rendered score.",
  whatChanged: "What changed",
  noChanges: "No changes since the document was loaded.",
  addedBlocks: "Added blocks",
  modifiedBlocks: "Modified blocks",
  deletedBlocks: "Deleted blocks",
  styleChanges: "Style changes",
  moreChanges: "and {count} more",
  confirmExport: "Confirm export",
  cancel: "Cancel",
  exportFailed: "This document cannot be exported yet: {reason}",
  historyTitle: "Version history",
  historyEmpty: "No saved versions yet.",
  historyKindAutosave: "Autosave",
  historyKindManual: "Checkpoint",
  historySize: "{bytes} bytes",
  restoreVersion: "Restore version",
  checkpointLabel: "Checkpoint label",
  saveManualCheckpoint: "Save checkpoint",
  historyError: "Version history is unavailable: {reason}",
  purgeHistory: "Purge history after redaction",
  redactionPurged:
    "Redaction purged {count} saved version(s) from local OPFS history so redacted content cannot be recovered from earlier snapshots.",
  redactionDisabledAutosave:
    "Autosave is off for this document. Save the redacted result as a new document to start a new history.",
  versionRestored: "Version restored",
  announceMovedUp: "Block moved up",
  announceMovedDown: "Block moved down",
  announceDuplicated: "Block duplicated",
  announceDeleted: "Block deleted",
  announceInserted: "Block inserted",
  announceCheckpoint: "Checkpoint saved",
};

export function resolveLabels(overrides?: Partial<DocumentModeLabels>): DocumentModeLabels {
  return { ...defaultDocumentModeLabels, ...overrides };
}

/** Replaces `{name}` placeholders. */
export function fillLabel(template: string, values: Record<string, string | number>): string {
  return template.replace(/\{(\w+)\}/g, (match, key: string) =>
    key in values ? String(values[key]) : match,
  );
}
