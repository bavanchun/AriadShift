// Catalogue gate for locales/: valid ICU, argument and tag parity with en.json, no stray
// keys, the portable ICU subset in the shared namespaces, metadata parity with
// locales.toml and the completeness thresholds. Rules: docs/i18n.md, "CI checks".
// Usage: node scripts/i18n-check.mjs [--report] [--dir <locales directory>]
import { checkCatalogues, isPickerVisible, LOCALES_DIR, readLocaleMetadata } from "./i18n-lib.mjs";

const args = process.argv.slice(2);
const dirFlag = args.indexOf("--dir");
const dir = dirFlag === -1 ? LOCALES_DIR : args[dirFlag + 1];

const { errors, report } = checkCatalogues(dir);

if (args.includes("--report")) {
  const metadata = readLocaleMetadata(dir);
  for (const row of report) {
    const entry = metadata[row.tag];
    const picker = isPickerVisible(entry, row.ratio) ? "picker" : "hidden";
    console.log(
      `${row.tag.padEnd(9)} ${row.status.padEnd(9)} ${String(row.translated).padStart(4)}/${String(row.total).padEnd(4)} ${(row.ratio * 100).toFixed(1).padStart(5)}%  ${picker}`,
    );
  }
}

if (errors.length > 0) {
  console.error(errors.map((error) => `error: ${error}`).join("\n"));
  console.error(`\n${errors.length} i18n problem${errors.length === 1 ? "" : "s"} found.`);
  process.exit(1);
}
console.log(`i18n check passed for ${report.length} locales.`);
