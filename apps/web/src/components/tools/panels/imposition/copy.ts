// English strings of the imposition and crop studio.
const plural = (count: number, one: string, other: string) =>
  `${count} ${count === 1 ? one : other}`;

export const impositionCopy = {
  paper: "Sheet size",
  pages: "Pages",
  nup: {
    perSheet: "Pages per sheet",
    order: "Fill order",
    row: "Across, then down",
    column: "Down, then across",
    landscape: "Landscape sheet",
    borders: "Draw a border around each page",
    margins: "Margins (pt)",
    marginsInvalid: "Margins must leave room for the pages.",
    preview: (perSheet: number) => `${perSheet}-up sheet layout preview`,
    sheets: (pages: number, sheets: number) =>
      `${plural(pages, "page", "pages")} → ${plural(sheets, "sheet", "sheets")}`,
    sample: "Preview shows a full sheet.",
  },
  booklet: {
    binding: "Binding edge",
    left: "Left (LTR)",
    right: "Right (RTL)",
    creep: "Creep compensation (pt)",
    creepInvalid: "Enter a number of at least 0.",
    sheetLabel: (index: number) => `Sheet ${index}`,
    front: "Front",
    back: "Back",
    blank: "Blank",
    sheets: (count: number, pages: number) =>
      `${plural(pages, "page", "pages")} → ${plural(count, "sheet", "sheets")}, folded and stitched`,
    more: (count: number) => `+ ${plural(count, "more sheet", "more sheets")}`,
    sample: "Example with 8 pages.",
    preview: "Booklet signature preview",
  },
  scale: {
    fit: "Fit mode",
    contain: "Fit",
    cover: "Fill",
    stretch: "Stretch",
    preview: (paper: string, fit: string) => `Preview of a page scaled onto ${paper} (${fit})`,
    readout: (x: number, y: number) => `Content scaled to ${x}% × ${y}%`,
    example: "Example: an A4 portrait page.",
  },
  single: {
    preview: "Continuous strip preview",
    summary: (count: number | null) =>
      count === null
        ? "All pages become one continuous page."
        : `${plural(count, "page", "pages")} → 1 continuous page`,
    noOptions: "This tool has no options. Pages are joined top to bottom with no gap.",
  },
  crop: {
    reference: "Reference page",
    target: "Box to set",
    crop: "Crop box",
    media: "Media box",
    presets: "Trim presets",
    none: "No trim",
    quarter: "0.25 in",
    half: "0.5 in",
    inch: "1 in",
    margin: (side: "top" | "right" | "bottom" | "left") =>
      ({ top: "Top margin", right: "Right margin", bottom: "Bottom margin", left: "Left margin" })[
        side
      ],
    unit: "pt",
    margins: "Margins from the page edge (pt)",
    preview: "Crop box preview",
    readout: (w: number, h: number, wMm: number, hMm: number) =>
      `Cropped size: ${w} × ${h} pt (${wMm} × ${hMm} mm)`,
    negative: "Margins cannot be negative.",
    empty: "The crop box would be empty.",
    note: "Coordinates assume pages of the reference size starting at 0, 0.",
  },
} as const;
