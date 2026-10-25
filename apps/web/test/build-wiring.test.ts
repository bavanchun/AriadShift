import { readFileSync } from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "../../..");

function read(file: string): string {
  return readFileSync(path.join(root, file), "utf8");
}

interface Manifest {
  scripts?: Record<string, string>;
  dependencies?: Record<string, string>;
  devDependencies?: Record<string, string>;
}

function manifest(dir: string): Manifest {
  return JSON.parse(read(`${dir}/package.json`)) as Manifest;
}

/** The specifiers pnpm-lock.yaml records for one workspace importer. */
function lockedSpecifiers(lock: string, dir: string): Record<string, string> {
  const lines = lock.split("\n");
  const start = lines.indexOf(`  ${dir}:`);
  expect(start, `${dir} has no importer in pnpm-lock.yaml`).toBeGreaterThan(-1);
  const specifiers: Record<string, string> = {};
  let name: string | null = null;
  for (const line of lines.slice(start + 1)) {
    if (line.trim() && !line.startsWith("    ")) {
      break;
    }
    const dependency = /^ {6}'?(.+?)'?:$/.exec(line);
    const specifier = /^ {8}specifier: '?(.+?)'?$/.exec(line);
    if (dependency?.[1]) {
      name = dependency[1];
    } else if (specifier?.[1] && name) {
      specifiers[name] = specifier[1];
    }
  }
  return specifiers;
}

describe("frozen installation", () => {
  // A manifest change without its lockfile fails `pnpm install --frozen-lockfile`,
  // which the web image and CI run before anything builds.
  it.each(["apps/web", "packages/sdk"])("%s matches its pnpm-lock.yaml importer", (dir) => {
    const { dependencies = {}, devDependencies = {} } = manifest(dir);
    expect(lockedSpecifiers(read("pnpm-lock.yaml"), dir)).toEqual({
      ...dependencies,
      ...devDependencies,
    });
  });
});

describe("SDK client generation", () => {
  // src/generated/ is not committed, so every entry point that compiles or tests the
  // web app generates it first instead of relying on an earlier typecheck.
  it.each(["dev", "build", "export", "typecheck", "test"])(
    "the web %s script generates the SDK client first",
    (script) => {
      const scripts = manifest("apps/web").scripts ?? {};
      expect(scripts[script]).toMatch(/^pnpm run generate-sdk && /);
      expect(scripts["generate-sdk"]).toBe("pnpm --filter @ariadshift/sdk generate");
    },
  );

  it("generates the client before the workspace tests", () => {
    expect(read("justfile")).toMatch(/^web-test: .*\bsdk-generate\b/m);
  });
});
