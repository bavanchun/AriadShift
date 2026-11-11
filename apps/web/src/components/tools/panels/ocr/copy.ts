// English strings of the OCR studio, kept apart from the markup so catalogue extraction can lift them.
const plural = (count: number, one: string, other: string) =>
  `${count} ${count === 1 ? one : other}`;

export const ocrCopy = {
  languages: {
    title: "Recognition languages",
    available: "Available languages",
    search: "Search languages",
    searchHint: "Search by name or code, for example vie or Japanese.",
    selected: "Selected languages, in priority order",
    none: "Choose at least one language.",
    empty: "No language matches your search.",
    remove: (name: string) => `Remove ${name}`,
    add: (name: string) => `Add ${name}`,
    primary: "Primary",
  },
  prep: {
    title: "Scan clean-up",
    deskew: "Straighten skewed pages",
    deskewHint: "Detects small rotation and levels the text lines before recognising.",
    autoRotate: "Fix page orientation",
    autoRotateHint:
      "Turns sideways or upside-down pages upright using the detected text direction.",
    clean: "Clean the background",
    cleanHint: "Removes speckle and binarises image-only pages for sharper recognition.",
  },
  existing: {
    title: "Pages that already have text",
    skip: "Skip",
    skipHint: "Leave pages with selectable text untouched.",
    force: "Force OCR",
    forceHint: "Recognise every page and add a new text layer.",
    redo: "Redo",
    redoHint: "Replace the existing text layer with fresh recognition.",
  },
  output: {
    title: "Output",
    pdf: "Searchable PDF",
    pdfTxt: "Searchable PDF + plain text",
    pdfHocr: "Searchable PDF + hOCR",
    txt: "Plain text",
    hocr: "hOCR",
    imagePdf: "Searchable PDF",
  },
  fields: {
    pages: "Pages to recognise",
    pagesHint: "Examples: all, 1-3, 5, last-1",
    dpi: "Render resolution (DPI)",
    dpiHint: (min: number, max: number) =>
      `Between ${min} and ${max}. Higher is slower but sharper.`,
    maxAngle: "Largest angle to correct (degrees)",
    minAngle: "Ignore skew below (degrees)",
    minConfidence: "Minimum confidence to rotate",
    minConfidenceHint: "Pages whose orientation score is lower are left as they are.",
  },
  preview: {
    title: "Scan versus text layer",
    scanned: "Scanned page image",
    layer: "Searchable text layer",
    overlay: "Invisible overlay",
    plain: "Plain text extraction",
    overlayNote:
      "The recognised words are placed invisibly on top of the scan, so the page looks identical but you can search, select and copy it.",
    plainNote:
      "Only the recognised characters are returned, without the scan or its layout. This is what a text export produces.",
    mode: "Preview of the text layer",
    sample: (languages: number) => `Sample text in ${plural(languages, "language", "languages")}`,
  },
  summary: {
    deskew: "Deskew pages with visible tilt.",
    autoRotate: "Turn pages upright using the text direction.",
  },
};
