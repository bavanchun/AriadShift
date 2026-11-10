import type { ToolDescriptor as WasmToolDescriptor } from "@ariadshift/ariad-wasm";
import registry from "../../../../schemas/tools.v1.json";

/** The registry wire format uses `arity`; older generated bindings called it `kind`. */
export interface ToolDescriptor extends Omit<WasmToolDescriptor, "inputs"> {
  inputs: { arity: "one" } | { arity: "many"; min: number; max: number };
}

/** Includes desktop/cloud tools even when the browser runtime cannot execute them. */
export function toolCatalog(runtime: WasmToolDescriptor[] = []): ToolDescriptor[] {
  const tools = new Map((registry.tools as ToolDescriptor[]).map((tool) => [tool.id, tool]));
  for (const tool of runtime) {
    const input = tool.inputs as unknown as {
      arity?: "one" | "many";
      kind?: "one" | "many";
      min: number;
      max: number;
    };
    const arity = input.arity ?? input.kind;
    tools.set(tool.id, {
      ...tools.get(tool.id),
      ...tool,
      inputs: arity === "many" ? { arity, min: input.min, max: input.max } : { arity: "one" },
    });
  }
  return structuredClone([...tools.values()]);
}
