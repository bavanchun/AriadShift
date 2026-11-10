// Server-only message loading. A locale's messages are layered over English, so a key that
// is not translated yet shows English instead of a raw key. Client components receive only
// the namespaces their route needs (`scopeMessages`), which keeps the per-locale message
// payload a small part of each page's first load.
import {
  PSEUDO_LOCALES,
  pseudoLocalize,
  readCatalogue,
  resolveMessages,
  SOURCE_LOCALE,
} from "../../../../scripts/i18n-lib.mjs";
import { localesDirectory } from "./locales";

export type Messages = { [key: string]: string | Messages };

const cache = new Map<string, Messages>();

/** Messages of a locale, layered over English, without translator notes. */
export function loadMessages(locale: string): Messages {
  const cached = cache.get(locale);
  if (cached) {
    return cached;
  }
  const dir = localesDirectory();
  const messages = (
    locale in PSEUDO_LOCALES
      ? pseudoLocalize(readCatalogue(SOURCE_LOCALE, dir), locale)
      : resolveMessages(locale, dir)
  ) as Messages;
  cache.set(locale, messages);
  return messages;
}

/**
 * Copies the named subtrees, for example `["common", "web.converter"]`, so a route
 * serialises only what its client components render.
 */
export function scopeMessages(messages: Messages, namespaces: readonly string[]): Messages {
  const scoped: Messages = {};
  for (const namespace of namespaces) {
    const parts = namespace.split(".");
    let from: string | Messages | undefined = messages;
    let to: Messages = scoped;
    for (const [index, part] of parts.entries()) {
      from = typeof from === "object" ? from[part] : undefined;
      if (from === undefined) {
        break;
      }
      if (index === parts.length - 1) {
        to[part] = from;
      } else {
        const next = to[part];
        to = to[part] = typeof next === "object" ? next : {};
      }
    }
  }
  return scoped;
}
