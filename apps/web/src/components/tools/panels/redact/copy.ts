// English strings of the redaction studio, kept apart from the markup so catalogue extraction can lift them.
export const redactCopy = {
  notice: {
    title: "Redaction is permanent",
    body: "Matched content is removed from the PDF itself, not covered with a black box. The result cannot be undone, so keep the original file. Signatures on the document are invalidated by the rewrite.",
  },
  targets: {
    title: "What to remove",
    terms: "Words or phrases",
    termInput: "Add a word or phrase",
    termHint: "Matches the exact text wherever it appears. Press Enter to add it.",
    add: "Add",
    remove: (label: string) => `Remove ${label}`,
    patterns: "Sensitive patterns",
    customRegex: "Custom Regex",
    regexInput: "Regular expression",
    regexHint: "Example: INV-\\d{6}. Lookahead and back-references are not supported.",
    regexOk: "Valid pattern.",
    areas: "Page areas",
    areasHint: "Coordinates are in PDF points from the bottom-left corner of the page.",
    page: "Page",
    left: "Left",
    bottom: "Bottom",
    right: "Right",
    top: "Top",
    addArea: "Add area",
    area: (text: string) => `Area ${text}`,
    summary: (count: number) => `${count} ${count === 1 ? "item" : "items"} selected`,
    none: "Add at least one word, pattern or area to redact.",
    tooMany: (limit: number) => `Use at most ${limit} words, patterns and detectors.`,
    list: "Items to remove",
  },
  appearance: {
    title: "Redaction appearance",
    fill: "Fill colour",
    fillPreset: (color: string) => `Use fill ${color}`,
    custom: "Custom fill colour",
    label: "Label on the redacted area",
    presets: "Label presets",
    labelNone: "No label",
    labelCustom: "Custom label",
  },
  scope: {
    title: "Parts of the document to clean",
    text: "Text and text-bearing structures",
    images: "Image pixels and masks",
    vectors: "Vector graphics",
    annotations: "Annotations and form fields",
    metadata: "Document metadata (Info and XMP)",
    attachments: "Embedded files",
    partial: "Leaving a part out makes the verification report partial.",
  },
  signatures: {
    label: "Allow this redaction to invalidate existing signatures",
    hint: "Redaction rewrites the whole file, which breaks any digital signature. Required if the document is signed.",
  },
  verification: {
    title: "Verification report",
    plannedTitle: "What is checked after you apply",
    plannedBody:
      "After the rewrite, the PDF is read back and checked by six independent gates. The report appears here when the run finishes.",
    status: {
      verified: "Verified",
      partial: "Partial",
      unverifiable: "Unverifiable",
      failed: "Failed",
      planned: "Pending",
    },
    statusBody: {
      verified:
        "Every gate passed for every part of the document: the matched content is physically removed and cannot be copied or searched.",
      partial:
        "The gates that ran passed, but some parts of the document were left out of scope, so the result is not a full guarantee.",
      unverifiable:
        "A gate could not establish its claim. Do not rely on this file until it is verified.",
      failed: "A gate found redacted content that is still present. Do not share this file.",
    },
    check: { pass: "Passed", fail: "Failed", unavailable: "Unavailable", pending: "Pending" },
    checks: {
      pdfium_text: {
        title: "Text extraction (PDFium)",
        body: "A fresh text extraction finds none of the redacted glyph runs.",
      },
      core_text: {
        title: "Independent text re-extraction",
        body: "A second, separate reader also finds no redacted text.",
      },
      region_render: {
        title: "Opaque rendering",
        body: "Every redacted region renders as a solid block.",
      },
      image_pixels: {
        title: "Image pixels cleared",
        body: "Original image pixels and masks under the regions are removed.",
      },
      object_graph: {
        title: "Object graph scan",
        body: "Decoded streams, annotations, metadata and attachments hold no redacted content.",
      },
      rewrite_history: {
        title: "Rewrite and history",
        body: "The file is rewritten canonically, with no earlier revisions left behind.",
      },
    },
    summary: { targets: "Targets", matches: "Occurrences removed" },
    carriers: {
      title: "Removed from the file",
      text_removed: "Text occurrences",
      images_modified: "Images modified",
      vectors_inspected: "Vector paths inspected",
      annotations_removed: "Annotations removed",
      metadata_scrubbed: "Metadata entries scrubbed",
      attachments_removed: "Embedded files removed",
    },
    warnings: "Warnings",
    standalone:
      "A standalone check cannot see the original file, so it never reports Verified; it only reports what it can still find.",
  },
};
