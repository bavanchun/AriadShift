// The conversion engines the worker can run, keyed by the engine ids that appear in
// planned routes. ariad-core runs through ariad-wasm; browserEngines adds the lazy
// Pandoc and Typst adapters without changing the worker protocol.
import type {
  ConvertOptions,
  ConvertResult,
  FormatId,
  InspectResult,
  Notice,
  Plan,
  PlanStep,
} from "@ariadshift/ariad-wasm";

export interface Engine {
  convert(
    bytes: Uint8Array,
    from: FormatId,
    to: FormatId,
    options: ConvertOptions,
  ): ConvertResult | Promise<ConvertResult>;
}

/** The functions of ariad-wasm the worker uses, plus the engine table. */
export interface Core {
  version(): string;
  /** Current size of the wasm module's memory, which never shrinks. */
  memoryBytes(): number;
  inspect(bytes: Uint8Array, name: string, totalBytes?: number | null): InspectResult;
  plan(input: FormatId, to: FormatId, profile?: string | null, scope?: string | null): Plan;
  engines: ReadonlyMap<string, Engine>;
}

export interface CoreModule {
  version(): string;
  inspect(bytes: Uint8Array, name: string, totalBytes?: number | null): InspectResult;
  plan(input: FormatId, to: FormatId, profile?: string | null, scope?: string | null): Plan;
  convert(
    bytes: Uint8Array,
    from: FormatId,
    to: FormatId,
    options: ConvertOptions | undefined,
  ): ConvertResult;
}

/** Wraps an initialized ariad-wasm module and its memory. */
export function coreFrom(module: CoreModule, memory: WebAssembly.Memory): Core {
  return {
    version: () => module.version(),
    memoryBytes: () => memory.buffer.byteLength,
    inspect: (bytes, name, totalBytes) => module.inspect(bytes, name, totalBytes),
    plan: (input, to, profile, scope) => module.plan(input, to, profile, scope),
    engines: new Map<string, Engine>([
      [
        "ariad-core",
        { convert: (bytes, from, to, options) => module.convert(bytes, from, to, options) },
      ],
    ]),
  };
}

/** Splits a route into runs of consecutive steps that use the same engine. */
export function groupSteps(
  steps: PlanStep[],
): Array<{ engine: string; from: FormatId; to: FormatId }> {
  const groups: Array<{ engine: string; from: FormatId; to: FormatId }> = [];
  for (const step of steps) {
    const last = groups.at(-1);
    if (last && last.engine === step.engine) {
      last.to = step.to;
    } else {
      groups.push({ engine: step.engine, from: step.from, to: step.to });
    }
  }
  return groups;
}

export interface RouteOutput {
  bytes: Uint8Array;
  mediaType: string;
  extension: string;
  warnings: Notice[];
}

/**
 * Largest intermediate `ariad-ir+json` payload a multi-engine browser route may hand to
 * the next engine (256 MiB). Matches the worker's wasm memory recycle threshold
 * (`RECYCLE_MEMORY_BYTES` in `session.ts`): an accepted <= 16 MiB source document with
 * embedded media legitimately expands past 16 MiB once base64-encoded into IR JSON,
 * while an intermediate payload above 256 MiB would exceed a single conversion's worker
 * memory budget before the consuming engine deserializes it.
 */
export const INTERMEDIATE_IR_LIMIT_BYTES = 256 * 1024 * 1024;

function intermediateOptions(options: ConvertOptions): ConvertOptions {
  return {
    ...options,
    limits: {
      ...options.limits,
      max_input_bytes: INTERMEDIATE_IR_LIMIT_BYTES,
    },
  };
}

/** Runs a planned route, handing each run of steps to its engine. */
export async function runRoute(
  core: Core,
  bytes: Uint8Array,
  steps: PlanStep[],
  options: ConvertOptions,
): Promise<RouteOutput> {
  let current = bytes;
  let last: ConvertResult | null = null;
  const warnings: Notice[] = [];
  // Ingest already capped the user's original bytes; intermediate IR uses its own browser budget.
  const laterOptions = intermediateOptions(options);
  for (const [index, group] of groupSteps(steps).entries()) {
    const engine = core.engines.get(group.engine);
    if (!engine) {
      throw {
        code: "engine_unavailable",
        message: `The ${group.engine} engine is not available in this browser.`,
        engine: group.engine,
      };
    }
    last = await engine.convert(
      current,
      group.from,
      group.to,
      index === 0 ? options : laterOptions,
    );
    current = last.bytes;
    warnings.push(...last.warnings);
  }
  if (!last) {
    throw { code: "invalid_input", message: "The route has no steps." };
  }
  return { bytes: current, mediaType: last.media_type, extension: last.extension, warnings };
}
