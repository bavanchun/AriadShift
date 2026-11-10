// English strings of the page organiser studio, kept apart from the markup so the catalogue
// extraction can lift them without touching the components.
const plural = (count: number, one: string, other: string) =>
  `${count} ${count === 1 ? one : other}`;

export const organiserCopy = {
  selector: {
    group: "Quick selection",
    all: "All pages",
    odd: "Odd pages",
    even: "Even pages",
    first: "First page",
    last: "Last page",
    custom: "Custom range",
    input: "Page range",
    hint: "Examples: 1-3, 5, last-1",
    selected: (count: number, total?: number) =>
      total === undefined
        ? `${plural(count, "page", "pages")} selected`
        : `${count} of ${plural(total, "page", "pages")} selected`,
    repeats: "Each page can be listed only once.",
    keepOne: "At least one page must remain.",
  },
  split: {
    mode: "Split method",
    ranges: "By page ranges",
    every: "Every N pages",
    points: "Split points",
    rangesInput: "Ranges, one output per entry",
    rangesHint: "Example: 1-3, 4-7, 8-last",
    everyInput: "Pages per part",
    everyInvalid: "Enter a whole number of at least 1.",
    pointsHint: "Choose where one part ends and the next begins.",
    pointsTooMany: "Use page ranges for documents over 200 pages.",
    needsPages: "The page count is not known yet.",
    marker: (page: number) => `Split after page ${page}`,
    outputs: (count: number) => `Creates ${plural(count, "PDF", "PDFs")}`,
    pillList: "Pages and split points",
  },
  rotate: {
    angle: "Rotation",
    ccw: "90° CCW",
    cw: "90° CW",
    half: "180°",
    preview: (angle: number) => `Preview of a page rotated ${angle} degrees`,
  },
  blank: {
    after: "Insert after pages",
    count: "Blank pages at each position",
    countInvalid: "Enter a whole number of at least 1.",
    size: "Blank page size",
    same: "Match source",
    result: (from: number, to: number) => `${from} → ${to} pages`,
    strip: "Result preview",
    blank: "Blank",
  },
  reorder: {
    preset: "Order",
    reverse: "Reverse",
    duplex: "Duplex interleave",
    oddEven: "Odd then even",
    custom: "Custom order",
    customInput: "Custom page order",
    customHint: "Pages in the order you want them, e.g. 3, 1, 2-last",
    result: "New order",
  },
  merge: {
    order: "Merge order",
    orderHint: "Files merge top to bottom. Reorder them in the file tray.",
    outline: "Bookmarks",
    pageLabels: "Page labels",
    total: (files: number, pages: number | null) =>
      pages === null
        ? plural(files, "file", "files")
        : `${plural(files, "file", "files")}, ${plural(pages, "page", "pages")}`,
    keep: "Keep",
    perFile: "One per file",
    none: "None",
    renumber: "Renumber",
    needsMore: (min: number) => `Add at least ${min} PDFs to continue.`,
    tooMany: (max: number) => `This tool accepts at most ${max} PDFs.`,
    pages: (count: number | null) =>
      count === null ? "pages unknown" : plural(count, "page", "pages"),
  },
  mix: {
    diagram: "Interleave preview",
    fileA: "File A",
    fileB: "File B",
    reverse: "Take the second file in reverse order",
    example: "Example with 4 pages in each file.",
    more: "…",
  },
} as const;
