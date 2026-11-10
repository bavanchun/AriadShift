"use client";

import { useLocale } from "next-intl";
import { localePath } from "./paths";

/** Builds a link target inside the current locale, for example `path("/download")`. */
export function useLocalePath(): (path: string) => string {
  const locale = useLocale();
  return (path) => localePath(locale, path);
}
