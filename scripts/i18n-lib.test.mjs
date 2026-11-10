import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import {
  checkCatalogues,
  describeMessage,
  flatten,
  isPickerVisible,
  LOCALES_DIR,
  PSEUDO_LOCALES,
  parseLocalesToml,
  parseMessage,
  pseudoLocalize,
  pseudoLocalizeMessage,
  readCatalogue,
} from "./i18n-lib.mjs";

const shape = (message) => {
  const info = describeMessage(parseMessage(message));
  return {
    args: [...info.args].map(([name, kinds]) => [name, [...kinds].join("|")]).sort(),
    plurals: [...info.plurals].map(([name, options]) => [name, [...options].sort()]).sort(),
    selects: [...info.selects].map(([name, options]) => [name, [...options].sort()]).sort(),
    tags: [...info.tags].sort(),
  };
};

const MESSAGES = [
  "Hello {name}",
  "{count, plural, =0 {No files} one {# file} other {# files}} in {folder}",
  "{kind, select, pdf {A PDF} other {A document}} with {n, plural, one {# page} other {# pages}}",
  "Read the <b>privacy</b> page or <link>contact us</link>, it's {state}",
  "Braces '{' and apostrophes ''",
];

test("both pseudo-locales keep every argument, plural branch and tag", () => {
  for (const tag of Object.keys(PSEUDO_LOCALES)) {
    for (const message of MESSAGES) {
      const pseudo = pseudoLocalizeMessage(message, tag);
      assert.deepEqual(shape(pseudo), shape(message), `${tag}: ${pseudo}`);
    }
  }
});

test("en-XA accents letters, pads the text and wraps it in brackets", () => {
  const pseudo = pseudoLocalizeMessage("Convert {name} now", "en-XA");
  assert.match(pseudo, /^\[.*\{name\}.*~+\]$/s);
  assert.doesNotMatch(pseudo.replace("{name}", ""), /[A-Za-z]/);
});

test("ar-XB wraps in right-to-left marks and reverses the word order", () => {
  const pseudo = pseudoLocalizeMessage("one two three", "ar-XB");
  assert.equal(pseudo, "‫three two one‬");
});

test("the real source catalogue survives both transforms", () => {
  const source = readCatalogue("en");
  const original = flatten(source).messages;
  for (const tag of Object.keys(PSEUDO_LOCALES)) {
    const generated = flatten(pseudoLocalize(source, tag)).messages;
    assert.deepEqual([...generated.keys()], [...original.keys()]);
    for (const [key, message] of original) {
      assert.deepEqual(shape(generated.get(key)), shape(message), `${tag} ${key}`);
    }
  }
});

test("locales.toml parser reads tables, quoted tags, strings and booleans", () => {
  const parsed = parseLocalesToml(`# comment
[locale.ar]
name = "العربية"
dir = "rtl"
rtl_checked = false

[locale."pt-BR"]
name = "Português (Brasil)"
`);
  assert.deepEqual(parsed, {
    ar: { name: "العربية", dir: "rtl", rtl_checked: false },
    "pt-BR": { name: "Português (Brasil)" },
  });
  assert.throws(() => parseLocalesToml("[locale.en]\nname = 4"), /unsupported value/);
});

test("picker visibility follows status, completeness and the RTL sign-off", () => {
  assert.equal(isPickerVisible({ status: "reviewed", dir: "ltr" }, 1), true);
  assert.equal(isPickerVisible({ status: "community", dir: "ltr" }, 0.79), false);
  assert.equal(isPickerVisible({ status: "community", dir: "ltr" }, 0.8), true);
  assert.equal(isPickerVisible({ status: "community", dir: "rtl", rtl_checked: false }, 1), false);
  assert.equal(isPickerVisible({ status: "community", dir: "rtl", rtl_checked: true }, 1), true);
  assert.equal(isPickerVisible({ status: "community", dir: "ltr", overlay: true }, 0.01), true);
});

function fixture(files, toml) {
  const dir = mkdtempSync(path.join(os.tmpdir(), "i18n-fixture-"));
  mkdirSync(dir, { recursive: true });
  writeFileSync(path.join(dir, "locales.toml"), toml);
  for (const [tag, catalogue] of Object.entries(files)) {
    writeFileSync(path.join(dir, `${tag}.json`), JSON.stringify(catalogue));
  }
  return dir;
}

const TOML = `[locale.en]
name = "English"
dir = "ltr"
status = "source"

[locale.ru]
name = "Русский"
dir = "ltr"
status = "community"

[locale.vi]
name = "Tiếng Việt"
dir = "ltr"
status = "reviewed"
`;

const EN = {
  common: { files: "{count, plural, one {# file} other {# files}}", greet: "Hi {name}" },
  web: { note: "Read the <b>docs</b>" },
  legal: { terms: "Terms for {product}" },
};

function problems(files, toml = TOML) {
  const dir = fixture(files, toml);
  try {
    return checkCatalogues(dir).errors;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const RU_FILES = "{count, plural, one {# файл} few {# файла} many {# файлов} other {# файла}}";
const VI = {
  common: { files: "{count, plural, other {# tệp}}", greet: "Chào {name}" },
  web: { note: "Đọc <b>tài liệu</b>" },
  legal: { terms: "Điều khoản của {product}" },
};

test("a consistent set of catalogues passes", () => {
  assert.deepEqual(
    problems({
      en: EN,
      ru: {
        common: { files: RU_FILES, greet: "Привет, {name}" },
        web: { note: "Читайте <b>документацию</b>" },
      },
      vi: VI,
    }),
    [],
  );
});

test("detects invalid ICU, argument drift, stray keys and tag drift", () => {
  const errors = problems({
    en: EN,
    ru: {
      common: { files: "{count, plural, one {# файл}", greet: "Привет, {nam}", extra: "x" },
      web: { note: "Читайте документацию" },
    },
    vi: VI,
  });
  const text = errors.join("\n");
  assert.match(text, /ru\.json common\.files: invalid ICU message/);
  assert.match(text, /ru\.json common\.greet: argument \{name\} is missing/);
  assert.match(text, /ru\.json common\.greet: argument \{nam\} does not exist/);
  assert.match(text, /ru\.json common\.extra: the key does not exist in en\.json/);
  assert.match(text, /ru\.json web\.note: tags differ/);
});

test("plural categories follow the target language", () => {
  const errors = problems({
    en: EN,
    ru: {
      common: { files: "{count, plural, one {# файл} other {# файлов}}", greet: "Привет, {name}" },
      web: { note: "<b>x</b>" },
    },
    vi: VI,
  }).join("\n");
  assert.match(errors, /missing the "few" branch that ru uses/);
  assert.match(errors, /missing the "many" branch/);
});

test("the portable subset is enforced in common, mac and legal only", () => {
  const errors = problems({
    en: {
      common: {
        when: "Opened {date, date, short}",
        rich: "<b>x</b>",
        ord: "{n, selectordinal, other {#th}}",
      },
      web: { ok: "{n, number} and <b>x</b>" },
      legal: { terms: "ok" },
    },
    ru: { common: {}, web: {} },
    vi: {
      common: {
        when: "{date, date, short}",
        rich: "<b>x</b>",
        ord: "{n, selectordinal, other {#}}",
      },
      web: { ok: "{n, number} <b>x</b>" },
      legal: { terms: "ok" },
    },
  }).join("\n");
  assert.match(errors, /en\.json common\.when: .*number, date or time style/);
  assert.match(errors, /en\.json common\.rich: rich-text tags are allowed in web\.\* only/);
  assert.match(errors, /en\.json common\.ord: .*selectordinal/);
  assert.doesNotMatch(errors, /en\.json web\.ok/);
});

test("enforces completeness, legal English and metadata parity", () => {
  const errors = problems({
    en: EN,
    ru: { common: { files: RU_FILES }, legal: { terms: "Условия {product}" } },
    xx: { common: {} },
  }).join("\n");
  assert.match(errors, /ru\.json: community locale is 33\.3% complete/);
  assert.match(errors, /ru\.json legal\.terms: legal\.\* is never machine-translated/);
  assert.match(errors, /xx\.json has no entry in locales\.toml/);
  assert.match(errors, /locales\.toml lists vi but vi\.json does not exist/);
});

test("a reviewed locale must translate every key, legal included", () => {
  const errors = problems({
    en: EN,
    ru: { common: { files: RU_FILES, greet: "Привет, {name}" }, web: { note: "<b>x</b>" } },
    vi: { common: VI.common, web: VI.web },
  }).join("\n");
  assert.match(errors, /vi\.json: reviewed locale is 75\.0% complete; missing legal\.terms/);
});

test("the committed catalogues pass the gate", () => {
  assert.deepEqual(checkCatalogues(LOCALES_DIR).errors, []);
});
