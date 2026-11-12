import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { flatten, readCatalogue } from "./i18n-lib.mjs";
import {
  buildCatalog,
  convertMessage,
  DEFAULT_OUTPUT,
  generate,
  parseArgumentTypes,
  serializeCatalog,
} from "./i18n-xcstrings.mjs";

const SCRIPT = path.join(import.meta.dirname, "i18n-xcstrings.mjs");
const MAC_DIR = path.resolve(import.meta.dirname, "../apps/macos");

const convert = (message, { order = [], types = new Map(), state = "translated" } = {}) =>
  Object.fromEntries(convertMessage(message, { order, types, state }));

/** Writes a small locales directory and returns its path. */
function fixtureDir(files) {
  const dir = mkdtempSync(path.join(os.tmpdir(), "xcstrings-"));
  for (const [name, content] of Object.entries(files)) {
    writeFileSync(path.join(dir, name), typeof content === "string" ? content : JSON.stringify(content));
  }
  return dir;
}

const TOML = `
[locale.en]
name = "English"
dir = "ltr"
status = "source"

[locale.vi]
name = "Tiếng Việt"
dir = "ltr"
status = "reviewed"

[locale.de]
name = "Deutsch"
dir = "ltr"
status = "community"
`;

test("argument types are read from the translator note", () => {
  assert.deepEqual([...parseArgumentTypes("Count of files. types: count=int, name=string")], [
    ["count", "int"],
    ["name", "string"],
  ]);
  assert.equal(parseArgumentTypes("no declaration").size, 0);
  assert.throws(() => parseArgumentTypes("types: n=float"), /int or string/);
});

test("arguments become positional specifiers by first use", () => {
  const result = convert("{name} has {size} bytes", { order: ["name", "size"], types: new Map([["size", "int"]]) });
  assert.equal(result[""].stringUnit.value, "%1$@ has %2$lld bytes");
  // A translation may reorder the arguments; numbering stays tied to the English order.
  const reordered = convert("{size} byte từ {name}", { order: ["name", "size"], types: new Map([["size", "int"]]) });
  assert.equal(reordered[""].stringUnit.value, "%2$lld byte từ %1$@");
});

test("a literal percent sign is escaped", () => {
  assert.equal(convert("100% done")[""].stringUnit.value, "100%% done");
});

test("plural becomes variations with the text around it copied into every option", () => {
  const result = convert("{count, plural, =0 {No files} one {# file} other {# files}} in {folder}", {
    order: ["count", "folder"],
  });
  assert.deepEqual(Object.keys(result), [""]);
  const plural = result[""].variations.plural;
  assert.deepEqual(Object.keys(plural).sort(), ["one", "other", "zero"]);
  assert.equal(plural.zero.stringUnit.value, "No files in %2$@");
  assert.equal(plural.one.stringUnit.value, "%1$lld file in %2$@");
  assert.equal(plural.other.stringUnit.value, "%1$lld files in %2$@");
});

test("only categories present in the translation are emitted", () => {
  const result = convert("{count, plural, other {# cảnh báo}}", { order: ["count"] });
  assert.deepEqual(Object.keys(result[""].variations.plural), ["other"]);
});

test("select becomes one key per branch", () => {
  const result = convert("{kind, select, pdf {A PDF} other {A document}}");
  assert.deepEqual(Object.keys(result).sort(), [".other", ".pdf"]);
  assert.equal(result[".pdf"].stringUnit.value, "A PDF");
  assert.equal(result[".other"].stringUnit.value, "A document");
});

test("select branches keep their arguments, and a plural may sit inside a branch", () => {
  const result = convert(
    "{kind, select, missing {{field} is required.} other {{count, plural, one {# item} other {# items}} left}}",
    { order: ["field", "count"] },
  );
  assert.equal(result[".missing"].stringUnit.value, "%1$@ is required.");
  assert.equal(result[".other"].variations.plural.one.stringUnit.value, "%2$lld item left");
});

test("two selects join their branches in order", () => {
  const result = convert("{a, select, x {X} other {O}}-{b, select, y {Y} other {P}}");
  assert.deepEqual(Object.keys(result).sort(), [".other.other", ".other.y", ".x.other", ".x.y"]);
  assert.equal(result[".x.y"].stringUnit.value, "X-Y");
  assert.equal(result[".other.other"].stringUnit.value, "O-P");
});

test("state follows the locale status and unsupported ICU is rejected", () => {
  assert.equal(convert("x", { state: "needs_review" })[""].stringUnit.state, "needs_review");
  assert.throws(() => convert("{n, plural, =2 {two} other {#}}", { order: ["n"] }), /"=2" cannot be converted/);
  assert.throws(() => convert("{n, plural, offset:1 other {#}}", { order: ["n"] }), /offset/);
  assert.throws(() => convert("{n, selectordinal, other {#}}", { order: ["n"] }), /selectordinal/);
  assert.throws(
    () => convert("{a, plural, other {# a}} {b, plural, other {# b}}", { order: ["a", "b"] }),
    /more than one plural/,
  );
  assert.throws(() => convert("see <b>this</b>"), /web\.\* only/);
});

test("catalogue keeps common, mac and legal, drops web, and maps state", () => {
  const dir = fixtureDir({
    "locales.toml": TOML,
    "en.json": {
      common: { ok: "OK" },
      mac: { open: "Open", "open@note": "Menu item.", files: "{n, plural, one {# file} other {# files}}" },
      web: { only: "Web only" },
      legal: { notice: "Notice" },
    },
    "vi.json": { common: { ok: "Đồng ý" }, mac: { open: "Mở" }, legal: { notice: "Thông báo" } },
    "de.json": { common: { ok: "OK (de)" }, web: { only: "Nur Web" } },
  });
  try {
    const catalog = buildCatalog(dir);
    assert.equal(catalog.sourceLanguage, "en");
    assert.deepEqual(Object.keys(catalog.strings).sort(), ["common.ok", "legal.notice", "mac.files", "mac.open"]);
    const open = catalog.strings["mac.open"];
    assert.equal(open.comment, "Menu item.");
    assert.deepEqual(Object.keys(open.localizations).sort(), ["en", "vi"], "untranslated locales are omitted");
    assert.equal(open.localizations.vi.stringUnit.state, "translated");
    assert.equal(catalog.strings["common.ok"].localizations.de.stringUnit.state, "needs_review");
    assert.equal(catalog.strings["mac.files"].localizations.en.variations.plural.one.stringUnit.value, "%1$lld file");
    assert.equal(catalog.strings["legal.notice"].localizations.de, undefined);
  } finally {
    rmSync(dir, { recursive: true });
  }
});

test("a note that declares an unknown argument is an error", () => {
  const dir = fixtureDir({
    "locales.toml": TOML,
    "en.json": { mac: { a: "Hello {name}", "a@note": "types: other=int" } },
    "vi.json": {},
    "de.json": {},
  });
  try {
    assert.throws(() => buildCatalog(dir), /mac\.a: the note declares a type for \{other\}/);
  } finally {
    rmSync(dir, { recursive: true });
  }
});

test("serialisation is Xcode's layout with sorted keys and a trailing newline", () => {
  const text = serializeCatalog({ version: "1.0", sourceLanguage: "en", strings: { b: { x: "1" }, a: { x: "2" } } });
  assert.equal(
    text,
    `{\n  "sourceLanguage" : "en",\n  "strings" : {\n    "a" : {\n      "x" : "2"\n    },\n    "b" : {\n      "x" : "1"\n    }\n  },\n  "version" : "1.0"\n}\n`,
  );
});

test("a second run is byte-identical and --check detects drift", () => {
  const dir = fixtureDir({
    "locales.toml": TOML,
    "en.json": { mac: { open: "Open" } },
    "vi.json": { mac: { open: "Mở" } },
    "de.json": {},
  });
  const out = path.join(dir, "out", "Localizable.xcstrings");
  try {
    generate({ dir, out });
    const first = readFileSync(out);
    generate({ dir, out });
    assert.ok(first.equals(readFileSync(out)), "rerun changed the bytes");
    assert.equal(generate({ dir, out, check: true }).changed, false);

    writeFileSync(path.join(dir, "en.json"), JSON.stringify({ mac: { open: "Open file" } }));
    assert.equal(generate({ dir, out, check: true }).changed, true);
    const cli = (...args) =>
      execFileSync("node", [SCRIPT, "--dir", dir, "--out", out, ...args], { encoding: "utf8", stdio: "pipe" });
    assert.throws(() => cli("--check"), /out of date/);
    cli();
    assert.match(cli("--check"), /up to date/);
  } finally {
    rmSync(dir, { recursive: true });
  }
});

test("the committed Localizable.xcstrings equals a fresh generation", () => {
  const { changed } = generate({ check: true });
  assert.equal(changed, false, "run: node scripts/i18n-xcstrings.mjs");
  assert.ok(statSync(DEFAULT_OUTPUT).size > 0);
});

test("the real catalogue converts and is deterministic", () => {
  assert.equal(serializeCatalog(buildCatalog()), serializeCatalog(buildCatalog()));
});

/** Every `"mac.…"` string literal in the app's Swift sources. */
function macKeysInSwift() {
  const keys = new Set();
  const visit = (dir) => {
    for (const name of readdirSync(dir)) {
      const file = path.join(dir, name);
      if (statSync(file).isDirectory()) {
        visit(file);
      } else if (name.endsWith(".swift")) {
        for (const match of readFileSync(file, "utf8").matchAll(/"(mac\.[a-z0-9_.]+)"/g)) {
          keys.add(match[1]);
        }
      }
    }
  };
  visit(path.join(MAC_DIR, "AriadShift"));
  return keys;
}

test("every mac key used in Swift exists, and every mac key is used", () => {
  const source = flatten(readCatalogue("en")).messages;
  const macKeys = [...source.keys()].filter((key) => key.startsWith("mac."));
  const used = macKeysInSwift();
  const catalog = buildCatalog().strings;
  for (const key of used) {
    assert.ok(key in catalog || macKeys.includes(key), `Swift uses ${key}, which locales/en.json does not define`);
  }
  for (const key of macKeys) {
    assert.ok(used.has(key), `${key} is defined in locales/en.json but no Swift source uses it`);
  }
});

test("a select key lists the same branches in every locale that translates it", () => {
  const catalog = buildCatalog().strings;
  for (const key of ["mac.tools.group", "mac.inspector.field", "mac.menu.zoom", "mac.tools.disabled"]) {
    const branches = Object.keys(catalog).filter((name) => name.startsWith(`${key}.`));
    assert.ok(branches.length >= 3, `${key} has branches`);
    const expected = Object.keys(catalog[branches[0]].localizations).sort();
    assert.ok(expected.includes("en") && expected.includes("vi"), `${key} is translated in en and vi`);
    for (const branch of branches) {
      assert.deepEqual(Object.keys(catalog[branch].localizations).sort(), expected, branch);
    }
  }
});
