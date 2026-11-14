// Collect notices from the installed desktop production graph, including shared UI assets.
import { existsSync, readdirSync, readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { isAllowedLicense } from "./check-js-licenses.mjs";

const defaultRootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export function resolveRootDir(args = process.argv.slice(2)) {
  const rootIndex = args.indexOf("--root-dir");
  if (rootIndex !== -1 && args[rootIndex + 1]) {
    return path.resolve(args[rootIndex + 1]);
  }
  return process.env.ARIADSHIFT_ROOT_DIR
    ? path.resolve(process.env.ARIADSHIFT_ROOT_DIR)
    : defaultRootDir;
}

export function assertDesktopPreconditions(root = defaultRootDir) {
  if (!existsSync(path.join(root, "apps/desktop/package.json"))) {
    throw new Error(`Missing apps/desktop/package.json under ${root}.`);
  }
  const hasRootNodeModules = existsSync(path.join(root, "node_modules"));
  const hasDesktopNodeModules = existsSync(path.join(root, "apps/desktop/node_modules"));
  if (!hasRootNodeModules && !hasDesktopNodeModules) {
    throw new Error(
      "JavaScript dependencies are not installed (missing node_modules); run 'pnpm install' first.",
    );
  }
}

function dependency(directory, name) {
  for (let current = directory; ; current = path.dirname(current)) {
    const candidate = path.join(current, "node_modules", name);
    if (existsSync(path.join(candidate, "package.json"))) return realpathSync(candidate);
    if (path.dirname(current) === current) {
      throw new Error(`Missing dependency ${name}; run 'pnpm install' first.`);
    }
  }
}

export function generateDesktopLicenses(root = defaultRootDir) {
  assertDesktopPreconditions(root);
  const seen = new Set();
  const notices = new Map();

  function visit(directory) {
    if (seen.has(directory)) return;
    seen.add(directory);
    const metadata = JSON.parse(readFileSync(path.join(directory, "package.json"), "utf8"));
    if (!metadata.private) {
      if (!isAllowedLicense(metadata.name, metadata.license ?? "Unknown"))
        throw new Error(`Unapproved license: ${metadata.name}: ${metadata.license}`);
      const files = readdirSync(directory)
        .filter((file) => /^(licen[cs]e|copying|notice)([._-]|$)/i.test(file))
        .sort();
      if (!files.length) throw new Error(`Missing license text: ${metadata.name}`);
      notices.set(
        `${metadata.name}@${metadata.version}`,
        [
          `License: ${metadata.license}`,
          ...files.map((file) => readFileSync(path.join(directory, file), "utf8")),
        ].join("\n\n"),
      );
    }
    for (const name of Object.keys({
      ...metadata.dependencies,
      ...metadata.optionalDependencies,
    }).sort())
      visit(dependency(directory, name));
  }

  visit(path.join(root, "apps/desktop"));
  const output = [
    "Generated desktop JavaScript dependency notices. Run just desktop-licenses.",
    ...[...notices]
      .sort(([left], [right]) => left.localeCompare(right, "en"))
      .map(([name, notice]) => `${name}\n${"=".repeat(name.length)}\n\n${notice.trim()}`),
  ].join("\n\n");
  const normalized = `${output.replace(/\r\n?/g, "\n").replace(/[\t ]+$/gm, "")}\n`;
  if (!normalized.trim()) {
    throw new Error("Generated desktop JavaScript dependency notices are empty.");
  }
  return normalized;
}

function isMainModule() {
  if (!process.argv[1]) {
    return false;
  }
  try {
    return realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
}

if (isMainModule()) {
  const args = process.argv.slice(2);
  const root = resolveRootDir(args);
  try {
    if (args.includes("--check-preconditions")) {
      assertDesktopPreconditions(root);
    } else {
      process.stdout.write(generateDesktopLicenses(root));
    }
  } catch (error) {
    console.error(`ERROR: ${error instanceof Error ? error.message : String(error)}`);
    process.exit(1);
  }
}
