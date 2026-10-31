// Collect notices from the installed desktop production graph, including shared UI assets.
import { existsSync, readdirSync, readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { isAllowedLicense } from "./check-js-licenses.mjs";

const root = fileURLToPath(new URL("..", import.meta.url));
const seen = new Set();
const notices = new Map();

function dependency(directory, name) {
  for (let current = directory; ; current = path.dirname(current)) {
    const candidate = path.join(current, "node_modules", name);
    if (existsSync(path.join(candidate, "package.json"))) return realpathSync(candidate);
    if (path.dirname(current) === current) throw new Error(`Missing dependency ${name}`);
  }
}

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
process.stdout.write(`${output.replace(/\r\n?/g, "\n").replace(/[\t ]+$/gm, "")}\n`);
