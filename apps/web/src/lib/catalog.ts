// Build-time access to the real planner and capabilities through the same wasm module
// the converter runs, so pages never restate routing rules in TypeScript.
import { readFileSync } from "node:fs";
import path from "node:path";
import {
  type Capabilities,
  type CapabilityEdge,
  capabilities,
  initSync,
  type Plan,
  plan,
} from "@ariadshift/ariad-wasm";
import { FORMATS, type FormatInfo } from "./formats";

let initialized = false;

function ready() {
  if (!initialized) {
    const module = path.join(process.cwd(), "../../packages/ariad-wasm/pkg/ariad_wasm_bg.wasm");
    initSync({ module: readFileSync(module) });
    initialized = true;
  }
}

export function loadCapabilities(): Capabilities {
  ready();
  return capabilities();
}

export interface PairRoute {
  from: FormatInfo;
  to: FormatInfo;
  /** Route that runs in the browser, if any. */
  browser: Plan | null;
  /** Best route across every runtime. */
  best: Plan;
}

let pairsCache: PairRoute[] | null = null;

/** Every ordered pair of displayed formats with a route, and where it runs. */
export function loadPairs(): PairRoute[] {
  if (pairsCache) {
    return pairsCache;
  }
  ready();
  const pairs: PairRoute[] = [];
  for (const from of FORMATS) {
    for (const to of FORMATS) {
      if (from === to) {
        continue;
      }
      let best: Plan;
      try {
        best = plan(from.id, to.id, "editable", "all");
      } catch {
        continue;
      }
      let browser: Plan | null = null;
      try {
        browser = plan(from.id, to.id, "editable", "browser");
      } catch {
        browser = null;
      }
      pairs.push({ from, to, browser, best });
    }
  }
  pairsCache = pairs;
  return pairs;
}

export function edgesFor(caps: Capabilities, route: Plan): CapabilityEdge[] {
  return route.steps.flatMap((step) =>
    caps.edges.filter(
      (edge) => edge.from === step.from && edge.to === step.to && edge.engine === step.engine,
    ),
  );
}
