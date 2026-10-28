// Every package the account routes import must be a declared runtime
// dependency of the web package, and locked for it. pnpm gives each workspace
// package only its own dependencies, so a package that another workspace
// member (such as services/auth) declares does not resolve here, and a clean
// `pnpm install --frozen-lockfile` fails when the lockfile lags the manifest.
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";

const webRoot = path.join(import.meta.dirname, "..");
const lockfile = path.join(webRoot, "../../pnpm-lock.yaml");
const accountSources = [
  path.join(webRoot, "src/components/account"),
  path.join(webRoot, "src/app/(account)"),
];

function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((item) => {
    const full = path.join(directory, item.name);
    if (item.isDirectory()) {
      return sourceFiles(full);
    }
    return /\.(ts|tsx)$/.test(item.name) ? [full] : [];
  });
}

/** The package names behind the bare import specifiers of a source file. */
function importedPackages(source: string): string[] {
  const specifiers = [...source.matchAll(/\bfrom\s+"([^"]+)"|\bimport\s+"([^"]+)"/g)].map(
    (match) => match[1] ?? match[2] ?? "",
  );
  return specifiers
    .filter((specifier) => !/^(\.|@\/|node:)/.test(specifier))
    .map((specifier) => {
      const parts = specifier.split("/");
      return specifier.startsWith("@") ? parts.slice(0, 2).join("/") : (parts[0] ?? specifier);
    });
}

/** The dependency names pnpm-lock.yaml records for the apps/web importer. */
function lockedWebDependencies(text: string): Set<string> {
  const lines = text.split("\n");
  const start = lines.indexOf("  apps/web:");
  const names = new Set<string>();
  if (start === -1) {
    return names;
  }
  let section = "";
  for (const line of lines.slice(start + 1)) {
    if (/^ {0,2}\S/.test(line)) {
      break;
    }
    const heading = /^ {4}(\w+):\s*$/.exec(line);
    if (heading) {
      section = heading[1] ?? "";
      continue;
    }
    const name = /^ {6}'?([^'\s]+?)'?:\s*$/.exec(line);
    if (name?.[1] && section === "dependencies") {
      names.add(name[1]);
    }
  }
  return names;
}

const manifest = JSON.parse(readFileSync(path.join(webRoot, "package.json"), "utf8")) as {
  dependencies?: Record<string, string>;
};
const imported = [
  ...new Set(
    accountSources
      .flatMap(sourceFiles)
      .flatMap((file) => importedPackages(readFileSync(file, "utf8"))),
  ),
].sort();

describe("account route dependencies", () => {
  it("finds the account clients' packages", () => {
    expect(imported).toEqual(
      expect.arrayContaining([
        "@ariadshift/sdk",
        "@better-auth/api-key",
        "@better-auth/passkey",
        "better-auth",
      ]),
    );
  });

  it("declares every imported package as a runtime dependency", () => {
    const declared = Object.keys(manifest.dependencies ?? {});
    expect(imported.filter((name) => !declared.includes(name))).toEqual([]);
  });

  it("uses the shared catalog pins for Better Auth", () => {
    for (const name of ["better-auth", "@better-auth/api-key", "@better-auth/passkey"]) {
      expect(manifest.dependencies?.[name]).toBe("catalog:");
    }
  });

  it("locks every imported package for the web importer", () => {
    const locked = lockedWebDependencies(readFileSync(lockfile, "utf8"));
    expect(imported.filter((name) => !locked.has(name))).toEqual([]);
  });
});
