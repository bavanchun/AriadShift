"use client";

import { MonitorIcon, MoonIcon, SunIcon } from "lucide-react";
import { Button } from "#components/button";
import { type ThemePreference, useTheme } from "#hooks/use-theme";

const NEXT: Record<ThemePreference, ThemePreference> = {
  system: "light",
  light: "dark",
  dark: "system",
};

const LABEL: Record<ThemePreference, string> = {
  system: "Theme: system",
  light: "Theme: light",
  dark: "Theme: dark",
};

/** Cycles the theme between system, light and dark. */
export function ThemeToggle({ className }: { className?: string }) {
  const { preference, setPreference } = useTheme();
  const Icon = preference === "light" ? SunIcon : preference === "dark" ? MoonIcon : MonitorIcon;
  return (
    <Button
      variant="ghost"
      size="icon"
      className={className}
      aria-label={`${LABEL[preference]}. Switch to ${NEXT[preference]}.`}
      title={LABEL[preference]}
      onClick={() => setPreference(NEXT[preference])}
    >
      <Icon aria-hidden="true" />
    </Button>
  );
}
