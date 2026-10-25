import type { ConvertResult } from "@ariadshift/ariad-wasm";
import * as wasm from "@ariadshift/ariad-wasm";
import { pandocUrl } from "../../engine-assets.mjs";
import type { Core, Engine } from "./engines";
import { ENGINE_CACHE, engineResponse, usedEngines } from "./wasm-cache";

const typstUrl = new URL(
  "../../../../packages/ariad-typst/pkg/ariad_typst_bg.wasm",
  import.meta.url,
).href;

/**
 * Loads the JavaScript of every engine whose binary is already cached. After a deployment
 * renames the lazy engine chunks, this fetches the new ones while online, and the page then
 * keeps them in the engine cache, so the route still works offline without converting first.
 */
export async function warmCachedEngines(): Promise<void> {
  if (typeof caches === "undefined" || !navigator.onLine) return;
  try {
    const cache = await caches.open(ENGINE_CACHE);
    if (await cache.match(pandocUrl)) {
      await import("./pandoc");
      usedEngines.add(pandocUrl);
    }
    if (await cache.match(typstUrl)) {
      await import("@ariadshift/ariad-typst");
      usedEngines.add(typstUrl);
    }
  } catch {
    /* Storage denied or a chunk failed to load; the first conversion loads it instead. */
  }
}

/** Register route engines without loading their modules until a conversion needs them. */
export function browserEngines(core: Core): Core {
  let pandoc: import("./pandoc").Pandoc | undefined;
  let typst: typeof import("@ariadshift/ariad-typst") | undefined;
  let typstMemory: WebAssembly.Memory | undefined;
  const engines = new Map(core.engines);
  engines.set("pdf", engines.get("ariad-core") as Engine);
  engines.set("pandoc", {
    async convert(bytes, from, to, options) {
      pandoc ??= new (await import("./pandoc")).Pandoc();
      const warnings = [];
      if (from !== "ariad-ir+json") {
        const output = await pandoc.convert(bytes, from, "json");
        const result = wasm.readPandoc(output.bytes, output.media, from);
        result.warnings.push(...output.warnings);
        if (to === "ariad-ir+json") return result;
        bytes = result.bytes;
        warnings.push(...result.warnings);
      }
      const mapped = wasm.writePandoc(bytes, options.title);
      const output = await pandoc.convert(mapped.bytes, "json", to === "epub" ? "epub3" : to);
      const stamped = wasm.stampPackage(
        output.bytes,
        to === "epub",
        new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
      );
      return {
        bytes: stamped,
        format: to,
        media_type:
          to === "epub"
            ? "application/epub+zip"
            : "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        extension: to,
        route: [],
        warnings: [...warnings, ...mapped.warnings, ...output.warnings],
      };
    },
  });
  engines.set("typst", {
    async convert(bytes, _from, _to, options) {
      if (!typst) {
        const module = await import("@ariadshift/ariad-typst");
        const initialized = await module.default({
          module_or_path: await engineResponse(typstUrl),
        });
        typstMemory = initialized.memory;
        typst = module;
      }
      return typst.renderPdf(bytes, options.title, Math.floor(Date.now() / 1000)) as ConvertResult;
    },
  });
  return {
    ...core,
    engines,
    memoryBytes: () =>
      core.memoryBytes() + (pandoc?.memoryBytes ?? 0) + (typstMemory?.buffer.byteLength ?? 0),
  };
}
