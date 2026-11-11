import { MemoryStore } from "@/converter/file-store";
import type { ConvertedFile } from "@/converter/protocol";
import { ConverterSession } from "@/converter/session";
import { loadCore } from "./wasm";

describe("ConverterSession convert sources", () => {
  const md = (text: string, name: string) => new File([text], name, { type: "text/markdown" });
  const text = async (file: ConvertedFile) =>
    file.location.kind === "memory" ? file.location.blob.text() : "";

  it("converts a chosen input slot instead of the first", async () => {
    const session = new ConverterSession(loadCore(), new MemoryStore());
    const [first, second] = await session.ingestMany([
      md("# First\n", "first.md"),
      md("# Second\n", "second.md"),
    ]);
    const output = await session.convert("html", "editable", {
      inputId: (second as { id: string }).id,
    });
    expect(output.name).toBe("second.html");
    expect(await text(output)).toContain("Second");
    expect(first).toBeTruthy();
  });

  it("converts a loose IR file without touching the input set", async () => {
    const session = new ConverterSession(loadCore(), new MemoryStore());
    await session.ingestMany([md("# Kept\n", "kept.md")]);
    const asIr = await session.convert("ariad-ir+json", "editable");
    const json = await text(asIr);
    const exported = await session.convert("markdown", "editable", {
      file: new File([json], "edited.ir.json", { type: "application/json" }),
    });
    expect(exported.name).toBe("edited.md");
    expect(await text(exported)).toContain("Kept");
    // The input set is unchanged: the default source is still kept.md.
    expect((await session.convert("html", "editable")).name).toBe("kept.html");
  });

  it("rejects an unknown input slot", async () => {
    const session = new ConverterSession(loadCore(), new MemoryStore());
    await session.ingestMany([md("# A\n", "a.md")]);
    await expect(session.convert("html", "editable", { inputId: "missing" })).rejects.toMatchObject(
      {
        code: "no_input",
      },
    );
  });
});
