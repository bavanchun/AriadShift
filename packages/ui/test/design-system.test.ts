import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const componentsDir = join(__dirname, "../src/components");
const components = readdirSync(componentsDir).filter((file) => file.endsWith(".tsx"));
const source = (file: string) => readFileSync(join(componentsDir, file), "utf8");

const NEW_COMPONENTS = [
  "dialog",
  "popover",
  "tooltip",
  "tabs",
  "toggle-group",
  "slider",
  "command",
  "toast",
  "progress",
  "skeleton",
  "page-thumbnail",
  "drop-overlay",
  "kbd",
  "dev-showcase",
];

/** Physical-edge utilities that break in RTL. Logical `ms-`, `ps-`, `start-`, `text-start` are fine. */
const PHYSICAL =
  /(?:^|[\s"'`:!([])-?(?:left|right|inset-l|inset-r)-|(?:^|[\s"'`:!([])-?(?:m|p)[lr]-[\d[a-z]|text-(?:left|right)\b|border-[lr](?:-|\s|"|')|rounded-(?:l|r|tl|tr|bl|br)(?:-|\s|"|')|(?:^|[\s"'`:])(?:float|clear)-(?:left|right)\b|scroll-[mp][lr]-/;

describe("RTL logical-class audit", () => {
  it.each(components)("%s uses no physical left/right utilities", (file) => {
    const code = source(file)
      .split("\n")
      .filter((line) => !line.trim().startsWith("//") && !line.trim().startsWith("*"))
      .join("\n");
    const hits = code.match(new RegExp(PHYSICAL, "g"));
    expect(hits, `${file}: ${hits?.join(", ")}`).toBeNull();
  });

  it("catches physical utilities (guards the audit itself)", () => {
    for (const bad of [
      "ml-2",
      "p-2 pr-3",
      "text-left",
      "left-0",
      "sm:right-2",
      "rounded-l-md",
      "border-r",
      "-ml-1",
      "has-x:pl-2",
    ]) {
      expect(PHYSICAL.test(` ${bad} `), bad).toBe(true);
    }
    for (const good of [
      "ms-2",
      "pe-3",
      "text-start",
      "start-0",
      "end-2",
      "rounded-s-md",
      "border-e",
      "-ms-1",
      "inset-s-0",
    ]) {
      expect(PHYSICAL.test(` ${good} `), good).toBe(false);
    }
  });
});

describe("interaction and motion rules", () => {
  it.each(["tabs", "toggle-group", "slider", "command", "page-thumbnail"])(
    "%s styles :focus-visible",
    (name) => {
      expect(source(`${name}.tsx`)).toMatch(/focus-visible/);
    },
  );

  it.each([
    "dialog",
    "popover",
    "tooltip",
    "tabs",
    "toggle-group",
    "slider",
    "progress",
    "skeleton",
    "toast",
    "page-thumbnail",
    "drop-overlay",
  ])("%s has a reduced-motion fallback", (name) => {
    expect(source(`${name}.tsx`)).toMatch(/motion-reduce:/);
  });

  it("marks client components that use state or effects", () => {
    for (const name of NEW_COMPONENTS.filter((n) => !["skeleton", "drop-overlay"].includes(n))) {
      expect(source(`${name}.tsx`).startsWith('"use client";'), name).toBe(true);
    }
  });

  it("ships every requested component file", () => {
    for (const name of NEW_COMPONENTS) {
      expect(components, name).toContain(`${name}.tsx`);
    }
  });
});

describe("design tokens", () => {
  const css = readFileSync(join(__dirname, "../src/styles/globals.css"), "utf8");
  const block = (selector: string) => {
    const start = css.indexOf(`${selector} {`);
    return css.slice(start, css.indexOf("\n}\n", start));
  };
  const names = (text: string) =>
    new Set(Array.from(text.matchAll(/^\s+(--[\w-]+):/gm), (m) => m[1]));
  const light = names(block(":root"));
  const dark = names(block(".dark"));

  it("defines a dark value for every themed token", () => {
    const missing = [...light].filter((name) => !dark.has(name) && name !== "--radius");
    expect(missing).toEqual([]);
  });

  it("covers the v2 palette, status, elevation and focus tokens", () => {
    for (const name of [
      "--bg-canvas",
      "--bg-surface",
      "--crimson",
      "--thread",
      "--success",
      "--warning",
      "--info",
      "--destructive",
      "--focus-ring",
      "--elevation-1",
      "--elevation-2",
      "--elevation-3",
      "--border",
    ]) {
      expect(light.has(name), name).toBe(true);
    }
    expect(block(":root")).toContain("#faf7f2");
    expect(block(".dark")).toContain("#0f0e0d");
    expect(block(":root")).toContain("#c8322b");
    expect(block(":root")).toContain("#d4940a");
  });

  it("keeps the shipped brand tokens", () => {
    for (const name of ["--background", "--primary", "--gold", "--gold-soft", "--sage", "--ring"]) {
      expect(light.has(name), name).toBe(true);
    }
  });

  it("exposes the tokens to Tailwind and defines each animation with its keyframes", () => {
    for (const name of [
      "--color-bg-canvas",
      "--color-thread",
      "--shadow-elevation-2",
      "--color-warning",
    ]) {
      expect(css).toContain(`${name}:`);
    }
    for (const animation of [
      "shimmer",
      "dash-march",
      "indeterminate",
      "indeterminate-rtl",
      "thread-drain",
    ]) {
      expect(css).toContain(`--animate-${animation}:`);
    }
    for (const frames of [
      "ui-shimmer",
      "ui-dash-march",
      "ui-indeterminate",
      "ui-indeterminate-rtl",
      "ui-thread-drain",
    ]) {
      expect(css).toContain(`@keyframes ${frames}`);
    }
  });
});
