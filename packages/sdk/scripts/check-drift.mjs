// Fails when the generated client would change without review.
//
// The client is generated at build time and not committed. generator.lock.json pins
// everything the output depends on: the generator version, the OpenAPI document and
// the generator configuration. Generation is deterministic, so the output changes
// exactly when one of these does. A changed input fails the check until someone
// reviews the new client and runs `pnpm --filter @ariadshift/sdk lock`.
//
// The check also regenerates into a scratch directory and fails when an existing
// src/generated/ differs from it, which catches stale or hand-edited output.
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { outputPath, specPath } from "../openapi-ts.config.mjs";
import { generate } from "./generate.mjs";

const packageDir = fileURLToPath(new URL("..", import.meta.url));
const lockPath = join(packageDir, "generator.lock.json");
const configPath = join(packageDir, "openapi-ts.config.mjs");
const scratchPath = join(packageDir, ".drift");
const generator = "@hey-api/openapi-ts";

/** SHA-256 of a text file with line endings normalized, so checkouts agree. */
function textSha256(path) {
  const text = readFileSync(path, "utf8").replace(/\r\n?/g, "\n");
  return createHash("sha256").update(text).digest("hex");
}

function installedVersion() {
  const require = createRequire(import.meta.url);
  return JSON.parse(readFileSync(require.resolve(`${generator}/package.json`), "utf8")).version;
}

function currentInputs() {
  return {
    generator,
    version: installedVersion(),
    spec_sha256: textSha256(specPath),
    config_sha256: textSha256(configPath),
  };
}

/** Relative path → contents for every file under `dir`. */
function readTree(dir) {
  const files = new Map();
  for (const entry of readdirSync(dir, { recursive: true, withFileTypes: true })) {
    if (entry.isFile()) {
      const path = join(entry.parentPath, entry.name);
      files.set(relative(dir, path), readFileSync(path, "utf8"));
    }
  }
  return files;
}

function treeDifferences(expected, actual) {
  const names = new Set([...expected.keys(), ...actual.keys()]);
  return [...names].filter((name) => expected.get(name) !== actual.get(name)).sort();
}

const inputs = currentInputs();

if (process.argv.includes("--update")) {
  writeFileSync(lockPath, `${JSON.stringify(inputs, null, 2)}\n`);
  console.log(`wrote ${relative(process.cwd(), lockPath)}`);
  process.exit(0);
}

const failures = [];
const locked = existsSync(lockPath) ? JSON.parse(readFileSync(lockPath, "utf8")) : {};
for (const [key, value] of Object.entries(inputs)) {
  if (locked[key] !== value) {
    failures.push(`${key}: locked ${JSON.stringify(locked[key])}, now ${JSON.stringify(value)}`);
  }
}
if (failures.length > 0) {
  failures.push(
    "the generated client would change: review it (`pnpm --filter @ariadshift/sdk generate`, " +
      "then typecheck and test), and run `pnpm --filter @ariadshift/sdk lock`",
  );
}

rmSync(scratchPath, { recursive: true, force: true });
try {
  await generate(scratchPath);
  if (existsSync(outputPath)) {
    const stale = treeDifferences(readTree(scratchPath), readTree(outputPath));
    if (stale.length > 0) {
      failures.push(
        `src/generated/ differs from a fresh generation (${stale.join(", ")}): ` +
          "run `pnpm --filter @ariadshift/sdk generate`",
      );
    }
  }
} finally {
  rmSync(scratchPath, { recursive: true, force: true });
}

if (failures.length > 0) {
  for (const failure of failures) {
    console.error(`sdk drift: ${failure}`);
  }
  process.exit(1);
}
console.log("sdk drift: generated client matches generator.lock.json");
