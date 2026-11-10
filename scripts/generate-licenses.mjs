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

process.stdout.write("\n\nBundled compliance assets\n-------------------------\n");
process.stdout.write("Liberation 2.1.5 (OFL-1.1), unmodified font files.\n");
process.stdout.write(
  "Source and SHA-256 receipts: crates/ariad-pdf/assets/compliance-assets.toml\n",
);
process.stdout.write(
  readFileSync("crates/ariad-pdf/assets/liberation/LICENSE-liberation.txt", "utf8"),
);
process.stdout.write("\n\nCompact-ICC-Profiles sRGB-v4.icc (CC0-1.0), via krilla 0.8.2.\n");
process.stdout.write(readFileSync("crates/ariad-pdf/assets/LICENSE-sRGB-v4.txt", "utf8"));
