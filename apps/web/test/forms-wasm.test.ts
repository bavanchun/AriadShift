import { readFileSync } from "node:fs";
import * as ariadWasm from "@ariadshift/ariad-wasm";
import { beforeAll, describe, expect, it } from "vitest";

const budget = 64 * 1024 * 1024;
const fixture = new Uint8Array(
  readFileSync(new URL("../../../fixtures/pdf-forms/forms-all-types.pdf", import.meta.url)),
);

function run(id: string, pdf: Uint8Array, params: object, payload?: Uint8Array) {
  const inputs = payload ? [pdf, payload] : [pdf];
  const names = payload ? ["form.pdf", "data.bin"] : ["form.pdf"];
  return ariadWasm.runTool(inputs, names, id, JSON.stringify(params), budget, false);
}

beforeAll(() => {
  ariadWasm.initSync({
    module: readFileSync(
      new URL("../../../packages/ariad-wasm/pkg/ariad_wasm_bg.wasm", import.meta.url),
    ),
  });
});

describe("form and annotation tools in WebAssembly", () => {
  it("creates, fills, lists, exchanges and flattens AcroForm fields", () => {
    const ids = ariadWasm.listTools().map((tool) => tool.id);
    expect(ids).toContain("pdf.forms_create");
    expect(ids).not.toContain("pdf.forms_detect");
    const created = run("pdf.forms_create", fixture, {
      fields: [{ type: "text", name: "wasm_new", page: 1, rect: [20, 20, 160, 45] }],
    });
    const filled = run("pdf.forms_fill", created.bytes, { values: { wasm_new: "An Nguyễn" } });
    const listing = run("pdf.forms_list", filled.bytes, {});
    const fields = listing.data?.fields as Array<{ name: string; value: unknown }>;
    expect(fields.find((field) => field.name === "wasm_new")?.value).toBe("An Nguyễn");
    for (const format of ["json", "csv", "fdf", "xfdf"]) {
      const exported = run("pdf.forms_export", filled.bytes, { format });
      expect(exported.format).toBe(format);
      const imported = run("pdf.forms_import", filled.bytes, {}, exported.bytes);
      expect(imported.format).toBe("pdf");
    }
    const flattened = run("pdf.forms_flatten", filled.bytes, { fields: ["wasm_new"] });
    const after = run("pdf.forms_list", flattened.bytes, {});
    const remaining = after.data?.fields as Array<{ name: string }>;
    expect(remaining.some((field) => field.name === "wasm_new")).toBe(false);
  }, 30_000);

  it("adds, exports and imports annotations", () => {
    const added = run("pdf.annotations", fixture, {
      action: "add",
      items: [
        { id: "wasm-note", type: "note", page: 1, rect: [20, 60, 45, 85], contents: "Ghi chú" },
      ],
    });
    const exported = run("pdf.annotations_export", added.bytes, { format: "json" });
    expect(exported.format).toBe("json");
    const imported = run("pdf.annotations_import", fixture, { replace: true }, exported.bytes);
    const listing = run("pdf.annotations", imported.bytes, { action: "list" });
    const annotations = listing.data?.annotations as Array<{ id: string; contents?: string }>;
    expect(annotations.find((item) => item.id === "wasm-note")?.contents).toBe("Ghi chú");
  }, 30_000);
});
