// Shared i18n logic for the catalogue checker (scripts/i18n-check.mjs), the pseudo-locale
// generator (scripts/i18n-pseudo.mjs) and the web app (apps/web/src/i18n). Everything here is
// plain Node: it reads locales/ from the repository and has no framework dependency. The
// contract it enforces is docs/i18n.md.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { parse, TYPE } from "@formatjs/icu-messageformat-parser";

// A bundler may not define `import.meta.dirname`; the web app always passes its own directory.
export const LOCALES_DIR = import.meta.dirname
  ? path.resolve(import.meta.dirname, "../locales")
  : "";
export const SOURCE_LOCALE = "en";
/** Namespaces that must stay inside the ICU subset the macOS String Catalog can express. */
export const PORTABLE_NAMESPACES = ["common", "mac", "legal"];
/** Namespaces counted by the community completeness threshold. */
export const COUNTED_NAMESPACES = ["common", "web", "mac"];
export const COMMUNITY_THRESHOLD = 0.8;
export const NOTE_SUFFIX = "@note";

// ---------------------------------------------------------------------------------------
// locales.toml: the small TOML subset the metadata file uses (tables, strings, booleans).
// ---------------------------------------------------------------------------------------

function parseTomlValue(raw, line) {
  const value = raw.trim();
  if (/^"(?:[^"\\]|\\.)*"$/.test(value)) {
    return JSON.parse(value);
  }
  if (value === "true" || value === "false") {
    return value === "true";
  }
  throw new Error(`locales.toml line ${line}: unsupported value ${value}`);
}

/**
 * @typedef {object} LocaleEntry
 * @property {string} name native name shown in the language picker
 * @property {"ltr" | "rtl"} dir
 * @property {"source" | "reviewed" | "community"} status
 * @property {boolean} [rtl_checked] a native reader has reviewed the right-to-left layout
 * @property {boolean} [overlay] holds only overrides of another locale, so completeness is not measured
 */

/**
 * Parses the metadata file into `{ <tag>: LocaleEntry }`.
 * @param {string} text
 * @returns {Record<string, LocaleEntry>}
 */
export function parseLocalesToml(text) {
  const locales = {};
  let current = null;
  text.split(/\r?\n/).forEach((rawLine, index) => {
    const line = rawLine.replace(/^\s+|\s+$/g, "");
    if (line === "" || line.startsWith("#")) {
      return;
    }
    const table = /^\[locale\.(?:"([^"]+)"|([A-Za-z0-9_-]+))\]$/.exec(line);
    if (table) {
      current = table[1] ?? table[2];
      if (Object.hasOwn(locales, current)) {
        throw new Error(`locales.toml line ${index + 1}: duplicate locale ${current}`);
      }
      locales[current] = {};
      return;
    }
    const pair = /^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.+)$/.exec(line);
    if (!pair || current === null) {
      throw new Error(`locales.toml line ${index + 1}: cannot parse "${line}"`);
    }
    locales[current][pair[1]] = parseTomlValue(pair[2], index + 1);
  });
  return locales;
}

/** @returns {Record<string, LocaleEntry>} */
export function readLocaleMetadata(dir = LOCALES_DIR) {
  return parseLocalesToml(readFileSync(path.join(dir, "locales.toml"), "utf8"));
}

/** Locale tags in the order of locales.toml, with the source locale first. */
export function localeTags(metadata) {
  return Object.keys(metadata);
}

// ---------------------------------------------------------------------------------------
// Catalogues: nested JSON of ICU messages.
// ---------------------------------------------------------------------------------------

export function readCatalogue(tag, dir = LOCALES_DIR) {
  const file = path.join(dir, `${tag}.json`);
  if (!existsSync(file)) {
    return {};
  }
  return JSON.parse(readFileSync(file, "utf8"));
}

/** Tags that have a catalogue file, found by listing the directory. */
export function catalogueTags(dir = LOCALES_DIR) {
  return readdirSync(dir)
    .filter((name) => name.endsWith(".json"))
    .map((name) => name.slice(0, -".json".length))
    .sort();
}

/**
 * Flattens a nested catalogue into dotted keys. Translator notes (`<key>@note`) are returned
 * separately because they are context for Weblate, not messages.
 * @returns {{ messages: Map<string, string>, notes: Map<string, string>, problems: string[] }}
 */
export function flatten(catalogue) {
  const messages = new Map();
  const notes = new Map();
  const problems = [];
  const visit = (node, prefix) => {
    for (const [key, value] of Object.entries(node)) {
      const full = prefix ? `${prefix}.${key}` : key;
      const name = key.endsWith(NOTE_SUFFIX) ? key.slice(0, -NOTE_SUFFIX.length) : key;
      if (!/^[a-z][a-z0-9_]*$/.test(name)) {
        problems.push(`${full}: key segments are lowercase letters, digits and underscores`);
      } else if (key.endsWith(NOTE_SUFFIX)) {
        notes.set(full, String(value));
      } else if (typeof value === "string") {
        messages.set(full, value);
      } else if (value !== null && typeof value === "object" && !Array.isArray(value)) {
        visit(value, full);
      } else {
        problems.push(`${full}: a message must be a string or an object`);
      }
    }
  };
  visit(catalogue, "");
  return { messages, notes, problems };
}

/** Rebuilds the nested object for a flat key map. */
export function nest(flat) {
  const root = {};
  for (const [key, value] of flat) {
    const parts = key.split(".");
    let node = root;
    for (const part of parts.slice(0, -1)) {
      node = node[part] ??= {};
    }
    node[parts.at(-1)] = value;
  }
  return root;
}

/** Messages of a locale layered over the source catalogue, without translator notes. */
export function resolveMessages(tag, dir = LOCALES_DIR) {
  const source = flatten(readCatalogue(SOURCE_LOCALE, dir)).messages;
  if (tag === SOURCE_LOCALE) {
    return nest(source);
  }
  const merged = new Map(source);
  for (const [key, value] of flatten(readCatalogue(tag, dir)).messages) {
    if (value !== "") {
      merged.set(key, value);
    }
  }
  return nest(merged);
}

// ---------------------------------------------------------------------------------------
// ICU analysis
// ---------------------------------------------------------------------------------------

/** Parses with the same strictness for every check. Throws on invalid ICU. */
export function parseMessage(message) {
  return parse(message, { requiresOtherClause: true });
}

/**
 * Summarises a parsed message: the arguments with their kinds, the plural and select
 * options, the rich-text tags and the deepest nesting of plural/select blocks.
 */
export function describeMessage(ast) {
  const args = new Map();
  const plurals = new Map();
  const selects = new Map();
  const tags = new Set();
  const problems = [];
  let maxDepth = 0;

  const note = (name, kind) => {
    const kinds = args.get(name) ?? new Set();
    kinds.add(kind);
    args.set(name, kinds);
  };

  const walk = (elements, depth) => {
    for (const element of elements) {
      switch (element.type) {
        case TYPE.argument:
          note(element.value, "argument");
          break;
        case TYPE.number:
        case TYPE.date:
        case TYPE.time:
          note(
            element.value,
            { [TYPE.number]: "number", [TYPE.date]: "date", [TYPE.time]: "time" }[element.type],
          );
          problems.push(`{${element.value}} uses a number, date or time style`);
          break;
        case TYPE.plural: {
          const kind = element.pluralType === "ordinal" ? "selectordinal" : "plural";
          note(element.value, kind);
          plurals.set(element.value, new Set(Object.keys(element.options)));
          maxDepth = Math.max(maxDepth, depth + 1);
          if (kind === "selectordinal") {
            problems.push(`{${element.value}} uses selectordinal`);
          }
          for (const option of Object.values(element.options)) {
            walk(option.value, depth + 1);
          }
          break;
        }
        case TYPE.select:
          note(element.value, "select");
          selects.set(element.value, new Set(Object.keys(element.options)));
          maxDepth = Math.max(maxDepth, depth + 1);
          for (const option of Object.values(element.options)) {
            walk(option.value, depth + 1);
          }
          break;
        case TYPE.tag:
          tags.add(element.value);
          walk(element.children, depth);
          break;
        default:
      }
    }
  };
  walk(ast, 0);
  if (maxDepth > 2) {
    problems.push("plural/select blocks nest more than one level deep");
  }
  return { args, plurals, selects, tags, problems, maxDepth };
}

const sameSet = (left, right) =>
  left.size === right.size && [...left].every((value) => right.has(value));

function kindsOf(args) {
  return new Map([...args].map(([name, kinds]) => [name, [...kinds].sort().join("|")]));
}

/** CLDR plural categories of a locale, for example ["one", "few", "many", "other"] in `ru`. */
export function pluralCategories(tag) {
  return new Set(new Intl.PluralRules(tag).resolvedOptions().pluralCategories);
}

/**
 * Compares a translation with its English source and returns human-readable problems.
 * Plural categories follow the target language; select options and tags must match.
 */
export function compareWithSource(source, translation, tag) {
  const problems = [];
  const sourceArgs = kindsOf(source.args);
  const translatedArgs = kindsOf(translation.args);
  for (const [name, kind] of sourceArgs) {
    if (!translatedArgs.has(name)) {
      problems.push(`argument {${name}} is missing`);
    } else if (translatedArgs.get(name) !== kind) {
      problems.push(`argument {${name}} changed from ${kind} to ${translatedArgs.get(name)}`);
    }
  }
  for (const name of translatedArgs.keys()) {
    if (!sourceArgs.has(name)) {
      problems.push(`argument {${name}} does not exist in the English source`);
    }
  }
  const required = pluralCategories(tag);
  for (const [name, options] of translation.plurals) {
    if (!options.has("other")) {
      problems.push(`plural {${name}} has no "other" branch`);
    }
    for (const category of required) {
      if (!options.has(category)) {
        problems.push(`plural {${name}} is missing the "${category}" branch that ${tag} uses`);
      }
    }
    for (const option of options) {
      if (!option.startsWith("=") && !required.has(option)) {
        problems.push(`plural {${name}} has "${option}", which ${tag} does not use`);
      }
    }
    const exact = (set) => new Set([...set].filter((option) => option.startsWith("=")));
    const expected = exact(source.plurals.get(name) ?? new Set());
    if (source.plurals.has(name) && !sameSet(exact(options), expected)) {
      problems.push(`plural {${name}} must keep the exact branches ${[...expected].join(", ")}`);
    }
  }
  for (const [name, options] of translation.selects) {
    const expected = source.selects.get(name);
    if (expected && !sameSet(options, expected)) {
      problems.push(`select {${name}} must keep the options ${[...expected].join(", ")}`);
    }
  }
  if (!sameSet(translation.tags, source.tags)) {
    problems.push(
      `tags differ: expected ${[...source.tags].map((tagName) => `<${tagName}>`).join(" ") || "none"}`,
    );
  }
  return problems;
}

// ---------------------------------------------------------------------------------------
// Catalogue checks (docs/i18n.md "CI checks")
// ---------------------------------------------------------------------------------------

const namespaceOf = (key) => key.slice(0, key.indexOf("."));

/**
 * Runs every catalogue check and returns `{ errors, report }`. Nothing is printed or thrown,
 * so tests can assert on the result and the CLI can format it.
 * @param {string} dir directory holding locales.toml and the <tag>.json files
 */
export function checkCatalogues(dir = LOCALES_DIR) {
  const errors = [];
  const report = [];
  let metadata;
  try {
    metadata = readLocaleMetadata(dir);
  } catch (error) {
    return { errors: [`locales.toml: ${error.message}`], report };
  }

  // Metadata parity: every file has an entry and every entry has a file.
  const files = new Set(catalogueTags(dir));
  for (const tag of files) {
    if (!(tag in metadata)) {
      errors.push(`${tag}.json has no entry in locales.toml`);
    }
  }
  for (const tag of Object.keys(metadata)) {
    if (!files.has(tag)) {
      errors.push(`locales.toml lists ${tag} but ${tag}.json does not exist`);
    }
  }
  if (metadata[SOURCE_LOCALE]?.status !== "source") {
    errors.push(`locales.toml: ${SOURCE_LOCALE} must have status "source"`);
  }
  for (const [tag, entry] of Object.entries(metadata)) {
    const where = `locales.toml [${tag}]`;
    try {
      if (new Intl.Locale(tag).toString() !== tag) {
        errors.push(`${where}: the tag is not in canonical BCP 47 casing`);
      }
    } catch {
      errors.push(`${where}: the tag is not valid BCP 47`);
    }
    if (typeof entry.name !== "string" || entry.name === "") {
      errors.push(`${where}: name is required`);
    }
    if (!["ltr", "rtl"].includes(entry.dir)) {
      errors.push(`${where}: dir must be "ltr" or "rtl"`);
    }
    if (!["source", "reviewed", "community"].includes(entry.status)) {
      errors.push(`${where}: status must be source, reviewed or community`);
    }
    if (tag !== SOURCE_LOCALE && entry.status === "source") {
      errors.push(`${where}: only ${SOURCE_LOCALE} can be the source`);
    }
    if (entry.dir === "rtl" && typeof entry.rtl_checked !== "boolean") {
      errors.push(`${where}: an rtl locale needs rtl_checked = true or false`);
    }
    if (entry.dir !== "rtl" && "rtl_checked" in entry) {
      errors.push(`${where}: rtl_checked only applies to rtl locales`);
    }
    const known = new Set(["name", "dir", "status", "rtl_checked", "overlay"]);
    for (const key of Object.keys(entry)) {
      if (!known.has(key)) {
        errors.push(`${where}: unknown key ${key}`);
      }
    }
  }
  if (!files.has(SOURCE_LOCALE)) {
    return { errors, report };
  }

  const source = flatten(readCatalogue(SOURCE_LOCALE, dir));
  errors.push(...source.problems.map((problem) => `en.json ${problem}`));
  const sourceInfo = new Map();
  for (const [key, message] of source.messages) {
    if (!["common", "web", "mac", "legal"].includes(namespaceOf(key))) {
      errors.push(`en.json ${key}: the first key segment must be common, web, mac or legal`);
    }
    try {
      sourceInfo.set(key, describeMessage(parseMessage(message)));
    } catch (error) {
      errors.push(`en.json ${key}: invalid ICU message (${error.message})`);
    }
  }

  for (const tag of files) {
    if (!(tag in metadata)) {
      continue;
    }
    const { messages, problems } = flatten(readCatalogue(tag, dir));
    const label = `${tag}.json`;
    errors.push(...problems.map((problem) => `${label} ${problem}`));
    const { status, overlay } = metadata[tag];
    for (const [key, message] of messages) {
      let info;
      try {
        info = describeMessage(parseMessage(message));
      } catch (error) {
        errors.push(`${label} ${key}: invalid ICU message (${error.message})`);
        continue;
      }
      if (PORTABLE_NAMESPACES.includes(namespaceOf(key))) {
        if (info.tags.size > 0) {
          errors.push(`${label} ${key}: rich-text tags are allowed in web.* only`);
        }
        for (const problem of info.problems) {
          errors.push(`${label} ${key}: ${problem}, which ${namespaceOf(key)}.* does not allow`);
        }
      }
      if (tag === SOURCE_LOCALE) {
        continue;
      }
      if (!source.messages.has(key)) {
        errors.push(`${label} ${key}: the key does not exist in en.json`);
        continue;
      }
      if (namespaceOf(key) === "legal" && status === "community") {
        errors.push(`${label} ${key}: legal.* is never machine-translated`);
      }
      const expected = sourceInfo.get(key);
      if (!expected) {
        continue;
      }
      for (const problem of compareWithSource(expected, info, tag)) {
        errors.push(`${label} ${key}: ${problem}`);
      }
    }

    // Completeness.
    const completeness = measureCompleteness(source.messages, messages, status);
    report.push({ tag, status, overlay: overlay === true, ...completeness });
    if (tag === SOURCE_LOCALE || overlay === true) {
      continue;
    }
    if (status === "reviewed" && completeness.missing.length > 0) {
      errors.push(
        `${label}: reviewed locale is ${percent(completeness.ratio)} complete; missing ${preview(completeness.missing)}`,
      );
    } else if (status === "community" && completeness.ratio < COMMUNITY_THRESHOLD) {
      errors.push(
        `${label}: community locale is ${percent(completeness.ratio)} complete; at least ${percent(COMMUNITY_THRESHOLD)} is required`,
      );
    }
  }
  return { errors, report };
}

const percent = (ratio) => `${(ratio * 100).toFixed(1)}%`;
const preview = (keys) =>
  keys.length > 5 ? `${keys.slice(0, 5).join(", ")} and ${keys.length - 5} more` : keys.join(", ");

/**
 * Fraction of source keys a locale translates. Reviewed locales are measured over every
 * namespace. Community locales are measured over common, web and mac, because legal text
 * stays English for them.
 */
export function measureCompleteness(sourceMessages, messages, status) {
  const counted = [...sourceMessages.keys()].filter(
    (key) => status !== "community" || COUNTED_NAMESPACES.includes(namespaceOf(key)),
  );
  const missing = counted.filter((key) => (messages.get(key) ?? "") === "");
  return {
    total: counted.length,
    translated: counted.length - missing.length,
    missing,
    ratio: counted.length === 0 ? 1 : (counted.length - missing.length) / counted.length,
  };
}

/**
 * Whether a locale appears in the language picker: reviewed and source locales are always
 * listed, community locales need the completeness threshold, and a right-to-left community
 * locale additionally needs a native reader's sign-off (`rtl_checked`).
 */
export function isPickerVisible(entry, ratio) {
  if (entry.status !== "community") {
    return true;
  }
  if (entry.dir === "rtl" && entry.rtl_checked !== true) {
    return false;
  }
  return entry.overlay === true || ratio >= COMMUNITY_THRESHOLD;
}

// ---------------------------------------------------------------------------------------
// Pseudo-locales (docs/i18n.md "Pseudo-locales"): generated at test time, never committed.
// ---------------------------------------------------------------------------------------

const ACCENTS = {
  a: "á",
  b: "ƀ",
  c: "ç",
  d: "ð",
  e: "é",
  f: "ƒ",
  g: "ĝ",
  h: "ĥ",
  i: "í",
  j: "ĵ",
  k: "ķ",
  l: "ļ",
  m: "ɱ",
  n: "ñ",
  o: "ö",
  p: "þ",
  q: "ɋ",
  r: "ŕ",
  s: "š",
  t: "ţ",
  u: "û",
  v: "ṽ",
  w: "ŵ",
  x: "ẋ",
  y: "ý",
  z: "ž",
  A: "Á",
  B: "Ɓ",
  C: "Ç",
  D: "Ð",
  E: "É",
  F: "Ƒ",
  G: "Ĝ",
  H: "Ĥ",
  I: "Í",
  J: "Ĵ",
  K: "Ķ",
  L: "Ļ",
  M: "Ṁ",
  N: "Ñ",
  O: "Ö",
  P: "Þ",
  Q: "Ɋ",
  R: "Ŕ",
  S: "Š",
  T: "Ţ",
  U: "Û",
  V: "Ṽ",
  W: "Ŵ",
  X: "Ẋ",
  Y: "Ý",
  Z: "Ž",
};

/** Tags of the two generated locales and their text direction. */
export const PSEUDO_LOCALES = { "en-XA": "ltr", "ar-XB": "rtl" };

const accent = (text) => text.replace(/[A-Za-z]/g, (letter) => ACCENTS[letter]);
const RLE = "‫";
const PDF = "‬";

function mapLiterals(elements, transform) {
  return elements.map((element) => {
    switch (element.type) {
      case TYPE.literal:
        return { ...element, value: transform(element.value) };
      case TYPE.tag:
        return { ...element, children: mapLiterals(element.children, transform) };
      case TYPE.plural:
      case TYPE.select:
        return {
          ...element,
          options: Object.fromEntries(
            Object.entries(element.options).map(([key, option]) => [
              key,
              { value: mapLiterals(option.value, transform) },
            ]),
          ),
        };
      default:
        return element;
    }
  });
}

function printAst(elements, inPlural = false) {
  return elements
    .map((element) => {
      switch (element.type) {
        case TYPE.literal:
          return element.value
            .replace(/'/g, "''")
            .replace(/[{}<]/g, (char) => `'${char}'`)
            .replace(/#/g, inPlural ? "'#'" : "#");
        case TYPE.argument:
          return `{${element.value}}`;
        case TYPE.pound:
          return "#";
        case TYPE.tag:
          return `<${element.value}>${printAst(element.children, inPlural)}</${element.value}>`;
        case TYPE.plural:
        case TYPE.select: {
          const keyword =
            element.type === TYPE.select
              ? "select"
              : element.pluralType === "ordinal"
                ? "selectordinal"
                : "plural";
          const offset = element.offset ? ` offset:${element.offset}` : "";
          const options = Object.entries(element.options)
            .map(
              ([key, option]) => `${key} {${printAst(option.value, element.type === TYPE.plural)}}`,
            )
            .join(" ");
          return `{${element.value}, ${keyword},${offset} ${options}}`;
        }
        default:
          throw new Error("pseudo-localisation supports plain arguments, plural, select and tags");
      }
    })
    .join("");
}

const literalLength = (elements) =>
  elements.reduce((sum, element) => {
    if (element.type === TYPE.literal) {
      return sum + element.value.length;
    }
    if (element.type === TYPE.tag) {
      return sum + literalLength(element.children);
    }
    if (element.type === TYPE.plural || element.type === TYPE.select) {
      return (
        sum +
        Math.max(0, ...Object.values(element.options).map((option) => literalLength(option.value)))
      );
    }
    return sum;
  }, 0);

/** Pseudo-localises one ICU message; arguments, plural branches and tags are kept intact. */
export function pseudoLocalizeMessage(message, tag) {
  const ast = parseMessage(message);
  if (tag === "en-XA") {
    const padding = "~".repeat(Math.ceil(literalLength(ast) * 0.4));
    return `[${printAst(mapLiterals(ast, accent))}${padding}]`;
  }
  if (tag === "ar-XB") {
    const reversed = mapLiterals(ast, (text) => {
      const lead = /^\s*/.exec(text)[0];
      const trail = /\s*$/.exec(text)[0];
      const words = text.trim().split(/\s+/).filter(Boolean).reverse().join(" ");
      return `${lead}${words}${words === "" ? "" : trail}`;
    });
    return `${RLE}${printAst(reversed)}${PDF}`;
  }
  throw new Error(`unknown pseudo-locale ${tag}`);
}

/** Pseudo-localises a nested catalogue. */
export function pseudoLocalize(catalogue, tag) {
  const { messages } = flatten(catalogue);
  return nest(
    new Map([...messages].map(([key, value]) => [key, pseudoLocalizeMessage(value, tag)])),
  );
}
