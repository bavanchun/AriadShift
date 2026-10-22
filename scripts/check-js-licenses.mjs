// Fails when a production dependency of the JavaScript workspace has a license outside
// the allow list. Production dependencies end up in the website, so AGENTS.md's licensing
// rules (no AGPL, no non-OSI code) apply to them as to the Rust crates in deny.toml.
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const ALLOWED = new Set([
  "0BSD",
  "Apache-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "BlueOak-1.0.0",
  "ISC",
  "MIT",
  "Unicode-3.0",
  "Zlib",
]);

// Licenses accepted for one package only, like the per-crate exceptions in deny.toml.
const EXCEPTIONS = new Map([
  // Browser support data that Next.js reads at build time.
  ["caniuse-lite", new Set(["CC-BY-4.0"])],
  // Plus Jakarta Sans, the brand typeface.
  ["@fontsource-variable/plus-jakarta-sans", new Set(["OFL-1.1"])],
  ["@fontsource/plus-jakarta-sans", new Set(["OFL-1.1"])],
]);

/** Splits an SPDX expression into identifiers, parentheses and operators. */
function tokenize(expression) {
  return expression.match(/\(|\)|[^\s()]+/g) ?? [];
}

/**
 * Evaluates an SPDX expression: AND binds tighter than OR, parentheses group, and an
 * identifier is accepted when `accepts` says so. Malformed expressions are rejected.
 */
export function satisfies(expression, accepts) {
  const tokens = tokenize(expression);
  let index = 0;
  const peek = () => tokens[index];
  const next = () => tokens[index++];

  function term() {
    const token = next();
    if (token === "(") {
      const value = or();
      if (next() !== ")") {
        throw new Error("unbalanced parentheses");
      }
      return value;
    }
    if (token === undefined || token === ")" || token === "AND" || token === "OR") {
      throw new Error(`unexpected ${token ?? "end of expression"}`);
    }
    if (peek() === "WITH") {
      const exception = next() && next();
      return accepts(`${token} WITH ${exception}`);
    }
    return accepts(token);
  }

  function and() {
    let value = term();
    while (peek() === "AND") {
      next();
      value = term() && value;
    }
    return value;
  }

  function or() {
    let value = and();
    while (peek() === "OR") {
      next();
      value = and() || value;
    }
    return value;
  }

  try {
    const value = or();
    return index === tokens.length && value;
  } catch {
    return false;
  }
}

function check() {
  const output = execFileSync("pnpm", ["-r", "licenses", "list", "--prod", "--json"], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    shell: process.platform === "win32",
  });
  const byLicense = JSON.parse(output);

  const rejected = [];
  let count = 0;
  for (const [license, packages] of Object.entries(byLicense)) {
    for (const entry of packages) {
      count += 1;
      const extra = EXCEPTIONS.get(entry.name);
      const accepts = (id) => ALLOWED.has(id) || (extra?.has(id) ?? false);
      if (!satisfies(license, accepts)) {
        rejected.push(`${entry.name}@${entry.versions.join(", ")}: ${license}`);
      }
    }
  }

  if (rejected.length > 0) {
    console.error("Production dependencies with licenses outside the allow list:");
    for (const line of rejected) {
      console.error(`  ${line}`);
    }
    process.exit(1);
  }
  console.log(`${count} production JavaScript dependencies use allowed licenses.`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  check();
}
