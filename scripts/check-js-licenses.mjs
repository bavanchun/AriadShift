// Fails when a production dependency of the JavaScript workspace has a license outside
// the allow list. Production dependencies end up in the website, so AGENTS.md's licensing
// rules (no AGPL, no non-OSI code) apply to them as to the Rust crates in deny.toml.
import { execFileSync } from "node:child_process";

const ALLOWED = new Set([
  "0BSD",
  "Apache-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "BlueOak-1.0.0",
  // Browser support data that Next.js reads at build time.
  "CC-BY-4.0",
  "CC0-1.0",
  "ISC",
  "MIT",
  // Plus Jakarta Sans, the brand typeface.
  "OFL-1.1",
  "Unicode-3.0",
  "Zlib",
]);

/** Accepts an SPDX expression when one OR branch has only allowed AND terms. */
function allowed(expression) {
  return expression
    .replace(/[()]/g, "")
    .split(/\s+OR\s+/)
    .some((branch) => branch.split(/\s+AND\s+/).every((term) => ALLOWED.has(term.trim())));
}

const output = execFileSync("pnpm", ["-r", "licenses", "list", "--prod", "--json"], {
  encoding: "utf8",
  shell: process.platform === "win32",
});
const byLicense = JSON.parse(output);

const rejected = Object.entries(byLicense)
  .filter(([license]) => !allowed(license))
  .flatMap(([license, packages]) =>
    packages.map((entry) => `${entry.name}@${entry.versions.join(", ")}: ${license}`),
  );

if (rejected.length > 0) {
  console.error("Production dependencies with licenses outside the allow list:");
  for (const line of rejected) {
    console.error(`  ${line}`);
  }
  process.exit(1);
}
const count = Object.values(byLicense).reduce((sum, packages) => sum + packages.length, 0);
console.log(`${count} production JavaScript dependencies use allowed licenses.`);
