// Standard sheet sizes in PDF points (1 pt = 1/72 in), as the engine resolves them.
export const PAPER_POINTS = {
  A3: { w: 841.89, h: 1190.55 },
  A4: { w: 595.28, h: 841.89 },
  A5: { w: 419.53, h: 595.28 },
  Letter: { w: 612, h: 792 },
  Legal: { w: 612, h: 1008 },
} as const;

export type PaperName = keyof typeof PAPER_POINTS;
export const PAPER_NAMES = [
  "A4",
  "Letter",
  "Legal",
  "A3",
  "A5",
] as const satisfies readonly PaperName[];

export interface Size {
  w: number;
  h: number;
}

export const toMillimetres = (points: number): number => (points * 25.4) / 72;

/** The `{ w, h }` of a sheet, swapped for landscape. */
export function sheetSize(name: PaperName, landscape = false): Size {
  const { w, h } = PAPER_POINTS[name];
  return landscape ? { w: h, h: w } : { w, h };
}

/** Grid of an N-up sheet, oriented like the engine: wide sheets get more columns. */
export function gridDimensions(perSheet: number, sheet: Size): { cols: number; rows: number } {
  const landscape = sheet.w >= sheet.h;
  switch (perSheet) {
    case 4:
      return { cols: 2, rows: 2 };
    case 6:
      return landscape ? { cols: 3, rows: 2 } : { cols: 2, rows: 3 };
    case 8:
      return landscape ? { cols: 4, rows: 2 } : { cols: 2, rows: 4 };
    case 9:
      return { cols: 3, rows: 3 };
    case 16:
      return { cols: 4, rows: 4 };
    default:
      return landscape ? { cols: 2, rows: 1 } : { cols: 1, rows: 2 };
  }
}

/** Page numbers of the booklet spreads: each sheet has a front and back with a left and right page. */
export function bookletSheets(
  pageCount: number,
  binding: "left" | "right",
): Array<{ front: [number, number]; back: [number, number] }> {
  const padded = Math.ceil(pageCount / 4) * 4;
  const sheets: Array<{ front: [number, number]; back: [number, number] }> = [];
  for (let index = 0; index < padded / 4; index++) {
    const front: [number, number] = [padded - 2 * index, 2 * index + 1];
    const back: [number, number] = [2 * index + 2, padded - 2 * index - 1];
    sheets.push(
      binding === "left"
        ? { front, back }
        : { front: [front[1], front[0]], back: [back[1], back[0]] },
    );
  }
  return sheets;
}
