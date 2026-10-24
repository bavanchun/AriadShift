import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import {
  type AriadError,
  capabilities,
  convert,
  initSync,
  inspect,
  plan,
  version,
} from "../pkg/ariad_wasm.js";

const encoder = new TextEncoder();
const decoder = new TextDecoder();

function thrown(run: () => unknown): AriadError {
  try {
    run();
  } catch (error) {
    return error as AriadError;
  }
  throw new Error("expected the call to throw");
}

beforeAll(() => {
  initSync({ module: readFileSync(new URL("../pkg/ariad_wasm_bg.wasm", import.meta.url)) });
});

describe("ariad-wasm", () => {
  it("reports the workspace version and the embedded capabilities", () => {
    expect(version()).toMatch(/^\d+\.\d+\.\d+/);
    const caps = capabilities();
    expect(caps.version).toBe("ariad-capabilities/0");
    expect(caps.edges.some((edge) => edge.runtime.includes("wasm"))).toBe(true);
  });

  it("inspects Markdown with counts and browser reachability", () => {
    const result = inspect(encoder.encode("# Title\n\nSome words here.\n"), "notes.md");
    expect(result.format).toBe("markdown");
    expect(result.bytes).toBe(26);
    expect(result.counts?.headings["1"]).toBe(1);
    expect(result.counts?.words).toBe(4);
    expect(result.meta?.title).toBeNull();
    expect(result.reachable).toEqual(["ariad-ir+json", "html"]);
    expect(result.reachable_elsewhere).toEqual(["docx", "epub", "pdf"]);
  });

  it("uses the total size for a prefix of a large file", () => {
    const result = inspect(encoder.encode("# Big"), "big.md", 64 * 1024 * 1024);
    expect(result.bytes).toBe(64 * 1024 * 1024);
    expect(result.counts).toBeNull();
    expect(result.warnings.map((warning) => warning.code)).toEqual(["document_too_large"]);
  });

  it("plans browser routes and explains the alternatives", () => {
    const route = plan("markdown", "html");
    expect(route.steps.map((step) => `${step.from}>${step.to}:${step.engine}`)).toEqual([
      "markdown>ariad-ir+json:ariad-core",
      "ariad-ir+json>html:ariad-core",
    ]);
    expect(route.measured).toBe(true);
    expect(route.score.fidelity).toBeGreaterThan(0.9);
  });

  it("plans CLI routes when asked for every runtime", () => {
    const route = plan("docx", "markdown", "editable", "all");
    expect(route.steps[0]?.engine).toBe("pandoc");
    expect(thrown(() => plan("docx", "markdown")).code).toBe("no_route");
  });

  it("converts Markdown to HTML and back", () => {
    const html = convert(encoder.encode("# Hello\n\n*world*\n"), "markdown", "html", {
      title: "greeting",
    });
    expect(html.media_type).toBe("text/html");
    expect(html.extension).toBe("html");
    expect(html.bytes).toBeInstanceOf(Uint8Array);
    const page = decoder.decode(html.bytes);
    expect(page).toContain("<title>greeting</title>");
    expect(page).toContain("<em>world</em>");

    const markdown = convert(html.bytes, "html", "markdown", undefined);
    expect(decoder.decode(markdown.bytes)).toContain("# Hello");
  });

  it("round-trips Markdown through IR JSON", () => {
    const source = encoder.encode("# Title\n\n- one\n- two\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
    const ir = convert(source, "markdown", "ariad-ir+json", undefined);
    expect(ir.extension).toBe("ir.json");
    const document = JSON.parse(decoder.decode(ir.bytes));
    expect(document.version).toBe("ariad-ir/1");
    const markdown = convert(ir.bytes, "ariad-ir+json", "markdown", undefined);
    const again = convert(markdown.bytes, "markdown", "ariad-ir+json", undefined);
    expect(JSON.parse(decoder.decode(again.bytes))).toEqual(document);
  });

  it("throws typed error objects", () => {
    const noRoute = thrown(() => convert(encoder.encode("x"), "markdown", "docx", undefined));
    expect(noRoute.code).toBe("no_route");
    expect(noRoute.reachable_elsewhere).toEqual(["docx", "epub", "pdf"]);
    expect(noRoute.message).toContain("CLI");

    expect(thrown(() => plan("md", "html")).code).toBe("unknown_format");
    expect(thrown(() => plan("html", "html")).code).toBe("same_format");
    const badProfile = thrown(() => plan("markdown", "html", "best"));
    expect(badProfile.code).toBe("invalid_options");
    expect(thrown(() => convert(new Uint8Array([0xff]), "markdown", "html", undefined)).code).toBe(
      "invalid_input",
    );
    const badIr = thrown(() =>
      convert(encoder.encode('{"version":"ariad-ir/7"}'), "ariad-ir+json", "html", undefined),
    );
    expect(badIr.code).toBe("unsupported_ir_version");
    expect(thrown(() => inspect(new Uint8Array([0, 1, 2]), "blob.bin")).code).toBe(
      "unknown_format",
    );
  });

  it("applies limit overrides and rejects unknown options", () => {
    const limited = thrown(() =>
      convert(encoder.encode("a\n\nb\n\nc\n"), "markdown", "html", { limits: { max_blocks: 2 } }),
    );
    expect(limited.code).toBe("limit_exceeded");
    expect(limited.limit).toBe("max_blocks");

    const unknown = thrown(() =>
      convert(encoder.encode("a"), "markdown", "html", { speed: "fast" } as never),
    );
    expect(unknown.code).toBe("invalid_options");

    const typo = thrown(() =>
      convert(encoder.encode("a"), "markdown", "html", { limits: { max_block: 1 } } as never),
    );
    expect(typo.code).toBe("invalid_options");
    expect(typo.option).toBe("limits");
  });

  it("converts IR nested to the local limit without exhausting the stack", () => {
    let block: unknown = { type: "paragraph", content: [{ type: "text", text: "deep" }] };
    for (let level = 0; level < 63; level += 1) {
      block = { type: "quote", blocks: [block] };
    }
    const document = { version: "ariad-ir/0", body: [block] };
    const html = convert(encoder.encode(JSON.stringify(document)), "ariad-ir+json", "html", {
      title: "deep",
    });
    expect(decoder.decode(html.bytes)).toContain("deep</p>");
    const markdown = convert(
      encoder.encode(JSON.stringify(document)),
      "ariad-ir+json",
      "markdown",
      undefined,
    );
    expect(decoder.decode(markdown.bytes)).toContain("> deep");
  });
});
