"use client";

import { useCallback, useEffect, useState } from "react";

export type ThemePreference = "light" | "dark" | "system";

/** localStorage key holding the explicit theme choice. */
export const THEME_STORAGE_KEY = "ariadshift-theme";

function systemPrefersDark() {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

function readPreference(): ThemePreference {
  try {
    const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
    return stored === "light" || stored === "dark" ? stored : "system";
  } catch {
    return "system";
  }
}

/** Applies a preference to `<html>` by toggling the `dark` class. */
export function applyTheme(preference: ThemePreference) {
  const dark = preference === "dark" || (preference === "system" && systemPrefersDark());
  document.documentElement.classList.toggle("dark", dark);
}

/** Theme preference persisted in localStorage, following the system when unset. */
export function useTheme() {
  const [preference, setPreferenceState] = useState<ThemePreference>("system");

  useEffect(() => {
    setPreferenceState(readPreference());
  }, []);

  useEffect(() => {
    applyTheme(preference);
    if (preference !== "system") {
      return;
    }
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const follow = () => applyTheme("system");
    query.addEventListener("change", follow);
    return () => query.removeEventListener("change", follow);
  }, [preference]);

  const setPreference = useCallback((next: ThemePreference) => {
    try {
      if (next === "system") {
        window.localStorage.removeItem(THEME_STORAGE_KEY);
      } else {
        window.localStorage.setItem(THEME_STORAGE_KEY, next);
      }
    } catch {
      // Storage can be unavailable (private mode); the choice then lasts for this page.
    }
    setPreferenceState(next);
  }, []);

  return { preference, setPreference };
}
