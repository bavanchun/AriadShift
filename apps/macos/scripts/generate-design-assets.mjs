#!/usr/bin/env node
// Generates the macOS asset catalogs from their sources. Never edit the outputs by hand.
//
//   apps/macos/AriadDesignSystem/tokens.json  ->  AriadDesignSystem/Sources/AriadDesignSystem/Resources/Colors.xcassets
//                                                 (light, dark and increased-contrast color sets)
//   tokens.json appAccentColor               ->  AriadShift/Assets.xcassets/AccentColor.colorset
//   brand/png/ariadshift-app-icon-macos-*.png ->  AriadShift/Assets.xcassets/AppIcon.appiconset
//
// Usage: node apps/macos/scripts/generate-design-assets.mjs [--check]
// --check writes nothing and exits 1 when a committed output differs from what the sources produce.
// The brand PNGs come from `pnpm --dir brand build`; this script only copies them, so it needs no dependencies.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const MACOS = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const REPO = path.resolve(MACOS, "../..");
const TOKENS = path.join(MACOS, "AriadDesignSystem/tokens.json");
const COLOR_CATALOG = path.join(
  MACOS,
  "AriadDesignSystem/Sources/AriadDesignSystem/Resources/Colors.xcassets",
);
const APP_CATALOG = path.join(MACOS, "AriadShift/Assets.xcassets");
const ICON_SOURCE = (size) => path.join(REPO, "brand/png", `ariadshift-app-icon-macos-${size}.png`);

// The macOS app icon set: point size, scale and the rendered pixel size that fills it.
const ICON_SLOTS = [16, 32, 128, 256, 512].flatMap((points) => [
  { points, scale: 1, pixels: points },
  { points, scale: 2, pixels: points * 2 },
]);

const INFO = { author: "xcode", version: 1 };
const json = (value) => `${JSON.stringify(value, null, 2)}\n`;

/** Parses "#RRGGBB" or "#RRGGBB@alpha" into an asset-catalog sRGB color. */
function parseColor(token, name) {
  const match = /^#([0-9A-Fa-f]{2})([0-9A-Fa-f]{2})([0-9A-Fa-f]{2})(?:@(0|1|0?\.\d+|1\.0+))?$/.exec(
    token,
  );
  if (!match) throw new Error(`tokens.json: color "${name}" has an invalid value "${token}"`);
  const [, red, green, blue, alpha = "1"] = match;
  const hex = (pair) => `0x${pair.toUpperCase()}`;
  return {
    "color-space": "srgb",
    components: {
      alpha: Number(alpha).toFixed(3),
      blue: hex(blue),
      green: hex(green),
      red: hex(red),
    },
  };
}

/** A color set with the base (light), dark, and increased-contrast light and dark appearances. */
function colorSet(name, spec) {
  for (const key of ["light", "dark"]) {
    if (typeof spec[key] !== "string")
      throw new Error(`tokens.json: color "${name}" needs a "${key}" value`);
  }
  const luminosity = (value) => ({ appearance: "luminosity", value });
  const contrast = { appearance: "contrast", value: "high" };
  const entry = (value, appearances) => ({
    ...(appearances.length ? { appearances } : {}),
    color: parseColor(value, name),
    idiom: "universal",
  });
  return {
    colors: [
      entry(spec.light, []),
      entry(spec.dark, [luminosity("dark")]),
      entry(spec.lightHighContrast ?? spec.light, [contrast]),
      entry(spec.darkHighContrast ?? spec.dark, [luminosity("dark"), contrast]),
    ],
    info: INFO,
  };
}

/** Every generated file as { absolute path: Buffer }. */
function buildOutputs() {
  const tokens = JSON.parse(fs.readFileSync(TOKENS, "utf8"));
  const out = new Map();
  const put = (file, content) =>
    out.set(file, Buffer.isBuffer(content) ? content : Buffer.from(content));

  put(path.join(COLOR_CATALOG, "Contents.json"), json({ info: INFO }));
  for (const [name, spec] of Object.entries(tokens.colors)) {
    if (!/^[a-z][A-Za-z0-9]*$/.test(name))
      throw new Error(`tokens.json: color name "${name}" must be lowerCamelCase`);
    put(path.join(COLOR_CATALOG, `${name}.colorset`, "Contents.json"), json(colorSet(name, spec)));
  }

  const accent = tokens.colors[tokens.appAccentColor];
  if (!accent)
    throw new Error(`tokens.json: appAccentColor "${tokens.appAccentColor}" is not a color`);
  put(path.join(APP_CATALOG, "Contents.json"), json({ info: INFO }));
  put(
    path.join(APP_CATALOG, "AccentColor.colorset", "Contents.json"),
    json(colorSet(tokens.appAccentColor, accent)),
  );

  const images = ICON_SLOTS.map(({ points, scale, pixels }) => {
    const filename = `icon_${points}x${points}${scale === 2 ? "@2x" : ""}.png`;
    const source = ICON_SOURCE(pixels);
    if (!fs.existsSync(source))
      throw new Error(`missing ${path.relative(REPO, source)}; run pnpm --dir brand build`);
    put(path.join(APP_CATALOG, "AppIcon.appiconset", filename), fs.readFileSync(source));
    return { filename, idiom: "mac", scale: `${scale}x`, size: `${points}x${points}` };
  });
  put(path.join(APP_CATALOG, "AppIcon.appiconset", "Contents.json"), json({ images, info: INFO }));
  return out;
}

function listFiles(dir) {
  if (!fs.existsSync(dir)) return [];
  return fs
    .readdirSync(dir, { withFileTypes: true, recursive: true })
    .filter((entry) => entry.isFile() && entry.name !== ".DS_Store")
    .map((entry) => path.join(entry.parentPath, entry.name));
}

const outputs = buildOutputs();
const roots = [COLOR_CATALOG, APP_CATALOG];
const rel = (file) => path.relative(REPO, file);

if (process.argv.includes("--check")) {
  const problems = [];
  for (const [file, content] of outputs) {
    if (!fs.existsSync(file)) problems.push(`missing   ${rel(file)}`);
    else if (!fs.readFileSync(file).equals(content)) problems.push(`differs   ${rel(file)}`);
  }
  for (const file of roots.flatMap(listFiles)) {
    if (!outputs.has(file)) problems.push(`unexpected ${rel(file)}`);
  }
  if (problems.length) {
    console.error("Generated macOS assets are out of date:");
    for (const line of problems) console.error(`  ${line}`);
    console.error("Run: node apps/macos/scripts/generate-design-assets.mjs");
    process.exit(1);
  }
  console.log(`macOS assets are up to date (${outputs.size} files).`);
} else {
  for (const root of roots) fs.rmSync(root, { recursive: true, force: true });
  for (const [file, content] of outputs) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, content);
  }
  console.log(`Wrote ${outputs.size} files into ${roots.map(rel).join(" and ")}.`);
}
