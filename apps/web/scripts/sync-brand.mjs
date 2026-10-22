// Copies the generated brand assets the site serves. brand/ stays the only source;
// the copies are gitignored and refreshed before every dev, build and typecheck run.
import { copyFileSync, mkdirSync } from "node:fs";
import path from "node:path";

const web = path.join(import.meta.dirname, "..");
const brand = path.join(web, "../../brand");

const copies = [
  ["svg/favicon.svg", "src/app/icon.svg"],
  ["png/ariadshift-app-icon-256.png", "src/app/apple-icon.png"],
  ["svg/ariadshift-app-icon.svg", "public/brand/ariadshift-app-icon.svg"],
  ["svg/ariadshift-app-icon-dark.svg", "public/brand/ariadshift-app-icon-dark.svg"],
  ["svg/ariadshift-lockup.svg", "public/brand/ariadshift-lockup.svg"],
  ["svg/ariadshift-lockup-dark.svg", "public/brand/ariadshift-lockup-dark.svg"],
  ["svg/ariadshift-mark-mono.svg", "public/brand/ariadshift-mark-mono.svg"],
  ["png/ariadshift-app-icon-512.png", "public/brand/ariadshift-app-icon-512.png"],
];

for (const [from, to] of copies) {
  const target = path.join(web, to);
  mkdirSync(path.dirname(target), { recursive: true });
  copyFileSync(path.join(brand, from), target);
}
