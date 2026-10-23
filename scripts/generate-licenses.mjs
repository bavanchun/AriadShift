// Keep generated license notices reproducible when upstream texts contain CRLF or trailing spaces.
import { execFileSync } from "node:child_process";

const inventory = execFileSync("cargo", ["about", "generate", "about.hbs"], {
  encoding: "utf8",
  maxBuffer: 32 * 1024 * 1024,
});
process.stdout.write(inventory.replace(/\r\n?/g, "\n").replace(/[\t ]+$/gm, ""));
