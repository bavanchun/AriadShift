"use client";

import { LanguagesIcon } from "lucide-react";
import { usePathname } from "next/navigation";
import { localePath, splitLocalePath } from "@/i18n/paths";

export interface LanguageOption {
  tag: string;
  /** Native name. */
  name: string;
  /** Machine-drafted text that people may still correct. */
  community: boolean;
}

export interface LanguageSwitcherProps {
  current: string;
  locales: LanguageOption[];
  labels: { button: string; list: string; badge: string; note: string };
}

/**
 * Language picker: a disclosure of plain links, one per published locale. Plain links keep
 * the converter page's cross-origin isolation correct (a new document is loaded) and work
 * without scripting; a click keeps the query string and hash of the current page.
 */
export function LanguageSwitcher({ current, locales, labels }: LanguageSwitcherProps) {
  const pathname = usePathname();
  const { path } = splitLocalePath(pathname ?? "/");
  return (
    <details className="relative">
      <summary
        aria-label={labels.button}
        title={labels.button}
        className="flex h-9 cursor-pointer list-none items-center gap-1.5 rounded-md px-2 text-sm text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring [&::-webkit-details-marker]:hidden"
      >
        <LanguagesIcon aria-hidden="true" className="size-4" />
        <span className="uppercase">{current}</span>
      </summary>
      <div className="absolute end-0 z-40 mt-2 w-72 rounded-xl border border-border bg-popover p-3 text-popover-foreground shadow-lg">
        <p className="mb-2 text-xs font-medium text-muted-foreground">{labels.list}</p>
        <ul className="grid max-h-80 grid-cols-2 gap-x-3 gap-y-1 overflow-y-auto text-sm">
          {locales.map((locale) => {
            const href = localePath(locale.tag, path);
            return (
              <li key={locale.tag}>
                <a
                  href={href}
                  lang={locale.tag}
                  hrefLang={locale.tag}
                  aria-current={locale.tag === current ? "true" : undefined}
                  className="block rounded-sm px-1.5 py-1 hover:bg-accent aria-[current=true]:font-semibold"
                  onClick={(event) => {
                    event.preventDefault();
                    window.location.assign(
                      `${href}${window.location.search}${window.location.hash}`,
                    );
                  }}
                >
                  {locale.name}
                  {locale.community && (
                    <span
                      className="ms-1.5 rounded-sm bg-muted px-1 py-0.5 align-middle text-[0.65rem] font-medium text-muted-foreground"
                      title={labels.note}
                    >
                      {labels.badge}
                    </span>
                  )}
                </a>
              </li>
            );
          })}
        </ul>
      </div>
    </details>
  );
}
