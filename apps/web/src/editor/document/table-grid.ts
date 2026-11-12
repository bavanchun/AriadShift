import type { TiptapNode } from "./mapping";

/**
 * Grid-aware table editing over the rows of one section (head or body). Cells are laid out with
 * the HTML table algorithm, edited as placed rectangles, and written back as rows, so every
 * operation leaves a rectangular grid that the IR core accepts.
 */

export interface PlacedCell {
  node: TiptapNode;
  row: number;
  col: number;
  rowspan: number;
  colspan: number;
}

/** Builds an empty cell; the flag says whether it belongs to a header row. */
export type MakeCell = (header: boolean) => TiptapNode;

const spanOf = (node: TiptapNode, key: "rowspan" | "colspan") =>
  Math.max(1, Math.trunc(Number(node.attrs?.[key] ?? 1)) || 1);

export function placeCells(rows: TiptapNode[]): PlacedCell[] {
  const occupied: boolean[][] = rows.map(() => []);
  const placed: PlacedCell[] = [];
  for (const [row, node] of rows.entries()) {
    let col = 0;
    for (const cell of node.content ?? []) {
      while (occupied[row]?.[col]) col++;
      const rowspan = spanOf(cell, "rowspan");
      const colspan = spanOf(cell, "colspan");
      placed.push({ node: cell, row, col, rowspan, colspan });
      for (let dr = 0; dr < rowspan; dr++) {
        for (let dc = 0; dc < colspan; dc++) {
          const line = occupied[row + dr];
          if (line) line[col + dc] = true;
        }
      }
      col += colspan;
    }
  }
  return placed;
}

export function gridWidth(rows: TiptapNode[]): number {
  return placeCells(rows).reduce((width, cell) => Math.max(width, cell.col + cell.colspan), 0);
}

function withSpans(cell: PlacedCell): TiptapNode {
  const { node } = cell;
  if (spanOf(node, "rowspan") === cell.rowspan && spanOf(node, "colspan") === cell.colspan) {
    return node;
  }
  return { ...node, attrs: { ...node.attrs, rowspan: cell.rowspan, colspan: cell.colspan } };
}

function rebuild(rows: TiptapNode[], cells: PlacedCell[]): TiptapNode[] {
  return rows.map((row, index) => ({
    ...row,
    content: cells
      .filter((cell) => cell.row === index)
      .sort((a, b) => a.col - b.col)
      .map(withSpans),
  }));
}

interface Rect {
  row: number;
  col: number;
  rowspan: number;
  colspan: number;
}

const intersects = (a: Rect, b: Rect) =>
  a.row < b.row + b.rowspan &&
  b.row < a.row + a.rowspan &&
  a.col < b.col + b.colspan &&
  b.col < a.col + a.colspan;

const contains = (outer: Rect, inner: Rect) =>
  inner.row >= outer.row &&
  inner.col >= outer.col &&
  inner.row + inner.rowspan <= outer.row + outer.rowspan &&
  inner.col + inner.colspan <= outer.col + outer.colspan;

const clamp = (value: number, min: number, max: number) =>
  Math.min(Math.max(min, max), Math.max(min, Math.trunc(value)));

const hasContent = (block: TiptapNode) => !(block.type === "paragraph" && !block.content?.length);

/** Finds the cell at a row and its index within that row. */
export function cellAt(rows: TiptapNode[], row: number, cell: number): PlacedCell | undefined {
  return placeCells(rows).filter((placed) => placed.row === row)[cell];
}

/**
 * Merges or splits a cell. Cells swallowed by a larger span are removed (their non-empty
 * blocks move into the merged cell); cells released by a smaller span are recreated empty.
 * Returns null when the new span would cut through another merged cell.
 */
export function setCellSpan(
  rows: TiptapNode[],
  ref: { row: number; cell: number },
  span: { rowspan?: number; colspan?: number },
  width: number,
  makeCell: MakeCell,
): TiptapNode[] | null {
  const placed = placeCells(rows);
  const target = placed.filter((cell) => cell.row === ref.row)[ref.cell];
  if (!target) return null;
  const area: Rect = {
    row: target.row,
    col: target.col,
    rowspan: clamp(span.rowspan ?? target.rowspan, 1, rows.length - target.row),
    colspan: clamp(span.colspan ?? target.colspan, 1, width - target.col),
  };
  const swallowed: PlacedCell[] = [];
  for (const other of placed) {
    if (other === target || !intersects(other, area)) continue;
    if (!contains(area, other)) return null;
    swallowed.push(other);
  }
  const blocks = [
    ...(target.node.content ?? []),
    ...swallowed.flatMap((cell) => (cell.node.content ?? []).filter(hasContent)),
  ];
  const kept = placed
    .filter((cell) => !swallowed.includes(cell))
    .map((cell) =>
      cell === target
        ? {
            ...cell,
            rowspan: area.rowspan,
            colspan: area.colspan,
            node: { ...cell.node, content: blocks },
          }
        : cell,
    );
  const header = target.node.type === "tableHeader";
  for (let r = target.row; r < target.row + target.rowspan; r++) {
    for (let c = target.col; c < target.col + target.colspan; c++) {
      if (!contains(area, { row: r, col: c, rowspan: 1, colspan: 1 })) {
        kept.push({ node: makeCell(header), row: r, col: c, rowspan: 1, colspan: 1 });
      }
    }
  }
  return rebuild(rows, kept);
}

/** Inserts a row at `position`; spans crossing that position grow instead of getting a cell. */
export function insertRow(
  rows: TiptapNode[],
  position: number,
  width: number,
  header: boolean,
  makeCell: MakeCell,
  template: TiptapNode,
): TiptapNode[] {
  const at = clamp(position, 0, rows.length);
  const placed = placeCells(rows);
  const crossing = placed.filter((cell) => cell.row < at && cell.row + cell.rowspan > at);
  const cells: PlacedCell[] = placed.map((cell) => {
    if (cell.row >= at) return { ...cell, row: cell.row + 1 };
    return crossing.includes(cell) ? { ...cell, rowspan: cell.rowspan + 1 } : cell;
  });
  const covered = new Set(
    crossing.flatMap((cell) => Array.from({ length: cell.colspan }, (_, i) => cell.col + i)),
  );
  for (let col = 0; col < width; col++) {
    if (!covered.has(col)) {
      cells.push({ node: makeCell(header), row: at, col, rowspan: 1, colspan: 1 });
    }
  }
  const next = [...rows.slice(0, at), { ...template, content: [] }, ...rows.slice(at)];
  return rebuild(next, cells);
}

/** Removes a row; cells that started in it continue into the next row. */
export function removeRow(rows: TiptapNode[], index: number): TiptapNode[] | null {
  if (rows.length <= 1 || !rows[index]) return null;
  const cells: PlacedCell[] = [];
  for (const cell of placeCells(rows)) {
    if (cell.row === index) {
      if (cell.rowspan > 1) cells.push({ ...cell, rowspan: cell.rowspan - 1 });
    } else if (cell.row < index && cell.row + cell.rowspan > index) {
      cells.push({ ...cell, rowspan: cell.rowspan - 1 });
    } else {
      cells.push(cell.row > index ? { ...cell, row: cell.row - 1 } : cell);
    }
  }
  return rebuild(
    rows.filter((_, position) => position !== index),
    cells,
  );
}

/** Appends one cell to every row at the right edge of the grid. */
export function appendColumn(
  rows: TiptapNode[],
  width: number,
  header: boolean,
  makeCell: MakeCell,
): TiptapNode[] {
  const cells = placeCells(rows);
  for (const [row] of rows.entries()) {
    cells.push({ node: makeCell(header), row, col: width, rowspan: 1, colspan: 1 });
  }
  return rebuild(rows, cells);
}

/** Removes one grid column; merged cells that cross it become one column narrower. */
export function removeColumn(rows: TiptapNode[], column: number): TiptapNode[] {
  const cells: PlacedCell[] = [];
  for (const cell of placeCells(rows)) {
    if (column < cell.col) cells.push({ ...cell, col: cell.col - 1 });
    else if (column >= cell.col + cell.colspan) cells.push(cell);
    else if (cell.colspan > 1) cells.push({ ...cell, colspan: cell.colspan - 1 });
  }
  return rebuild(rows, cells);
}
