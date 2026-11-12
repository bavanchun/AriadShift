// Generates the macOS String Catalog (apps/macos/AriadShift/Localizable.xcstrings) from
// locales/*.json. Contract: docs/i18n.md, "macOS String Catalogs". The output is deterministic
// (sorted keys, fixed indentation, Xcode's own layout), so a second run changes nothing.
//
// Usage: node scripts/i18n-xcstrings.mjs [--check] [--dir <locales>] [--out <file>]
//   --check   write nothing; exit 1 when the committed catalogue differs from a fresh one.
//
// Conversion rules (see the contract for the reasoning):
//   keys       the dotted JSON path, unchanged; only common.*, mac.* and legal.* are kept.
//   arguments  {name} becomes a positional specifier numbered by first use in the English
//              message: %N$@ for text, %N$lld for numbers. A plural argument is always a
//              number; any other argument is text unless the English `<key>@note` declares
//              it, for example `types: count=int, size=int`.
//   plural     one `variations.plural` entry per option; `#` is the numeric specifier. The
//              text around the plural is copied into every option. `=0` maps to `zero`;
//              other exact matches, offsets and selectordinal are rejected.
//   select     a String Catalog has no select, so each branch becomes its own key,
//              `<key>.<branch>` (several selects join as `<key>.<first>.<second>`); the Swift
//              call site picks the branch.
//   state      source and reviewed locales are `translated`; community (machine draft)
//              locales are `needs_review`. Untranslated keys are omitted.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { TYPE } from "@formatjs/icu-messageformat-parser";
import {
  catalogueTags,
  flatten,
  LOCALES_DIR,
  parseMessage,
  readCatalogue,
  readLocaleMetadata,
  SOURCE_LOCALE,
} from "./i18n-lib.mjs";

/** Namespaces delivered to macOS. `web.*` is dropped. */
export const MAC_NAMESPACES = ["common", "mac", "legal"];
export const DEFAULT_OUTPUT = path.resolve(
  import.meta.dirname,
  "../apps/macos/AriadShift/Localizable.xcstrings",
);
const PLURAL_CATEGORIES = new Set(["zero", "one", "two", "few", "many", "other"]);
const NOTE_SUFFIX = "@note";

// ---------------------------------------------------------------------------------------
// ICU to String Catalog
// ---------------------------------------------------------------------------------------

/** `types: count=int, name=string` inside a translator note. */
export function parseArgumentTypes(note) {
  const types = new Map();
  const match = /\btypes:\s*([^\n]*)/i.exec(note ?? "");
  if (!match) {
    return types;
  }
  for (const part of match[1].split(",")) {
    const pair = /^\s*([a-z][a-z0-9_]*)\s*=\s*(\w+)\s*\.?\s*$/.exec(part);
    if (!pair) {
      continue;
    }
    if (!["int", "string"].includes(pair[2])) {
      throw new Error(`argument type "${pair[2]}" for {${pair[1]}} must be int or string`);
    }
    types.set(pair[1], pair[2]);
  }
  return types;
}

/** Names of the arguments that become format specifiers, in order of first use. */
function collectArguments(elements, order = []) {
  for (const element of elements) {
    switch (element.type) {
      case TYPE.argument:
        if (!order.includes(element.value)) {
          order.push(element.value);
        }
        break;
      case TYPE.plural:
        if (!order.includes(element.value)) {
          order.push(element.value);
        }
        for (const option of Object.values(element.options)) {
          collectArguments(option.value, order);
        }
        break;
      case TYPE.select:
        for (const option of Object.values(element.options)) {
          collectArguments(option.value, order);
        }
        break;
      default:
    }
  }
  return order;
}

/**
 * Resolves every select into its branches. Returns `[{ branches: string[], elements }]` where
 * `elements` has no select left outside plural options (those are rejected later).
 */
function expandSelects(elements) {
  let variants = [{ branches: [], elements: [] }];
  for (const element of elements) {
    const choices =
      element.type === TYPE.select
        ? Object.entries(element.options).flatMap(([branch, option]) =>
            expandSelects(option.value).map((inner) => ({
              branches: [branch, ...inner.branches],
              elements: inner.elements,
            })),
          )
        : [{ branches: [], elements: [element] }];
    variants = variants.flatMap((variant) =>
      choices.map((choice) => ({
        branches: [...variant.branches, ...choice.branches],
        elements: [...variant.elements, ...choice.elements],
      })),
    );
  }
  return variants;
}

function specifier(name, order, types, plurals) {
  const position = order.indexOf(name) + 1;
  const numeric = plurals.has(name) || types.get(name) === "int";
  return `%${position}$${numeric ? "lld" : "@"}`;
}

function renderElements(elements, context, pluralName) {
  let text = "";
  for (const element of elements) {
    switch (element.type) {
      case TYPE.literal:
        text += element.value.replaceAll("%", "%%");
        break;
      case TYPE.argument:
        text += specifier(element.value, context.order, context.types, context.plurals);
        break;
      case TYPE.pound:
        text += specifier(pluralName, context.order, context.types, context.plurals);
        break;
      case TYPE.tag:
        throw new Error("rich-text tags are allowed in web.* only");
      default:
        throw new Error(`unsupported ICU element type ${element.type}`);
    }
  }
  return text;
}

/** One localisation (a stringUnit or plural variations) for a select-free message. */
function localizeVariant(elements, context, state) {
  const pluralIndexes = elements
    .map((element, index) => (element.type === TYPE.plural ? index : -1))
    .filter((index) => index !== -1);
  if (pluralIndexes.length === 0) {
    return { stringUnit: { state, value: renderElements(elements, context, null) } };
  }
  const names = new Set(pluralIndexes.map((index) => elements[index].value));
  if (pluralIndexes.length > 1 || names.size > 1) {
    throw new Error("more than one plural in a message cannot be converted");
  }
  const index = pluralIndexes[0];
  const plural = elements[index];
  if (plural.pluralType === "ordinal") {
    throw new Error("selectordinal cannot be converted");
  }
  if (plural.offset) {
    throw new Error("a plural offset cannot be converted");
  }
  const before = elements.slice(0, index);
  const after = elements.slice(index + 1);
  const variations = {};
  for (const [selector, option] of Object.entries(plural.options)) {
    const category = selector === "=0" ? "zero" : selector;
    if (!PLURAL_CATEGORIES.has(category)) {
      throw new Error(`plural option "${selector}" cannot be converted`);
    }
    for (const inner of option.value) {
      if (inner.type === TYPE.plural || inner.type === TYPE.select) {
        throw new Error("nested plural/select blocks inside a plural cannot be converted");
      }
    }
    variations[category] = {
      stringUnit: {
        state,
        value: renderElements([...before, ...option.value, ...after], context, plural.value),
      },
    };
  }
  return { variations: { plural: variations } };
}

/**
 * Converts one ICU message into catalogue localisations.
 * @returns {Map<string, object>} branch suffix ("" or ".pdf" ...) to localisation
 */
export function convertMessage(message, { order, types, state }) {
  const variants = expandSelects(parseMessage(message));
  const plurals = new Set();
  const scan = (elements) => {
    for (const element of elements) {
      if (element.type === TYPE.plural) {
        plurals.add(element.value);
      }
    }
  };
  for (const variant of variants) {
    scan(variant.elements);
  }
  const context = { order, types, plurals };
  const result = new Map();
  for (const variant of variants) {
    const suffix = variant.branches.map((branch) => `.${branch}`).join("");
    result.set(suffix, localizeVariant(variant.elements, context, state));
  }
  return result;
}

// ---------------------------------------------------------------------------------------
// Catalogue
// ---------------------------------------------------------------------------------------

const namespaceOf = (key) => key.split(".")[0];

/** Builds the catalogue object (not yet serialised) from a locales directory. */
export function buildCatalog(dir = LOCALES_DIR) {
  const metadata = readLocaleMetadata(dir);
  const source = flatten(readCatalogue(SOURCE_LOCALE, dir));
  const sourceKeys = [...source.messages.keys()].filter((key) =>
    MAC_NAMESPACES.includes(namespaceOf(key)),
  );

  /** Per source key: argument order, declared types, translator note. */
  const keyInfo = new Map();
  for (const key of sourceKeys) {
    try {
      const ast = parseMessage(source.messages.get(key));
      const note = source.notes.get(`${key}${NOTE_SUFFIX}`);
      const types = parseArgumentTypes(note);
      const order = collectArguments(ast);
      for (const name of types.keys()) {
        if (!order.includes(name)) {
          throw new Error(`the note declares a type for {${name}}, which the message does not use`);
        }
      }
      keyInfo.set(key, { order, types, note });
    } catch (error) {
      throw new Error(`${SOURCE_LOCALE}.json ${key}: ${error.message}`);
    }
  }

  const strings = {};
  const tags = [SOURCE_LOCALE, ...catalogueTags(dir).filter((tag) => tag !== SOURCE_LOCALE)];
  for (const tag of tags) {
    const entry = metadata[tag];
    if (!entry) {
      throw new Error(`${tag}.json has no entry in locales.toml`);
    }
    const state = entry.status === "community" ? "needs_review" : "translated";
    const messages =
      tag === SOURCE_LOCALE ? source.messages : flatten(readCatalogue(tag, dir)).messages;
    for (const key of sourceKeys) {
      const message = messages.get(key);
      if (message === undefined || message === "") {
        continue;
      }
      const { order, types, note } = keyInfo.get(key);
      let converted;
      try {
        converted = convertMessage(message, { order, types, state });
      } catch (error) {
        throw new Error(`${tag}.json ${key}: ${error.message}`);
      }
      for (const [suffix, localization] of converted) {
        const full = `${key}${suffix}`;
        const record = (strings[full] ??= { extractionState: "manual", localizations: {} });
        if (note && record.comment === undefined) {
          record.comment = note;
        }
        record.localizations[tag] = localization;
      }
    }
  }
  return { sourceLanguage: SOURCE_LOCALE, strings, version: "1.0" };
}

/** Xcode's own JSON layout: sorted keys, two-space indent, `"key" : value`. */
export function serializeCatalog(catalog) {
  const write = (value, depth) => {
    if (value === null || typeof value !== "object") {
      return JSON.stringify(value);
    }
    const indent = "  ".repeat(depth + 1);
    const keys = Object.keys(value).sort();
    if (keys.length === 0) {
      return "{\n" + "  ".repeat(depth) + "}";
    }
    const body = keys.map((key) => `${indent}${JSON.stringify(key)} : ${write(value[key], depth + 1)}`);
    return `{\n${body.join(",\n")}\n${"  ".repeat(depth)}}`;
  };
  return `${write(catalog, 0)}\n`;
}

export function generate({ dir = LOCALES_DIR, out = DEFAULT_OUTPUT, check = false } = {}) {
  const text = serializeCatalog(buildCatalog(dir));
  if (check) {
    const current = existsSync(out) ? readFileSync(out, "utf8") : null;
    return { changed: current !== text, text };
  }
  mkdirSync(path.dirname(out), { recursive: true });
  writeFileSync(out, text);
  return { changed: false, text };
}

// ---------------------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------------------

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = process.argv.slice(2);
  const value = (flag) => (args.includes(flag) ? args[args.indexOf(flag) + 1] : undefined);
  const dir = value("--dir") ?? LOCALES_DIR;
  const out = value("--out") ?? DEFAULT_OUTPUT;
  try {
    const { changed } = generate({ dir, out, check: args.includes("--check") });
    if (args.includes("--check")) {
      if (changed) {
        console.error(
          `error: ${path.relative(process.cwd(), out)} is out of date; run node scripts/i18n-xcstrings.mjs`,
        );
        process.exit(1);
      }
      console.log("Localizable.xcstrings is up to date.");
    } else {
      console.log(`wrote ${path.relative(process.cwd(), out)}`);
    }
  } catch (error) {
    console.error(`error: ${error.message}`);
    process.exit(1);
  }
}
