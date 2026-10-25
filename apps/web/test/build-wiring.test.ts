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

describe("workspace packages", () => {
  // Next.js compiles a workspace package only when it is listed in
  // transpilePackages; one that exports TypeScript source fails the build otherwise.
  it("transpiles every workspace dependency that exports TypeScript source", async () => {
    const { default: nextConfig } = await import("../next.config");
    const { dependencies = {} } = manifest("apps/web");
    const typeScriptSources = Object.entries(dependencies)
      .filter(([, specifier]) => specifier.startsWith("workspace:"))
      .map(([name]) => name)
      .filter((name) => {
        const dir = `packages/${name.replace("@ariadshift/", "")}`;
        const { exports } = JSON.parse(read(`${dir}/package.json`)) as { exports?: unknown };
        return /(?<!\.d)\.tsx?"/.test(JSON.stringify(exports ?? null));
      });
    expect(typeScriptSources).toEqual(
      expect.arrayContaining(["@ariadshift/sdk", "@ariadshift/ui"]),
    );
    expect(nextConfig.transpilePackages).toEqual(expect.arrayContaining(typeScriptSources));
  });
});

describe("web image build arguments", () => {
  // The static export inlines NEXT_PUBLIC_* at build time, so each variable the
  // cloud configuration reads must reach the image's export step.
  const variables = [
    ...read("apps/web/src/lib/cloud/config.ts").matchAll(/process\.env\.(NEXT_PUBLIC_\w+)/g),
  ].map((match) => match[1] as string);

  it("reads the four cloud variables", () => {
    expect([...variables].sort()).toEqual([
      "NEXT_PUBLIC_ARIAD_API_URL",
      "NEXT_PUBLIC_ARIAD_AUTH_URL",
      "NEXT_PUBLIC_ARIAD_OPERATOR",
      "NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY",
    ]);
  });

  it.each(variables)("the web image forwards %s to the export", (variable) => {
    const dockerfile = read("infra/docker/web.Dockerfile");
    expect(dockerfile).toMatch(new RegExp(`^ARG ${variable}=`, "m"));
    expect(dockerfile).toMatch(new RegExp(`\\b${variable}=\\$\\{${variable}\\}`));
  });

  it.each(variables)("the compose web build passes %s", (variable) => {
    const compose = read("infra/compose/docker-compose.yml");
    const web = compose.slice(compose.indexOf("\n  web:\n"));
    const args = web.slice(web.indexOf("      args:\n"), web.indexOf("\n    image:"));
    expect(args).toMatch(new RegExp(`^ {8}${variable}: \\$\\{\\w+:-[^}]*\\}$`, "m"));
  });
});

describe("web image browser engines", () => {
  it.each(["ariad-wasm", "ariad-typst"])("emits %s bindings before exporting the site", (crate) => {
    const dockerfile = read("infra/docker/web.Dockerfile");
    const beforeExport = dockerfile.slice(
      0,
      dockerfile.indexOf("RUN pnpm --filter @ariadshift/web export"),
    );
    expect(beforeExport).toContain(`-p ${crate} `);
    expect(beforeExport).toContain(`--out-dir packages/${crate}/pkg`);
    expect(beforeExport).toContain(
      `target/wasm32-unknown-unknown/wasm-release/${crate.replaceAll("-", "_")}.wasm`,
    );
  });
});
