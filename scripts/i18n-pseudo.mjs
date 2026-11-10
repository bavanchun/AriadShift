// Writes the two test-only pseudo-locales (docs/i18n.md, "Pseudo-locales") from
// locales/en.json into a cache folder. They are never committed, never listed in
// locales.toml and never shipped. Usage: node scripts/i18n-pseudo.mjs [--out <directory>]
import { mkdirSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { PSEUDO_LOCALES, pseudoLocalize, readCatalogue } from "./i18n-lib.mjs";

const flag = process.argv.indexOf("--out");
const out =
  flag === -1
    ? path.join(os.tmpdir(), "ariadshift-i18n-pseudo")
    : path.resolve(process.argv[flag + 1]);
mkdirSync(out, { recursive: true });
const source = readCatalogue("en");
for (const tag of Object.keys(PSEUDO_LOCALES)) {
  const file = path.join(out, `${tag}.json`);
  writeFileSync(file, `${JSON.stringify(pseudoLocalize(source, tag), null, 2)}\n`);
  console.log(file);
}
