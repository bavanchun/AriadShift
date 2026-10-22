import { readFileSync } from "node:fs";
import * as ariadWasm from "@ariadshift/ariad-wasm";
import { type Core, coreFrom } from "@/converter/engines";

let core: Core | null = null;

/** The real ariad-wasm module, initialized once for Node tests. */
export function loadCore(): Core {
  if (!core) {
    ariadWasm.initSync({
      module: readFileSync(
        new URL("../../../packages/ariad-wasm/pkg/ariad_wasm_bg.wasm", import.meta.url),
      ),
    });
    core = coreFrom(ariadWasm);
  }
  return core;
}
