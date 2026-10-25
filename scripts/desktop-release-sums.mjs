// Collects the desktop installers and updater artifacts of a release, or of a local
// `target/release/bundle` tree, into one directory and writes `SHA256SUMS` over them.
//
//   node scripts/desktop-release-sums.mjs <out-dir>                  # GITHUB_REPOSITORY, RELEASE_TAG
//   node scripts/desktop-release-sums.mjs --from <bundle-dir> <out-dir>
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  closeSync,
  copyFileSync,
  mkdirSync,
  openSync,
  readdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

export const SUMS_FILE = "SHA256SUMS";

// Every release must carry each of these; one entry per installer or updater kind.
export const REQUIRED_KINDS = [
  ".AppImage",
  ".deb",
  ".rpm",
  ".dmg",
  ".app.tar.gz",
  "-setup.exe",
  ".msi",
];

const SAFE_NAME = /^[A-Za-z0-9][\w.+-]*$/;

export function isDesktopAsset(name, productName) {
  if (name === "latest.json") return true;
  if (!name.startsWith(productName)) return false;
  const artifact = name.endsWith(".sig") ? name.slice(0, -".sig".length) : name;
  return REQUIRED_KINDS.some((kind) => artifact.endsWith(kind));
}

export function selectAssets(names, productName) {
  const selected = names.filter((name) => isDesktopAsset(name, productName)).sort();
  for (const [index, name] of selected.entries()) {
    if (!SAFE_NAME.test(name)) throw new Error(`Unsafe desktop asset name: ${name}`);
    if (selected[index - 1] === name) throw new Error(`Duplicate desktop asset: ${name}`);
  }
  if (selected.length === 0) throw new Error("No desktop installers or updater artifacts found");
  return selected;
}

export function requireAllKinds(names) {
  const missing = REQUIRED_KINDS.filter((kind) => !names.some((name) => name.endsWith(kind)));
  if (!names.includes("latest.json")) missing.push("latest.json");
  if (missing.length > 0) throw new Error(`Release lacks desktop assets: ${missing.join(", ")}`);
}

export function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

// GNU coreutils format, checked by `sha256sum -c` and `shasum -a 256 -c`.
export function checksumFile(entries) {
  return [...entries]
    .sort((left, right) => (left.name < right.name ? -1 : 1))
    .map(({ name, digest }) => {
      if (!/^[0-9a-f]{64}$/.test(digest)) throw new Error(`Invalid SHA-256 for ${name}`);
      return `${digest}  ${name}\n`;
    })
    .join("");
}

export function productName() {
  const config = JSON.parse(
    readFileSync(new URL("../apps/desktop/src-tauri/tauri.conf.json", import.meta.url), "utf8"),
  );
  return config.productName;
}

// Installers sit directly in each bundle kind directory (`bundle/<kind>/<file>`).
export function localBundleFiles(bundleDir) {
  const files = new Map();
  for (const kind of readdirSync(bundleDir, { withFileTypes: true })) {
    if (!kind.isDirectory()) continue;
    for (const entry of readdirSync(join(bundleDir, kind.name), { withFileTypes: true })) {
      if (entry.isFile()) files.set(entry.name, join(bundleDir, kind.name, entry.name));
    }
  }
  return files;
}

export function collectLocal(bundleDir, outDir, name = productName()) {
  const files = localBundleFiles(bundleDir);
  mkdirSync(outDir, { recursive: true });
  const entries = selectAssets([...files.keys()], name).map((asset) => {
    const target = join(outDir, asset);
    copyFileSync(files.get(asset), target);
    return { name: asset, digest: sha256File(target) };
  });
  writeFileSync(join(outDir, SUMS_FILE), checksumFile(entries));
  return entries;
}

export function collectRelease(repository, tag, outDir, name = productName()) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository) || !/^v\d+\.\d+\.\d+$/.test(tag)) {
    throw new Error("Expected a repository and a stable vX.Y.Z release tag");
  }
  const release = JSON.parse(
    execFileSync("gh", ["api", `repos/${repository}/releases/tags/${tag}`], {
      encoding: "utf8",
      maxBuffer: 16 * 1024 * 1024,
    }),
  );
  const selected = selectAssets(
    release.assets.map((asset) => asset.name),
    name,
  );
  requireAllKinds(selected);
  mkdirSync(outDir, { recursive: true });
  const entries = selected.map((assetName) => {
    const asset = release.assets.find((candidate) => candidate.name === assetName);
    const target = join(outDir, assetName);
    const output = openSync(target, "w");
    try {
      execFileSync(
        "gh",
        [
          "api",
          `repos/${repository}/releases/assets/${asset.id}`,
          "-H",
          "Accept: application/octet-stream",
        ],
        { stdio: ["ignore", output, "inherit"] },
      );
    } finally {
      closeSync(output);
    }
    const digest = sha256File(target);
    // GitHub records a digest for each uploaded asset; a mismatch means a corrupt download.
    if (asset.digest && asset.digest !== `sha256:${digest}`) {
      throw new Error(`Downloaded ${assetName} differs from the digest GitHub recorded`);
    }
    return { name: assetName, digest };
  });
  writeFileSync(join(outDir, SUMS_FILE), checksumFile(entries));
  return entries;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [first, ...rest] = process.argv.slice(2);
  let entries;
  if (first === "--from" && rest.length === 2) {
    entries = collectLocal(rest[0], rest[1]);
  } else if (first && first !== "--from" && rest.length === 0) {
    entries = collectRelease(
      process.env.GITHUB_REPOSITORY ?? "",
      process.env.RELEASE_TAG ?? "",
      first,
    );
  } else {
    throw new Error("Usage: desktop-release-sums.mjs [--from <bundle-dir>] <out-dir>");
  }
  for (const { name, digest } of entries) console.log(`${digest}  ${name}`);
}
