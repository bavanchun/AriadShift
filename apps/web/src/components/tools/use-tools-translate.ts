"use client";
// Connects the tool hub and the command palette to the message catalogue (`web.tools`).
import { useTranslations } from "next-intl";
import { useCallback } from "react";
import type { ToolsTranslate } from "../../lib/tools";

/** `tools.hub.openPalette` -> `hub.open_palette`; catalogue keys are lowercase snake case. */
export function catalogueKey(key: string): string {
  const local = key.startsWith("tools.") ? key.slice("tools.".length) : key;
  return local.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`);
}

/**
 * A `ToolsTranslate` backed by `web.tools`. A key the locale does not define keeps the English
 * fallback the component passes, so a tool the registry adds later still renders.
 */
export function useToolsTranslate(): ToolsTranslate {
  const t = useTranslations("web.tools");
  return useCallback<ToolsTranslate>(
    (key, fallback, values) => {
      const local = catalogueKey(key);
      return t.has(local) ? t(local, values) : fallback;
    },
    [t],
  );
}
