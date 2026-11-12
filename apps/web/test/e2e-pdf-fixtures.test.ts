import { MemoryStore } from "@/converter/file-store";
import type { ToolExecutionResult } from "@/converter/protocol";
import { ConverterSession } from "@/converter/session";
import { declaredPageCount, makePdf, pageMarkers, pageRotations } from "../e2e/pdf-fixtures";
import { loadCore } from "./wasm";

// The end-to-end tests build their PDFs and read tool output with these helpers, so check
// them here against the same core the browser runs.
const session = () => new ConverterSession(loadCore(), new MemoryStore());
const file = (name: string, pages: number, label = "P") =>
  new File([new Uint8Array(makePdf(pages, label))], name, { type: "application/pdf" });

async function output(result: ToolExecutionResult): Promise<Buffer> {
  const location = result.primaryLocation;
  if (location.kind !== "memory") throw new Error("expected an in-memory output");
  return Buffer.from(await location.blob.arrayBuffer());
}

describe("end-to-end PDF fixtures", () => {
  it("builds PDFs the core accepts, with the page count and markers asked for", async () => {
    const pdf = makePdf(7, "Q");
    expect(declaredPageCount(pdf)).toBe(7);
    expect(pageMarkers(pdf, "Q")).toEqual(["Q-1", "Q-2", "Q-3", "Q-4", "Q-5", "Q-6", "Q-7"]);
    const runner = session();
    const [input] = await runner.ingestMany([file("seven.pdf", 7, "Q")]);
    expect(input?.inspectionError).toBeUndefined();
    expect((await runner.inspectInsights(input?.id)).pageCount).toBe(7);
  });

  it("reads the page order back from merge and reorder output", async () => {
    const runner = session();
    const ids = (await runner.ingestMany([file("a.pdf", 2, "A"), file("b.pdf", 3, "B")])).map(
      (entry) => entry.id,
    );
    const merged = await output(
      await runner.runTool({ type: "runTool", toolId: "pdf.merge", params: {}, inputIds: ids }),
    );
    expect(declaredPageCount(merged)).toBe(5);
    expect(pageMarkers(merged, "[AB]")).toEqual(["A-1", "A-2", "B-1", "B-2", "B-3"]);

    const [single] = await runner.ingestMany([file("c.pdf", 4, "C")]);
    const reordered = await output(
      await runner.runTool({
        type: "runTool",
        toolId: "pdf.reorder",
        params: { order: "3,1,4,2" },
        inputIds: [single?.id as string],
      }),
    );
    expect(pageMarkers(reordered, "C")).toEqual(["C-3", "C-1", "C-4", "C-2"]);
  });

  it("reads page rotations back from rotate output", async () => {
    const runner = session();
    const [input] = await runner.ingestMany([file("d.pdf", 3, "D")]);
    const rotated = await output(
      await runner.runTool({
        type: "runTool",
        toolId: "pdf.rotate",
        params: { angle: 90, pages: "2" },
        inputIds: [input?.id as string],
      }),
    );
    expect(pageRotations(rotated)).toEqual([0, 90, 0]);
    expect(pageMarkers(rotated, "D")).toEqual(["D-1", "D-2", "D-3"]);
  });
});
