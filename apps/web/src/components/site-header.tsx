import { ThemeToggle } from "@ariadshift/ui/components/theme-toggle";
import { getTranslations } from "next-intl/server";
import { Suspense } from "react";
import { publishedLocales } from "@/i18n/locales";
import { localePath } from "@/i18n/paths";
import { NAVIGATION, SITE_NAME } from "@/lib/site";
import { LanguageSwitcher } from "./language-switcher";

// Plain links, not client-side navigation: /app is cross-origin isolated by its response
// headers, which only apply to a document loaded from it, and pages left from /app must
// not stay isolated.
export async function SiteHeader({ locale }: { locale: string }) {
  const t = await getTranslations({ locale, namespace: "web.shell" });
  // The current language is always listed, even when it is not published, so the picker
  // shows where the visitor is.
  const listed = publishedLocales();
  const options = (
    listed.some((entry) => entry.tag === locale)
      ? listed
      : [...listed, { tag: locale, name: locale, community: true }]
  ).map(({ tag, name, community }) => ({ tag, name, community }));
  const current = options.find((option) => option.tag === locale);
  return (
    <header className="border-b border-border">
      <div className="mx-auto flex max-w-5xl flex-wrap items-center gap-x-6 gap-y-2 px-4 py-3">
        <a
          href={localePath(locale, "/")}
          className="flex items-center gap-2 font-bold tracking-tight"
        >
          <img src="/brand/ariadshift-app-icon.svg" alt="" width={32} height={32} />
          <span className="text-lg">{SITE_NAME}</span>
        </a>
        <nav aria-label={t("nav_label")} className="order-3 w-full sm:order-none sm:w-auto">
          <ul className="flex flex-wrap gap-x-4 gap-y-1 text-sm">
            {NAVIGATION.map((item) => (
              <li key={item.href}>
                <a
                  href={localePath(locale, item.href)}
                  className="rounded-sm text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring"
                >
                  {t(`nav.${item.key}`)}
                </a>
              </li>
            ))}
          </ul>
        </nav>
        <div className="ms-auto flex items-center gap-1">
          <LanguageSwitcher
            current={locale}
            locales={options}
            labels={{
              button: t("language.button", { name: current?.name ?? locale }),
              list: t("language.list_label"),
              badge: t("language.community_badge"),
              note: t("language.community_note"),
            }}
          />
          <Suspense fallback={null}>
            <ThemeToggle />
          </Suspense>
        </div>
      </div>
    </header>
  );
}
