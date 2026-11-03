// Keep generated license notices reproducible when upstream texts contain CRLF or trailing spaces.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const inventory = execFileSync("cargo", ["about", "generate", "about.hbs"], {
  encoding: "utf8",
  maxBuffer: 32 * 1024 * 1024,
});
process.stdout.write(inventory.replace(/\r\n?/g, "\n").replace(/[\t ]+$/gm, ""));
process.stdout.write("\n\nBundled OCR font\n----------------\n");
process.stdout.write(readFileSync("crates/ariad-pdf/assets/glyphless-LICENSE.txt", "utf8"));
