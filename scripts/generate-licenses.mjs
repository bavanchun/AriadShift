// Keep generated license notices reproducible when upstream texts contain CRLF or trailing spaces.
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

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

export function readPinnedCargoAboutVersion(
  rootDir = defaultRootDir,
  args = process.argv.slice(2),
) {
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--cargo-about-version" && args[index + 1]) {
      return args[index + 1];
    }
    if (arg.startsWith("--cargo-about-version=")) {
      return arg.slice("--cargo-about-version=".length);
    }
  }
  if (process.env.CARGO_ABOUT_VERSION) {
    return process.env.CARGO_ABOUT_VERSION;
  }
  const justfilePath = path.join(rootDir, "justfile");
  if (existsSync(justfilePath)) {
    const match = /^cargo_about_version\s*:=\s*"([^"]+)"/m.exec(readFileSync(justfilePath, "utf8"));
    if (match?.[1]) {
      return match[1];
    }
  }
  return null;
}

export function checkCargoAbout(
  rootDir = defaultRootDir,
  expectedVersion = readPinnedCargoAboutVersion(rootDir),
) {
  const result = spawnSync("cargo", ["about", "--version"], {
    cwd: rootDir,
    encoding: "utf8",
  });
  const actual = (result.stdout ?? "").trim();
  const installHint = expectedVersion
    ? `Install with 'cargo install --locked cargo-about --version ${expectedVersion}'.`
    : "Install cargo-about first.";
  if (result.error || result.status !== 0) {
    const detail = actual || (result.stderr ?? "").trim() || result.error?.message;
    throw new Error(
      `cargo-about${expectedVersion ? ` ${expectedVersion}` : ""} is required${detail ? ` (${detail})` : ""}. ${installHint}`,
    );
  }
  if (expectedVersion) {
    const expected = `cargo-about ${expectedVersion}`;
    if (actual !== expected) {
      throw new Error(`cargo-about ${expectedVersion} required, found '${actual}'. ${installHint}`);
    }
  } else if (!/^cargo-about\s+\S+/.test(actual)) {
    throw new Error(`Unexpected 'cargo about --version' output: '${actual}'. ${installHint}`);
  }
}

export function generateLicenses({
  rootDir = defaultRootDir,
  expectedVersion = readPinnedCargoAboutVersion(rootDir),
} = {}) {
  checkCargoAbout(rootDir, expectedVersion);
  const result = spawnSync("cargo", ["about", "generate", "about.hbs"], {
    cwd: rootDir,
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
  });
  if (result.error || result.status !== 0) {
    const detail = (result.stderr ?? "").trim() || result.error?.message || `exit ${result.status}`;
    throw new Error(`cargo about generate about.hbs failed: ${detail}`);
  }
  const inventory = result.stdout ?? "";
  if (!inventory.trim()) {
    throw new Error("cargo about generate about.hbs produced empty output.");
  }

  const glyphlessLicense = readFileSync(
    path.join(rootDir, "crates/ariad-pdf/assets/glyphless-LICENSE.txt"),
    "utf8",
  );
  const liberationLicense = readFileSync(
    path.join(rootDir, "crates/ariad-pdf/assets/liberation/LICENSE-liberation.txt"),
    "utf8",
  );
  const srgbLicense = readFileSync(
    path.join(rootDir, "crates/ariad-pdf/assets/LICENSE-sRGB-v4.txt"),
    "utf8",
  );

  return [
    inventory.replace(/\r\n?/g, "\n").replace(/[\t ]+$/gm, ""),
    "\n\nBundled OCR font\n----------------\n",
    glyphlessLicense,
    "\n\nBundled compliance assets\n-------------------------\n",
    "Liberation 2.1.5 (OFL-1.1), unmodified font files.\n",
    "Source and SHA-256 receipts: crates/ariad-pdf/assets/compliance-assets.toml\n",
    liberationLicense,
    "\n\nCompact-ICC-Profiles sRGB-v4.icc (CC0-1.0), via krilla 0.8.2.\n",
    srgbLicense,
  ].join("");
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
  const rootDir = resolveRootDir(args);
  const expectedVersion = readPinnedCargoAboutVersion(rootDir, args);
  try {
    if (args.includes("--check-preconditions")) {
      checkCargoAbout(rootDir, expectedVersion);
    } else {
      const output = generateLicenses({ rootDir, expectedVersion });
      process.stdout.write(output);
    }
  } catch (error) {
    console.error(`ERROR: ${error instanceof Error ? error.message : String(error)}`);
    process.exit(1);
  }
}
