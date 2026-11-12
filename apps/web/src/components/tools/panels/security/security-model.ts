import type { ParamsValidation } from "../../params-form";

export const SECURITY_TOOLS = [
  "pdf.encrypt",
  "pdf.decrypt",
  "pdf.permissions",
  "pdf.sanitize",
  "pdf.metadata",
  "pdf.unlock_forms",
] as const;
export type SecurityToolId = (typeof SECURITY_TOOLS)[number];

export function isSecurityTool(id: string): id is SecurityToolId {
  return (SECURITY_TOOLS as readonly string[]).includes(id);
}

// ---- passwords --------------------------------------------------------------------------------

export type StrengthLevel = "empty" | "weak" | "fair" | "good" | "strong";

export interface PasswordStrength {
  bits: number;
  level: StrengthLevel;
  /** 0-4, for the meter. */
  score: number;
}

/**
 * Estimated entropy from length and the character classes used. It over-estimates words and
 * patterns, so it is a guide for the user and never a gate: the engine accepts any password.
 */
export function passwordStrength(password: string): PasswordStrength {
  if (!password) return { bits: 0, level: "empty", score: 0 };
  let pool = 0;
  if (/[a-z]/.test(password)) pool += 26;
  if (/[A-Z]/.test(password)) pool += 26;
  if (/\d/.test(password)) pool += 10;
  if (/[^A-Za-z0-9]/.test(password)) pool += 33;
  const distinct = new Set(password).size;
  // Repeated characters add little: count them at a fraction of their length.
  const effectiveLength = distinct + (password.length - distinct) * 0.25;
  const bits = Math.round(effectiveLength * Math.log2(pool));
  const level: StrengthLevel =
    bits < 28 ? "weak" : bits < 50 ? "fair" : bits < 72 ? "good" : "strong";
  const score = { empty: 0, weak: 1, fair: 2, good: 3, strong: 4 }[level];
  return { bits, level, score };
}

// ---- permissions ------------------------------------------------------------------------------

export type PermissionKey = "print" | "copy" | "modify" | "annotate" | "assemble" | "accessibility";

export interface PermissionRow {
  key: PermissionKey;
  label: string;
  detail: string;
  /** `PdfPermissions` fields this toggle controls. */
  fields: readonly string[];
}

export const PERMISSION_ROWS: readonly PermissionRow[] = [
  {
    key: "print",
    label: "Print",
    detail: "Low and high quality printing",
    fields: ["print", "print_high"],
  },
  {
    key: "copy",
    label: "Copy text and graphics",
    detail: "Select and copy content",
    fields: ["copy"],
  },
  { key: "modify", label: "Modify document", detail: "Edit page content", fields: ["modify"] },
  {
    key: "annotate",
    label: "Annotate and fill forms",
    detail: "Comments, highlights and form fields",
    fields: ["annotate", "fill_forms"],
  },
  {
    key: "assemble",
    label: "Extract pages",
    detail: "Insert, rotate, delete and extract pages",
    fields: ["assemble"],
  },
  {
    key: "accessibility",
    label: "Screen-reader access",
    detail: "Let assistive technology read the text",
    fields: ["extract_accessibility"],
  },
];

export type PermissionState = Record<PermissionKey, boolean>;

export const ALL_PERMITTED: PermissionState = {
  print: true,
  copy: true,
  modify: true,
  annotate: true,
  assemble: true,
  accessibility: true,
};

export function buildPermissions(state: PermissionState): Record<string, boolean> {
  const permissions: Record<string, boolean> = {};
  for (const row of PERMISSION_ROWS) {
    for (const field of row.fields) permissions[field] = state[row.key];
  }
  return permissions;
}

// ---- sanitize ---------------------------------------------------------------------------------

export type SanitizeForms = "keep" | "flatten" | "remove";

export interface SanitizeState {
  javascript: boolean;
  embedded_files: boolean;
  links: boolean;
  metadata: boolean;
  xmp: boolean;
  thumbnails: boolean;
  forms: SanitizeForms;
}

export const SANITIZE_DEFAULTS: SanitizeState = {
  javascript: true,
  embedded_files: true,
  links: false,
  metadata: false,
  xmp: false,
  thumbnails: true,
  forms: "keep",
};

export const SANITIZE_ROWS: ReadonlyArray<{
  key: Exclude<keyof SanitizeState, "forms">;
  label: string;
  detail: string;
}> = [
  {
    key: "javascript",
    label: "JavaScript actions",
    detail: "Scripts that run when the file opens",
  },
  { key: "embedded_files", label: "Embedded files", detail: "Attachments hidden inside the PDF" },
  { key: "metadata", label: "Document info metadata", detail: "Author, title, creator and dates" },
  { key: "xmp", label: "XMP metadata", detail: "Extended metadata stream" },
  { key: "links", label: "Launch and URI actions", detail: "Links that open files or web pages" },
  { key: "thumbnails", label: "Page thumbnails", detail: "Embedded preview images" },
];

export const FORM_MODES: ReadonlyArray<{ value: SanitizeForms; label: string }> = [
  { value: "keep", label: "Keep form actions" },
  { value: "flatten", label: "Flatten forms and drop submit actions" },
  { value: "remove", label: "Remove forms and submit actions" },
];

export function countSanitized(state: SanitizeState): number {
  return SANITIZE_ROWS.filter((row) => state[row.key]).length + (state.forms === "keep" ? 0 : 1);
}

// ---- metadata strip ---------------------------------------------------------------------------

export const METADATA_FIELDS = [
  "Author",
  "Title",
  "Subject",
  "Keywords",
  "Creator",
  "Producer",
  "CreationDate",
  "ModDate",
] as const;

export const METADATA_LABELS: Record<(typeof METADATA_FIELDS)[number], string> = {
  Author: "Author",
  Title: "Title",
  Subject: "Subject",
  Keywords: "Keywords",
  Creator: "Creator application",
  Producer: "PDF producer",
  CreationDate: "Creation date",
  ModDate: "Modification date",
};

// ---- validation -------------------------------------------------------------------------------

export function validation(errors: Record<string, string>): ParamsValidation {
  return { isValid: Object.keys(errors).length === 0, errors };
}
