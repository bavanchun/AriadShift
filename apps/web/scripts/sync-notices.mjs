// Copies NOTICE and filters THIRD_PARTY_LICENSES to the browser-WASM production
// closure in public/notices/, generates the web JavaScript dependency notices
// from the installed production graph, and validates notices, /licenses, Pandoc
// SOURCE.txt, and Turbopack module graph evidence against lockfiles.
import { execFileSync } from "node:child_process";
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
import { pandocArchiveSha256, pandocVersion, pandocWasmSha256 } from "../engine-assets.mjs";
import {
  pandocSourceInventory,
  renderPandocSourceTxt,
  verifyPandocSourceInventory,
} from "../pandoc-source-inventory.mjs";

const defaultWebDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoRootDir = path.resolve(defaultWebDir, "../..");
const defaultRootDir = process.env.ARIADSHIFT_ROOT_DIR
  ? path.resolve(process.env.ARIADSHIFT_ROOT_DIR)
  : repoRootDir;
const defaultOutDir = path.join(defaultRootDir, "apps/web/public/notices");
const defaultPagePath = path.join(defaultRootDir, "apps/web/src/app/licenses/page.tsx");

// Monorepo subpackages whose published tarballs omit the root LICENSE file
// that accompanies the main package in the same dependency graph.
const COMPANION_LICENSES = new Map([
  ["@better-auth/utils", ["better-auth", "LICENSE.md"]],
  ["@next/env", ["next", "license.md"]],
  ["client-only", ["react", "LICENSE"]],
]);

// Optional peer dependencies of next and better-auth that belong only to
// development, typechecking, or test runners and are never shipped in production.
const DEV_ONLY_PEERS = new Set([
  "@babel/core",
  "@playwright/test",
  "@types/node",
  "@types/react",
  "@types/react-dom",
  "vitest",
]);

// Packages declared in apps/web devDependencies that are traced into .next/server
// and embedded into generated assets (src/lib/og-image.tsx).
const BUNDLED_DEV_PACKAGES = new Set(["@fontsource/plus-jakarta-sans"]);

function normalize(text) {
  return `${text
    .replace(/\r\n?/g, "\n")
    .replace(/[\t ]+$/gm, "")
    .trimEnd()}\n`;
}

function unquote(value) {
  return value.trim().replace(/^['"]|['"]$/g, "");
}

function isOsSpecificPackage(metadata) {
  return Array.isArray(metadata.os) || Array.isArray(metadata.cpu);
}

function resolveDependency(directory, name, optional = false) {
  for (let current = directory; ; current = path.dirname(current)) {
    const candidate = path.join(current, "node_modules", name);
    if (existsSync(path.join(candidate, "package.json"))) {
      return realpathSync(candidate);
    }
    if (path.dirname(current) === current) {
      if (optional) {
        return null;
      }
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
    if (isOsSpecificPackage(metadata)) {
      return;
    }
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
    const depNames = Object.keys(metadata.dependencies ?? {});
    const optionalNames = Object.keys(metadata.optionalDependencies ?? {});
    const peerNames = Object.keys(metadata.peerDependencies ?? {});
    const bundledDevNames = Object.keys(metadata.devDependencies ?? {}).filter((name) =>
      BUNDLED_DEV_PACKAGES.has(name),
    );
    const allNames = new Set([...depNames, ...optionalNames, ...peerNames, ...bundledDevNames]);
    for (const name of [...allNames].sort()) {
      if (DEV_ONLY_PEERS.has(name)) {
        continue;
      }
      const isOptional =
        !Object.hasOwn(metadata.dependencies ?? {}, name) && !BUNDLED_DEV_PACKAGES.has(name);
      const resolved = resolveDependency(directory, name, isOptional);
      if (resolved) {
        visit(resolved);
      }
    }
  }

  visit(path.join(rootDir, "apps/web"));
  const header = [
    "Generated web JavaScript dependency notices. Run just web-licenses.",
    "Scope: dependencies, optionalDependencies, peerDependencies, and bundled font devDependencies (@fontsource/plus-jakarta-sans) in the @ariadshift/web production graph.",
    "Exclusions: host-OS-specific native Node/SWC/libvips binaries (os/cpu-constrained optional packages such as @img/sharp-*, @img/sharp-libvips-*, @next/swc-* that run only on the build/SSR host) and dev/test-only optional peers (@babel/core, @playwright/test, @types/node, @types/react, @types/react-dom, vitest).",
  ].join("\n");
  const output = [
    header,
    ...[...notices]
      .sort(([left], [right]) => left.localeCompare(right, "en"))
      .map(([name, notice]) => `${name}\n${"=".repeat(name.length)}\n\n${notice.trim()}`),
  ].join("\n\n");
  return normalize(output);
}

export function parsePnpmLockProdPackages(lockText) {
  const lines = lockText.split(/\r?\n/);
  let section = "";
  let currentImporter = null;
  let currentSub = null;
  let lastPkg = null;
  let currentPkgDef = null;
  let currentSnapshot = null;
  const importers = new Map();
  const packageDefs = new Map();
  const snapshots = new Map();

  for (const line of lines) {
    if (/^importers:$/.test(line)) {
      section = "importers";
      continue;
    }
    if (/^packages:$/.test(line)) {
      section = "packages";
      continue;
    }
    if (/^snapshots:$/.test(line)) {
      section = "snapshots";
      continue;
    }
    if (/^[a-zA-Z]/.test(line)) {
      section = "";
      continue;
    }

    if (section === "importers") {
      const impMatch = /^ {2}(\S[^:]*):\s*(\{\})?$/.exec(line);
      if (impMatch) {
        currentImporter = unquote(impMatch[1]);
        importers.set(currentImporter, {
          dependencies: {},
          optionalDependencies: {},
          devDependencies: {},
        });
        currentSub = null;
        lastPkg = null;
        continue;
      }
      const subMatch = /^ {4}(dependencies|optionalDependencies|devDependencies):$/.exec(line);
      if (subMatch) {
        currentSub = subMatch[1];
        lastPkg = null;
        continue;
      }
      if (
        currentImporter &&
        (currentSub === "dependencies" ||
          currentSub === "optionalDependencies" ||
          currentSub === "devDependencies")
      ) {
        const depMatch = /^ {6}(\S[^:]*):$/.exec(line);
        if (depMatch) {
          lastPkg = unquote(depMatch[1]);
          continue;
        }
        const verMatch = /^ {8}version:\s*(.+)$/.exec(line);
        if (
          verMatch &&
          lastPkg &&
          (currentSub !== "devDependencies" || BUNDLED_DEV_PACKAGES.has(lastPkg))
        ) {
          importers.get(currentImporter)[currentSub][lastPkg] = unquote(verMatch[1]);
        }
      }
    } else if (section === "packages") {
      const pkgMatch = /^ {2}(\S[^:]*):$/.exec(line);
      if (pkgMatch) {
        currentPkgDef = unquote(pkgMatch[1]);
        packageDefs.set(currentPkgDef, { osSpecific: false });
        continue;
      }
      if (currentPkgDef && /^ {4}(os|cpu):/.test(line)) {
        packageDefs.get(currentPkgDef).osSpecific = true;
      }
    } else if (section === "snapshots") {
      const snapMatch = /^ {2}(\S[^:]*):\s*(\{\})?$/.exec(line);
      if (snapMatch) {
        currentSnapshot = unquote(snapMatch[1]);
        snapshots.set(currentSnapshot, { dependencies: {}, optionalDependencies: {} });
        currentSub = null;
        continue;
      }
      const subMatch = /^ {4}(dependencies|optionalDependencies|transitivePeerDependencies):$/.exec(
        line,
      );
      if (subMatch) {
        currentSub = subMatch[1];
        continue;
      }
      if (
        currentSnapshot &&
        (currentSub === "dependencies" || currentSub === "optionalDependencies")
      ) {
        const depMatch = /^ {6}(\S[^:]*):\s*(.+)$/.exec(line);
        if (depMatch) {
          const pkg = unquote(depMatch[1]);
          const ver = unquote(depMatch[2]);
          snapshots.get(currentSnapshot)[currentSub][pkg] = ver;
        }
      }
    }
  }

  const visitedImporters = new Set();
  const visitedSnapshots = new Set();
  const prodPackages = new Set();

  function stripPeerSuffix(ver) {
    const idx = ver.indexOf("(");
    return idx === -1 ? ver : ver.slice(0, idx);
  }

  function visitSnapshot(key) {
    if (visitedSnapshots.has(key)) {
      return;
    }
    visitedSnapshots.add(key);
    const snap = snapshots.get(key);
    if (!snap) {
      throw new Error(
        `JavaScript notices cannot be checked: pnpm-lock.yaml is missing snapshot for ${key}`,
      );
    }
    const atIdx = key.indexOf("@", key.startsWith("@") ? 1 : 0);
    const name = key.slice(0, atIdx);
    const ver = stripPeerSuffix(key.slice(atIdx + 1));
    const pkgId = `${name}@${ver}`;
    if (packageDefs.get(pkgId)?.osSpecific) {
      return;
    }
    prodPackages.add(pkgId);
    for (const [depName, depVer] of [
      ...Object.entries(snap.dependencies),
      ...Object.entries(snap.optionalDependencies),
    ]) {
      if (DEV_ONLY_PEERS.has(depName) || depVer.startsWith("link:")) {
        continue;
      }
      visitSnapshot(`${depName}@${depVer}`);
    }
  }

  function visitImporter(impPath) {
    if (visitedImporters.has(impPath)) {
      return;
    }
    visitedImporters.add(impPath);
    const imp = importers.get(impPath);
    if (!imp) {
      throw new Error(
        `JavaScript notices cannot be checked: pnpm-lock.yaml is missing importer ${impPath}`,
      );
    }
    for (const [depName, depVer] of [
      ...Object.entries(imp.dependencies),
      ...Object.entries(imp.optionalDependencies),
      ...Object.entries(imp.devDependencies),
    ]) {
      if (depVer.startsWith("link:")) {
        const rel = depVer.slice("link:".length);
        const target = path.posix.normalize(`${impPath}/${rel}`);
        visitImporter(target);
      } else {
        visitSnapshot(`${depName}@${depVer}`);
      }
    }
  }

  visitImporter("apps/web");
  return prodPackages;
}

export function verifyPnpmLockAgainstJsNotices(rootDir, jsNoticesText) {
  const lockPath = path.join(rootDir, "pnpm-lock.yaml");
  const lockPackages = parsePnpmLockProdPackages(readFileSync(lockPath, "utf8"));
  const noticePackages = new Set(
    [...jsNoticesText.matchAll(/^([^\n=]+@\d+[^\n]*)\n=+/gm)].map((match) => match[1]),
  );
  const missingFromNotices = [...lockPackages].filter((pkg) => !noticePackages.has(pkg)).sort();
  const extraInNotices = [...noticePackages].filter((pkg) => !lockPackages.has(pkg)).sort();
  if (missingFromNotices.length > 0 || extraInNotices.length > 0) {
    throw new Error(
      `JavaScript notices disagree with pnpm-lock.yaml (missing: ${missingFromNotices.join(", ") || "none"}; extra: ${extraInNotices.join(", ") || "none"})`,
    );
  }
}

export function computeWasmRustProductionClosure(rootDir = defaultRootDir) {
  const manifestPath = existsSync(path.join(rootDir, "Cargo.toml"))
    ? path.join(rootDir, "Cargo.toml")
    : path.join(repoRootDir, "Cargo.toml");
  const treeOutput = execFileSync(
    "cargo",
    [
      "tree",
      "--manifest-path",
      manifestPath,
      "--locked",
      "-p",
      "ariad-wasm",
      "-p",
      "ariad-typst",
      "--target",
      "wasm32-unknown-unknown",
      "--edges",
      "normal",
      "--prefix",
      "none",
      "--format",
      "{p}",
    ],
    { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 },
  );
  const closure = new Set();
  for (const line of treeOutput.split(/\r?\n/)) {
    const match = /^([a-zA-Z0-9_-]+) v(\d+\.\d+\.\d+[^\s]*)/.exec(line.trim());
    if (match && !["ariad-core", "ariad-wasm", "ariad-typst"].includes(match[1])) {
      closure.add(`${match[1]} ${match[2]}`);
    }
  }
  if (closure.size === 0) {
    throw new Error("cargo tree returned an empty wasm32-unknown-unknown production closure");
  }
  const cargoLock = readFileSync(path.join(rootDir, "Cargo.lock"), "utf8");
  const lockPackages = new Set();
  for (const block of cargoLock.split(/\r?\n\[\[package\]\]\r?\n/).slice(1)) {
    const name = /^name = "([^"]+)"/m.exec(block)?.[1];
    const version = /^version = "([^"]+)"/m.exec(block)?.[1];
    if (name && version) {
      lockPackages.add(`${name} ${version}`);
    }
  }
  const missingInCargoLock = [...closure].filter((entry) => !lockPackages.has(entry)).sort();
  if (missingInCargoLock.length > 0) {
    throw new Error(
      `Cargo.lock disagrees with wasm32-unknown-unknown production dependency closure (missing from Cargo.lock: ${missingInCargoLock.slice(0, 10).join(", ")})`,
    );
  }
  return closure;
}

export function generateWebRustNotices(rootDir = defaultRootDir) {
  const wasmClosure = computeWasmRustProductionClosure(rootDir);
  const raw = readFileSync(path.join(rootDir, "THIRD_PARTY_LICENSES"), "utf8");
  const separator =
    "================================================================================";
  const parts = raw.split(separator);
  const keptSections = [];
  const tocOrder = [];
  const tocCounts = new Map();

  for (let index = 1; index < parts.length; index += 1) {
    const section = parts[index];
    if (!section?.trim()) {
      continue;
    }
    const licMatch = /License:\s*([^\n]+)\s*\(([^)]+)\)/.exec(section);
    const usedByMatches = [
      ...section.matchAll(/^ {2}- ([a-zA-Z0-9_-]+ \d+\.\d+\.\d+[^\s(]*).*$/gm),
    ];
    const keptLines = usedByMatches.filter((m) => wasmClosure.has(m[1])).map((m) => m[0]);
    if (keptLines.length === 0) {
      continue;
    }
    if (licMatch) {
      const label = `${licMatch[1].trim()} (${licMatch[2].trim()})`;
      if (!tocCounts.has(label)) {
        tocOrder.push(label);
        tocCounts.set(label, 0);
      }
      tocCounts.set(label, tocCounts.get(label) + keptLines.length);
    }
    const updatedSection = section.replace(
      /Used by:\r?\n(?: {2}- [^\r\n]+\r?\n)+/,
      `Used by:\n${keptLines.join("\n")}\n`,
    );
    keptSections.push(updatedSection);
  }

  const tocLines = tocOrder
    .sort((left, right) => {
      const diff = (tocCounts.get(right) ?? 0) - (tocCounts.get(left) ?? 0);
      return diff !== 0 ? diff : left.localeCompare(right, "en");
    })
    .map((label) => `- ${label}: ${tocCounts.get(label)} component(s)`);

  const header = [
    "Third-Party Licenses for AriadShift (Browser WebAssembly Crates)",
    "================================================================",
    "",
    "This document contains licensing notices and terms for third-party Rust",
    "crates in the wasm32-unknown-unknown production dependency closure of",
    "ariad-wasm and ariad-typst, including their dependencies.",
    "",
    "Table of Contents",
    "-----------------",
    ...tocLines,
    "",
    separator,
  ].join("\n");

  const output = normalize(`${header}${keptSections.join(separator)}${separator}\n`);
  verifyCargoLockAgainstRustNotices(rootDir, output, wasmClosure);
  return output;
}

export function verifyCargoLockAgainstRustNotices(
  rootDir,
  rustNoticesText,
  wasmClosure = computeWasmRustProductionClosure(rootDir),
) {
  const listedSet = new Set(
    [...rustNoticesText.matchAll(/^ {2}- ([a-zA-Z0-9_-]+ \d+\.\d+\.\d+[^\s(]*)/gm)].map(
      (match) => match[1],
    ),
  );
  if (listedSet.size === 0) {
    throw new Error("THIRD_PARTY_LICENSES contains no crate entries");
  }
  const missingFromNotices = [...wasmClosure].filter((entry) => !listedSet.has(entry)).sort();
  const extraInNotices = [...listedSet].filter((entry) => !wasmClosure.has(entry)).sort();
  if (missingFromNotices.length > 0 || extraInNotices.length > 0) {
    throw new Error(
      `Web Rust notices disagree with wasm32-unknown-unknown production dependency closure and Cargo.lock (missing: ${missingFromNotices.slice(0, 10).join(", ") || "none"}; extra: ${extraInNotices.slice(0, 10).join(", ") || "none"})`,
    );
  }
}

export function collectBundledNodeModulesPackages(webDir) {
  let output;
  try {
    output = execFileSync("pnpm", ["exec", "next", "analyze", "export"], {
      cwd: webDir,
      encoding: "utf8",
      maxBuffer: 128 * 1024 * 1024,
    });
  } catch (error) {
    throw new Error(
      `Cannot read Next.js bundle analysis; run next build --analyze: ${error.message}`,
    );
  }
  return parseClientBundleAnalysis(output);
}

export function parseClientBundleAnalysis(output) {
  const bundled = new Set();
  let schema = false;
  let clientParts = 0;
  for (const line of output.split(/\r?\n/)) {
    if (!line) continue;
    let record;
    try {
      record = JSON.parse(line);
    } catch {
      throw new Error("Unrecognized Next.js bundle analysis: invalid JSON Lines");
    }
    if (record.type === "meta") {
      schema = record.schema_version === 1 && record.selected_routes > 0;
    }
    if (record.type !== "part" || !record.filename?.startsWith("[client-fs]/")) continue;
    clientParts += 1;
    const source = record.source_path;
    if (typeof source !== "string") {
      throw new Error("Unrecognized client module in Next.js bundle analysis");
    }
    if (!source.includes("/node_modules/")) continue;
    const match = /\/node_modules\/\.pnpm\/([^/]+)\/node_modules\/(@[^/]+\/[^/]+|[^/]+)\//.exec(
      source,
    );
    if (!match) throw new Error(`Unrecognized bundled package path: ${source}`);
    const [, folder, name] = match;
    const prefix = `${name.replace("/", "+")}@`;
    if (!folder.startsWith(prefix)) throw new Error(`Unrecognized package version: ${source}`);
    const version = folder.slice(prefix.length).split("_")[0];
    if (!/^\d+\.\d+\.\d+/.test(version)) {
      throw new Error(`Unrecognized package version: ${source}`);
    }
    bundled.add(`${name}@${version}`);
  }
  const result = [...bundled].sort();
  if (
    !schema ||
    !clientParts ||
    !result.some((pkg) => pkg.startsWith("@bjorn3/browser_wasi_shim@"))
  ) {
    throw new Error("Empty or unrecognized Next.js client bundle analysis, or WASI shim missing");
  }
  return result;
}

export function verifyBundleAgainstJsNotices(webDir, jsNoticesText) {
  const bundledPackages = collectBundledNodeModulesPackages(webDir);
  const noticePackages = new Set(
    [...jsNoticesText.matchAll(/^([^\n=]+@\d+[^\n]*)\n=+/gm)].map((match) => match[1]),
  );
  const missing = bundledPackages.filter((pkg) => !noticePackages.has(pkg));
  if (missing.length > 0) {
    throw new Error(
      `Production bundle references node_modules packages missing from THIRD_PARTY_JS_LICENSES.txt: ${missing.join(", ")}`,
    );
  }
}

export function generateWebNoticeTxt(rootDir = defaultRootDir, inventory = pandocSourceInventory) {
  const rootNotice = readFileSync(path.join(rootDir, "NOTICE"), "utf8").trimEnd();
  const pandocSection = [
    "------------------------------------------------------------------------------",
    "",
    `Pandoc ${inventory.version} WebAssembly Module (Browser DOCX/EPUB Engine)`,
    "",
    `The web application distributes the unmodified official pandoc.wasm binary (${inventory.license}) as a separate WebAssembly file (public/wasm/pandoc/pandoc.wasm), invoked out of process in a Web Worker via WASI.`,
    `Upstream release page (tag): ${inventory.releaseTagUrl}`,
    `Upstream release asset (zip): ${inventory.archiveUrl}`,
    `Release archive SHA-256: ${inventory.archiveSha256} (upstream published GitHub Release asset digest: ${inventory.upstreamArchiveDigest})`,
    `Shipped pandoc.wasm SHA-256: ${inventory.wasmSha256} (upstream publishes no separate standalone digest for the inner pandoc.wasm file)`,
    `Upstream build definitions (tag ${inventory.gitTag}): ${inventory.recipe.files.map((item) => item.url).join(", ")}`,
    `Upstream freeze/lock status: ${inventory.freezeAndLockFiles.summary}`,
    ...inventory.freezeAndLockFiles.unverifiedFacts,
    "",
    "Written Corresponding-Source Offer (Three Years):",
    inventory.writtenOffer.text,
  ].join("\n");
  return normalize(`${rootNotice}\n\n${pandocSection}\n`);
}

export function expectedNotices(rootDir = defaultRootDir) {
  return {
    "NOTICE.txt": generateWebNoticeTxt(rootDir),
    "THIRD_PARTY_LICENSES.txt": generateWebRustNotices(rootDir),
    "THIRD_PARTY_JS_LICENSES.txt": generateWebJsNotices(rootDir),
  };
}

function readPandocEngineAssets(webDir) {
  const engineAssetsPath = path.join(webDir, "engine-assets.mjs");
  if (!existsSync(engineAssetsPath)) {
    return {
      version: pandocVersion,
      archiveSha256: pandocArchiveSha256,
      wasmSha256: pandocWasmSha256,
    };
  }
  const text = readFileSync(engineAssetsPath, "utf8");
  const version = /pandocVersion\s*=\s*"([^"]+)"/.exec(text)?.[1] ?? pandocVersion;
  const archiveSha256 =
    /pandocArchiveSha256\s*=\s*"([^"]+)"/.exec(text)?.[1] ?? pandocArchiveSha256;
  const wasmSha256 = /pandocWasmSha256\s*=\s*"([^"]+)"/.exec(text)?.[1] ?? pandocWasmSha256;
  return { version, archiveSha256, wasmSha256 };
}

export function verifyLicensesPage(
  rootDir = defaultRootDir,
  pagePath = defaultPagePath,
  { verifyWasmBinary = false } = {},
) {
  const webDir = path.join(rootDir, "apps/web");
  const wasmPath = path.join(webDir, "public/wasm/pandoc/pandoc.wasm");
  const engineAssets = readPandocEngineAssets(webDir);
  verifyPandocSourceInventory({
    version: engineAssets.version,
    archiveSha256: engineAssets.archiveSha256,
    wasmSha256: engineAssets.wasmSha256,
    wasmBytes: verifyWasmBinary && existsSync(wasmPath) ? readFileSync(wasmPath) : null,
  });

  const sourceTxtPath = path.join(webDir, "public/wasm/pandoc/SOURCE.txt");
  if (existsSync(sourceTxtPath)) {
    const actualSourceTxt = normalize(readFileSync(sourceTxtPath, "utf8"));
    const expectedSourceTxt = normalize(renderPandocSourceTxt(pandocSourceInventory));
    if (actualSourceTxt !== expectedSourceTxt) {
      throw new Error(
        "public/wasm/pandoc/SOURCE.txt is out of date with pandoc-source-inventory.mjs. Run 'node apps/web/scripts/sync-pandoc.mjs'.",
      );
    }
  }

  const page = readFileSync(pagePath, "utf8");
  for (const requiredHref of [
    "wasm/pandoc/COPYING.md",
    "wasm/pandoc/COPYRIGHT",
    "wasm/pandoc/SOURCE.txt",
    "notices/NOTICE.txt",
    "notices/THIRD_PARTY_LICENSES.txt",
    "notices/THIRD_PARTY_JS_LICENSES.txt",
  ]) {
    if (!page.includes(`href="${requiredHref}"`)) {
      throw new Error(`src/app/licenses/page.tsx is missing relative link ${requiredHref}`);
    }
  }
  if (/href="\/(?:notices|wasm\/pandoc)\//.test(page)) {
    throw new Error(
      "src/app/licenses/page.tsx must use document-relative notice/wasm links so path-prefix hosting works",
    );
  }
  if (
    page.includes("blob/dev/NOTICE") ||
    /\/(?:blob|tree)\/(?:main|master|dev|HEAD)\b/.test(page)
  ) {
    throw new Error(
      "src/app/licenses/page.tsx must not link to a moving branch (main/master/dev/HEAD)",
    );
  }
  for (const requiredRef of [
    "pandocSourceInventory.releaseTagUrl",
    "pandocSourceInventory.archiveUrl",
    "pandocSourceInventory.archiveSha256",
    "pandocSourceInventory.upstreamArchiveDigest",
    "pandocSourceInventory.wasmSha256",
    "pandocSourceInventory.hashComparisonNote",
    "pandocSourceInventory.recipe.files",
    "pandocSourceInventory.freezeAndLockFiles",
    "pandocSourceInventory.writtenOffer.text",
    "pandocSourceInventory.writtenOffer.contactUrl",
  ]) {
    if (!page.includes(requiredRef)) {
      throw new Error(
        `src/app/licenses/page.tsx is missing required Pandoc source disclosure reference: ${requiredRef}`,
      );
    }
  }

  const cargoLock = readFileSync(path.join(rootDir, "Cargo.lock"), "utf8");
  const ammoniaBlock = /name = "ammonia"[\s\S]*?dependencies = \[([\s\S]*?)\]/m.exec(cargoLock);
  const cssparserVer = /"cssparser (\d+\.\d+\.\d+)"/.exec(ammoniaBlock?.[1] ?? "")?.[1];
  const dtoaShortVer = /name = "dtoa-short"\nversion = "(\d+\.\d+\.\d+)"/m.exec(cargoLock)?.[1];
  if (!cssparserVer || !page.includes(`https://crates.io/crates/cssparser/${cssparserVer}`)) {
    throw new Error(
      `src/app/licenses/page.tsx cssparser version does not match Cargo.lock (${cssparserVer ?? "missing"})`,
    );
  }
  if (!dtoaShortVer || !page.includes(`https://crates.io/crates/dtoa-short/${dtoaShortVer}`)) {
    throw new Error(
      `src/app/licenses/page.tsx dtoa-short version does not match Cargo.lock (${dtoaShortVer ?? "missing"})`,
    );
  }

  const lockPackages = parsePnpmLockProdPackages(
    readFileSync(path.join(rootDir, "pnpm-lock.yaml"), "utf8"),
  );
  const shimDir = resolveDependency(webDir, "@bjorn3/browser_wasi_shim");
  const shimMeta = JSON.parse(readFileSync(path.join(shimDir, "package.json"), "utf8"));
  const shimId = `@bjorn3/browser_wasi_shim@${shimMeta.version}`;
  if (!lockPackages.has(shimId)) {
    throw new Error(`pnpm-lock.yaml does not match installed ${shimId}`);
  }
  if (
    !page.includes(`@bjorn3/browser_wasi_shim`) ||
    !page.includes(shimMeta.version) ||
    !page.includes(`https://github.com/bjorn3/browser_wasi_shim/tree/v${shimMeta.version}`)
  ) {
    throw new Error(
      `src/app/licenses/page.tsx WASI shim version or tag URL does not match ${shimMeta.version}`,
    );
  }
  if (/Copyright \(c\) bjorn3/i.test(page)) {
    throw new Error(
      "src/app/licenses/page.tsx must not present a constructed Copyright (c) bjorn3 header line as upstream LICENSE-MIT text",
    );
  }
  const upstreamShimMit = readFileSync(path.join(shimDir, "LICENSE-MIT"), "utf8").trim();
  if (!page.includes(upstreamShimMit)) {
    throw new Error(
      "src/app/licenses/page.tsx does not include the verbatim upstream @bjorn3/browser_wasi_shim LICENSE-MIT text",
    );
  }
}

export function validateNoticesAndLockfiles({
  rootDir = defaultRootDir,
  pagePath = defaultPagePath,
  verifyWasmBinary = false,
  checkBundle = false,
} = {}) {
  verifyLicensesPage(rootDir, pagePath, { verifyWasmBinary });
  const notices = expectedNotices(rootDir);
  verifyPnpmLockAgainstJsNotices(rootDir, notices["THIRD_PARTY_JS_LICENSES.txt"]);
  if (checkBundle) {
    verifyBundleAgainstJsNotices(
      path.join(rootDir, "apps/web"),
      notices["THIRD_PARTY_JS_LICENSES.txt"],
    );
  }
  return notices;
}

export function syncNotices({
  rootDir = defaultRootDir,
  outDir = defaultOutDir,
  pagePath = path.join(rootDir, "apps/web/src/app/licenses/page.tsx"),
} = {}) {
  const notices = validateNoticesAndLockfiles({ rootDir, pagePath });
  mkdirSync(outDir, { recursive: true });
  for (const [name, content] of Object.entries(notices)) {
    writeFileSync(path.join(outDir, name), content, "utf8");
  }
  const exportNoticesDir = path.join(rootDir, "apps/web/out/notices");
  if (outDir === path.join(rootDir, "apps/web/public/notices") && existsSync(exportNoticesDir)) {
    for (const [name, content] of Object.entries(notices)) {
      writeFileSync(path.join(exportNoticesDir, name), content, "utf8");
    }
  }
}

export function checkNotices({
  rootDir = defaultRootDir,
  outDir = defaultOutDir,
  pagePath = path.join(rootDir, "apps/web/src/app/licenses/page.tsx"),
  checkFiles = true,
} = {}) {
  const isDefaultNoticesDir =
    rootDir === defaultRootDir && outDir === path.join(rootDir, "apps/web/public/notices");
  const notices = validateNoticesAndLockfiles({
    rootDir,
    pagePath,
    verifyWasmBinary: isDefaultNoticesDir,
    checkBundle: isDefaultNoticesDir,
  });
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
  const exportOutDir = path.join(rootDir, "apps/web/out");
  if (outDir === path.join(rootDir, "apps/web/public/notices")) {
    const nextHtmlPath = path.join(rootDir, "apps/web/.next/server/app/licenses.html");
    if (existsSync(nextHtmlPath)) {
      const nextHtml = readFileSync(nextHtmlPath, "utf8");
      const nextLinks = [...nextHtml.matchAll(/href="((?:notices|wasm\/pandoc)\/[^"]+)"/g)].map(
        (m) => m[1],
      );
      if (nextLinks.length < 6) {
        throw new Error(
          ".next/server/app/licenses.html is missing relative notice/wasm links. Rebuild with 'pnpm --filter @ariadshift/web build'.",
        );
      }
      for (const href of nextLinks) {
        const publicTarget = path.join(rootDir, "apps/web/public", href);
        if (!existsSync(publicTarget)) {
          throw new Error(`.next/server/app/licenses.html link ${href} is missing in public/`);
        }
      }
    }
    if (existsSync(path.join(exportOutDir, "licenses.html"))) {
      const exportHtml = readFileSync(path.join(exportOutDir, "licenses.html"), "utf8");
      const exportLinks = [...exportHtml.matchAll(/href="((?:notices|wasm\/pandoc)\/[^"]+)"/g)].map(
        (m) => m[1],
      );
      if (exportLinks.length < 6) {
        throw new Error("out/licenses.html is missing relative notice/wasm links.");
      }
      for (const href of exportLinks) {
        const exportTarget = path.join(exportOutDir, href);
        if (!existsSync(exportTarget)) {
          throw new Error(`out/licenses.html link ${href} is missing in out/`);
        }
      }
      for (const [name, expected] of Object.entries(notices)) {
        const exported = path.join(exportOutDir, "notices", name);
        if (!existsSync(exported) || readFileSync(exported, "utf8") !== expected) {
          throw new Error(
            `Static export notice ${name} in ${exportOutDir}/notices is missing or out of date.`,
          );
        }
      }
    }
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const rootDirIndex = args.indexOf("--root-dir");
  const rootDir =
    rootDirIndex !== -1 && args[rootDirIndex + 1]
      ? path.resolve(args[rootDirIndex + 1])
      : defaultRootDir;
  const outDirIndex = args.indexOf("--out-dir");
  const outDir =
    outDirIndex !== -1 && args[outDirIndex + 1]
      ? path.resolve(args[outDirIndex + 1])
      : path.join(rootDir, "apps/web/public/notices");
  const pagePath = path.join(rootDir, "apps/web/src/app/licenses/page.tsx");
  const isCheck = args.includes("--check");
  const isCheckGraph = args.includes("--check-graph");
  try {
    if (isCheck || isCheckGraph) {
      checkNotices({ rootDir, outDir, pagePath, checkFiles: isCheck });
    } else {
      syncNotices({ rootDir, outDir, pagePath });
    }
  } catch (error) {
    console.error(`ERROR: ${error instanceof Error ? error.message : String(error)}`);
    process.exit(1);
  }
}
