// Copies NOTICE and THIRD_PARTY_LICENSES into public/notices/ and generates the
// web JavaScript dependency notices from the installed production graph.
import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  realpathSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { isAllowedLicense } from "../../../scripts/check-js-licenses.mjs";

const defaultWebDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const defaultRootDir = path.resolve(defaultWebDir, "../..");
const defaultOutDir = path.join(defaultWebDir, "public/notices");
const defaultPagePath = path.join(defaultWebDir, "src/app/licenses/page.tsx");

// Monorepo subpackages whose published tarballs omit the root LICENSE file
// that accompanies the main package in the same dependency graph.
const COMPANION_LICENSES = new Map([
  ["@better-auth/utils", ["better-auth", "LICENSE.md"]],
  ["@next/env", ["next", "license.md"]],
  ["client-only", ["react", "LICENSE"]],
]);

function normalize(text) {
  return `${text
    .replace(/\r\n?/g, "\n")
    .replace(/[\t ]+$/gm, "")
    .trimEnd()}\n`;
}

function resolveDependency(directory, name) {
  for (let current = directory; ; current = path.dirname(current)) {
    const candidate = path.join(current, "node_modules", name);
    if (existsSync(path.join(candidate, "package.json"))) {
      return realpathSync(candidate);
    }
    if (path.dirname(current) === current) {
      throw new Error(`Missing dependency ${name}`);
    }
  }
}

function extractReadmeLicense(directory) {
  const readmePath = path.join(directory, "README.md");
  if (!existsSync(readmePath)) {
    return null;
  }
  const readme = readFileSync(readmePath, "utf8");
  const match = /^##+\s+Licenses?(?:\(s\))?\s*\r?\n+([\s\S]+)$/im.exec(readme);
  if (match?.[1] && /Permission (?:is hereby granted|to use, copy, modify)/i.test(match[1])) {
    return match[1].trim();
  }
  return null;
}

function readLicenseTexts(directory, metadata) {
  const files = readdirSync(directory)
    .filter((file) => /^(licen[cs]e|copying|notice)([._-]|$)/i.test(file))
    .sort();
  if (files.length > 0) {
    return files.map((file) => readFileSync(path.join(directory, file), "utf8"));
  }
  const readmeLicense = extractReadmeLicense(directory);
  if (readmeLicense) {
    return [readmeLicense];
  }
  const companion = COMPANION_LICENSES.get(metadata.name);
  if (companion) {
    const [companionPkg, companionFile] = companion;
    const companionDir = resolveDependency(directory, companionPkg);
    const companionMeta = JSON.parse(readFileSync(path.join(companionDir, "package.json"), "utf8"));
    if (companionMeta.license === metadata.license) {
      const target = path.join(companionDir, companionFile);
      if (existsSync(target)) {
        return [readFileSync(target, "utf8")];
      }
    }
  }
  throw new Error(`Missing license text: ${metadata.name}`);
}

export function generateWebJsNotices(rootDir = defaultRootDir) {
  const seen = new Set();
  const notices = new Map();

  function visit(directory) {
    if (seen.has(directory)) {
      return;
    }
    seen.add(directory);
    const metadata = JSON.parse(readFileSync(path.join(directory, "package.json"), "utf8"));
    if (!metadata.private) {
      if (!isAllowedLicense(metadata.name, metadata.license ?? "Unknown")) {
        throw new Error(`Unapproved license: ${metadata.name}: ${metadata.license}`);
      }
      const texts = readLicenseTexts(directory, metadata);
      notices.set(
        `${metadata.name}@${metadata.version}`,
        [`License: ${metadata.license}`, ...texts].join("\n\n"),
      );
    }
    for (const name of Object.keys(metadata.dependencies ?? {}).sort()) {
      visit(resolveDependency(directory, name));
    }
  }

  visit(path.join(rootDir, "apps/web"));
  const output = [
    "Generated web JavaScript dependency notices. Run just web-licenses.",
    ...[...notices]
      .sort(([left], [right]) => left.localeCompare(right, "en"))
      .map(([name, notice]) => `${name}\n${"=".repeat(name.length)}\n\n${notice.trim()}`),
  ].join("\n\n");
  return normalize(output);
}

export function expectedNotices(rootDir = defaultRootDir) {
  return {
    "NOTICE.txt": normalize(readFileSync(path.join(rootDir, "NOTICE"), "utf8")),
    "THIRD_PARTY_LICENSES.txt": normalize(
      readFileSync(path.join(rootDir, "THIRD_PARTY_LICENSES"), "utf8"),
    ),
    "THIRD_PARTY_JS_LICENSES.txt": generateWebJsNotices(rootDir),
  };
}

function verifyLicensesPage(rootDir, pagePath) {
  const page = readFileSync(pagePath, "utf8");
  for (const requiredHref of [
    "/wasm/pandoc/SOURCE.txt",
    "/notices/NOTICE.txt",
    "/notices/THIRD_PARTY_LICENSES.txt",
    "/notices/THIRD_PARTY_JS_LICENSES.txt",
  ]) {
    if (!page.includes(`href="${requiredHref}"`)) {
      throw new Error(`src/app/licenses/page.tsx is missing link ${requiredHref}`);
    }
  }
  if (page.includes("blob/dev/NOTICE")) {
    throw new Error("src/app/licenses/page.tsx still links to the moving dev branch NOTICE");
  }

  const cargoLock = readFileSync(path.join(rootDir, "Cargo.lock"), "utf8");
  const ammoniaBlock = /name = "ammonia"[\s\S]*?dependencies = \[([\s\S]*?)\]/m.exec(cargoLock);
  const cssparserVer = /"cssparser (\d+\.\d+\.\d+)"/.exec(ammoniaBlock?.[1] ?? "")?.[1];
  const dtoaShortVer = /name = "dtoa-short"\nversion = "(\d+\.\d+\.\d+)"/m.exec(cargoLock)?.[1];
  if (cssparserVer && !page.includes(`https://crates.io/crates/cssparser/${cssparserVer}`)) {
    throw new Error(
      `src/app/licenses/page.tsx cssparser version does not match Cargo.lock (${cssparserVer})`,
    );
  }
  if (dtoaShortVer && !page.includes(`https://crates.io/crates/dtoa-short/${dtoaShortVer}`)) {
    throw new Error(
      `src/app/licenses/page.tsx dtoa-short version does not match Cargo.lock (${dtoaShortVer})`,
    );
  }

  const shimDir = resolveDependency(path.join(rootDir, "apps/web"), "@bjorn3/browser_wasi_shim");
  const shimMeta = JSON.parse(readFileSync(path.join(shimDir, "package.json"), "utf8"));
  if (!page.includes(`@bjorn3/browser_wasi_shim ${shimMeta.version}`)) {
    throw new Error(
      `src/app/licenses/page.tsx WASI shim version does not match ${shimMeta.version}`,
    );
  }
}

export function syncNotices({ rootDir = defaultRootDir, outDir = defaultOutDir } = {}) {
  const notices = expectedNotices(rootDir);
  mkdirSync(outDir, { recursive: true });
  for (const [name, content] of Object.entries(notices)) {
    writeFileSync(path.join(outDir, name), content, "utf8");
  }
}

export function checkNotices({
  rootDir = defaultRootDir,
  outDir = defaultOutDir,
  pagePath = defaultPagePath,
  checkFiles = true,
} = {}) {
  verifyLicensesPage(rootDir, pagePath);
  const notices = expectedNotices(rootDir);
  if (!checkFiles) {
    return;
  }
  for (const [name, expected] of Object.entries(notices)) {
    const target = path.join(outDir, name);
    if (!existsSync(target)) {
      throw new Error(
        `Missing generated web notice ${name} in ${outDir}. Run 'just web-licenses'.`,
      );
    }
    const actual = readFileSync(target, "utf8");
    if (actual !== expected) {
      throw new Error(
        `Generated web notice ${name} in ${outDir} is out of date. Run 'just web-licenses'.`,
      );
    }
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const outDirIndex = args.indexOf("--out-dir");
  const outDir =
    outDirIndex !== -1 && args[outDirIndex + 1]
      ? path.resolve(args[outDirIndex + 1])
      : defaultOutDir;
  const isCheck = args.includes("--check");
  const isCheckGraph = args.includes("--check-graph");
  try {
    if (isCheck || isCheckGraph) {
      checkNotices({ outDir, checkFiles: isCheck });
    } else {
      syncNotices({ outDir });
    }
  } catch (error) {
    console.error(`ERROR: ${error instanceof Error ? error.message : String(error)}`);
    process.exit(1);
  }
}
