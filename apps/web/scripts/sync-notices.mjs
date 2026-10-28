// Copies NOTICE and THIRD_PARTY_LICENSES into public/notices/, generates the
// web JavaScript dependency notices from the installed production graph, and
// validates notices, /licenses, Pandoc SOURCE.txt, and bundles against lockfiles.
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
import { pandocVersion, pandocWasmSha256 } from "../engine-assets.mjs";
import {
  pandocSourceInventory,
  renderPandocSourceTxt,
  verifyPandocSourceInventory,
} from "../pandoc-source-inventory.mjs";

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
      throw new Error(`pnpm-lock.yaml is missing snapshot for ${key}`);
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
      throw new Error(`pnpm-lock.yaml is missing importer ${impPath}`);
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

function parseDevCargoTomlDeps(tomlText) {
  const deps = new Set();
  let inDev = false;
  for (const line of tomlText.split(/\r?\n/)) {
    const sec = /^\[([^\]]+)\]/.exec(line)?.[1];
    if (sec) {
      inDev = sec === "dev-dependencies" || sec.endsWith(".dev-dependencies");
      continue;
    }
    if (inDev) {
      const match = /^([a-zA-Z0-9_-]+)(?:\.workspace)?\s*=/.exec(line);
      if (match) {
        deps.add(match[1]);
      }
    }
  }
  return deps;
}

export function verifyCargoLockAgainstRustNotices(rootDir, rustNoticesText) {
  const cargoLock = readFileSync(path.join(rootDir, "Cargo.lock"), "utf8");
  const devDeps = new Set();
  for (const rel of [
    "crates/ariad-core/Cargo.toml",
    "crates/ariad-wasm/Cargo.toml",
    "crates/ariad-typst/Cargo.toml",
  ]) {
    const full = path.join(rootDir, rel);
    if (existsSync(full)) {
      for (const dep of parseDevCargoTomlDeps(readFileSync(full, "utf8"))) {
        devDeps.add(dep);
      }
    }
  }

  const byName = new Map();
  const lockPackages = new Set();
  for (const block of cargoLock.split(/\r?\n\[\[package\]\]\r?\n/).slice(1)) {
    const name = /^name = "([^"]+)"/m.exec(block)?.[1];
    const version = /^version = "([^"]+)"/m.exec(block)?.[1];
    const depsMatch = /^dependencies = \[([\s\S]*?)^\]/m.exec(block);
    const deps = depsMatch ? [...depsMatch[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]) : [];
    if (name && version) {
      lockPackages.add(`${name} ${version}`);
      if (!byName.has(name)) {
        byName.set(name, []);
      }
      byName.get(name).push({ name, version, deps });
    }
  }

  const listedSet = new Set(
    [...rustNoticesText.matchAll(/^ {2}- ([a-zA-Z0-9_-]+ \d+\.\d+\.\d+[^\s(]*)/gm)].map(
      (match) => match[1],
    ),
  );
  if (listedSet.size === 0) {
    throw new Error("THIRD_PARTY_LICENSES contains no crate entries");
  }
  const notInLock = [...listedSet].filter((entry) => !lockPackages.has(entry)).sort();
  if (notInLock.length > 0) {
    throw new Error(
      `THIRD_PARTY_LICENSES disagrees with Cargo.lock (entries not in Cargo.lock: ${notInLock.slice(0, 10).join(", ")})`,
    );
  }

  const missingWasmDeps = [];
  for (const rootName of ["ariad-core", "ariad-wasm", "ariad-typst"]) {
    const pkgs = byName.get(rootName) ?? [];
    if (pkgs.length === 0) {
      throw new Error(`Cargo.lock is missing workspace crate ${rootName}`);
    }
    for (const pkg of pkgs) {
      for (const depSpec of pkg.deps) {
        const [depName, depVer] = depSpec.split(" ");
        if (depName.startsWith("ariad-") || devDeps.has(depName)) {
          continue;
        }
        const candidates = (byName.get(depName) ?? []).filter(
          (cand) => !depVer || cand.version === depVer || cand.version.startsWith(`${depVer}.`),
        );
        if (candidates.length === 0) {
          throw new Error(`Cargo.lock is missing dependency ${depSpec} of ${rootName}`);
        }
        for (const cand of candidates) {
          const id = `${cand.name} ${cand.version}`;
          if (!listedSet.has(id)) {
            missingWasmDeps.push(id);
          }
        }
      }
    }
  }
  if (missingWasmDeps.length > 0) {
    throw new Error(
      `THIRD_PARTY_LICENSES is missing Cargo.lock dependencies for browser WASM crates: ${missingWasmDeps.join(", ")}`,
    );
  }
}

export function collectBundledNodeModulesPackages(webDir) {
  const bundled = new Set();
  const pnpmPathRe =
    /node_modules\/\.pnpm\/[^/]+\/node_modules\/((?:@[a-zA-Z0-9._-]+\/[a-zA-Z0-9._-]+)|(?:[a-zA-Z0-9._-]+))\//g;

  function isExcludedBundleReference(pkgName) {
    if (pkgName === "shared" || pkgName.startsWith("@ariadshift/")) {
      return true;
    }
    // Host-OS-specific native Node add-on binaries traced into .nft.json for server image optimization.
    if (
      pkgName.startsWith("@img/sharp-libvips-") ||
      pkgName.startsWith("@img/sharp-linux-") ||
      pkgName.startsWith("@img/sharp-darwin-") ||
      pkgName.startsWith("@img/sharp-win32-") ||
      pkgName.startsWith("@img/sharp-wasm32") ||
      pkgName.startsWith("@next/swc-")
    ) {
      return true;
    }
    return false;
  }

  function walk(dir) {
    if (!existsSync(dir)) {
      return;
    }
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const fullPath = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if (
          entry.name === "cache" ||
          entry.name === "dev" ||
          entry.name === "build" ||
          entry.name === "types"
        ) {
          continue;
        }
        walk(fullPath);
      } else if (
        entry.name.endsWith(".nft.json") ||
        entry.name.endsWith(".map") ||
        entry.name.endsWith("_client-reference-manifest.js") ||
        entry.name.endsWith(".js")
      ) {
        const text = readFileSync(fullPath, "utf8");
        for (const match of text.matchAll(pnpmPathRe)) {
          const pkgName = match[1];
          if (pkgName && !isExcludedBundleReference(pkgName)) {
            bundled.add(pkgName);
          }
        }
      }
    }
  }

  walk(path.join(webDir, ".next"));
  walk(path.join(webDir, "out/_next"));
  return [...bundled].sort();
}

export function verifyBundleAgainstJsNotices(webDir, jsNoticesText) {
  const bundledPackages = collectBundledNodeModulesPackages(webDir);
  if (bundledPackages.length === 0) {
    return;
  }
  const noticePackageNames = new Set(
    [...jsNoticesText.matchAll(/^([^\n=]+)@\d+[^\n]*\n=+/gm)].map((match) => match[1]),
  );
  const missing = bundledPackages.filter((pkg) => !noticePackageNames.has(pkg));
  if (missing.length > 0) {
    throw new Error(
      `Production bundle references node_modules packages missing from THIRD_PARTY_JS_LICENSES.txt: ${missing.join(", ")}`,
    );
  }
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

export function verifyLicensesPage(
  rootDir = defaultRootDir,
  pagePath = defaultPagePath,
  { verifyWasmBinary = false } = {},
) {
  const webDir = path.join(rootDir, "apps/web");
  const wasmPath = path.join(webDir, "public/wasm/pandoc/pandoc.wasm");
  verifyPandocSourceInventory({
    version: pandocVersion,
    wasmSha256: pandocWasmSha256,
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
  if (page.includes("blob/dev/NOTICE")) {
    throw new Error("src/app/licenses/page.tsx still links to the moving dev branch NOTICE");
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
} = {}) {
  verifyLicensesPage(rootDir, pagePath, { verifyWasmBinary });
  const notices = expectedNotices(rootDir);
  verifyCargoLockAgainstRustNotices(rootDir, notices["THIRD_PARTY_LICENSES.txt"]);
  verifyPnpmLockAgainstJsNotices(rootDir, notices["THIRD_PARTY_JS_LICENSES.txt"]);
  verifyBundleAgainstJsNotices(
    path.join(rootDir, "apps/web"),
    notices["THIRD_PARTY_JS_LICENSES.txt"],
  );
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
