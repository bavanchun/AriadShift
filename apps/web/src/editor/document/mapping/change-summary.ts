import { allBlocks } from "./document-mapping";
import { equal } from "./snapshots";
import type { EditorChangeSummary, IrBlock, IrDocument, IrInline } from "./types";

function inlineStyles(content: IrInline[] = []): unknown[] {
  return content.flatMap((inline) => {
    const nested = inlineStyles(inline.content);
    if (
      [
        "styled",
        "emph",
        "strong",
        "strikeout",
        "superscript",
        "subscript",
        "code",
        "link",
      ].includes(inline.type)
    ) {
      return [{ type: inline.type, style: inline.style, direct: inline.direct, content: nested }];
    }
    return nested;
  });
}

function localStyles(block: IrBlock | undefined): unknown {
  return {
    style: block?.style,
    direct: block?.direct,
    content: inlineStyles(block?.content),
    caption: inlineStyles(block?.caption ?? []),
    footnotes: inlineStyles(block?.footnotes),
    cells: [...(block?.head ?? []), ...(block?.body ?? [])].map((row) =>
      row.map((cell) => ({
        style: cell.style,
        direct: cell.direct,
      })),
    ),
  };
}

export function summarizeEditorChanges(
  original: IrDocument,
  current: IrDocument,
): EditorChangeSummary {
  const before = new Map(allBlocks(original).map((block) => [block.id, block]));
  const after = new Map(allBlocks(current).map((block) => [block.id, block]));
  // Include changed container subtrees, matching edit.export_document's BlockChangeList.
  const addedBlocks = [...after.keys()].filter((id) => !before.has(id));
  const deletedBlocks = [...before.keys()].filter((id) => !after.has(id));
  const modifiedBlocks = [...after.keys()].filter(
    (id) => before.has(id) && !equal(before.get(id), after.get(id)),
  );
  const styleChanges = modifiedBlocks.filter((id) => {
    return !equal(localStyles(before.get(id)), localStyles(after.get(id)));
  });
  if (!equal(original.styles, current.styles)) styleChanges.push("styles");
  const changeItems: EditorChangeSummary["changeItems"] = [];
  for (const [kind, ids] of [
    ["added", addedBlocks],
    ["modified", modifiedBlocks],
    ["deleted", deletedBlocks],
    ["style", styleChanges],
  ] as const) {
    for (const blockId of ids) changeItems.push({ kind, blockId, message: `${kind}: ${blockId}` });
  }
  for (const field of new Set([...Object.keys(original), ...Object.keys(current)])) {
    if (["body", "furniture", "styles"].includes(field)) continue;
    if (!equal(original[field], current[field])) {
      changeItems.push({ kind: "document", message: `Document ${field} changed` });
    }
  }
  const order = (doc: IrDocument) => [
    doc.body.map((block) => block.id),
    (doc.furniture ?? []).map((block) => block.id),
  ];
  if (!equal(order(original), order(current))) {
    changeItems.push({ kind: "document", message: "Document block order changed" });
  }
  // This preview measures retained block identities/types and out-of-band layers.
  // It deliberately does not claim to predict font substitution or rendered fidelity.
  const layers = ["assets", "layout", "styles", "sections", "source_map"];
  let total = before.size;
  let retained = [...before].filter(([id, block]) => after.get(id)?.type === block.type).length;
  for (const field of layers) {
    if (original[field] != null && Object.keys(original[field] as object).length) {
      total++;
      if (equal(original[field], current[field])) retained++;
    }
  }
  return {
    addedBlocks,
    modifiedBlocks,
    deletedBlocks,
    styleChanges,
    estimatedFidelityScore: total ? retained / total : 1,
    changeItems,
  };
}
