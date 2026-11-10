import type { Metadata, Viewport } from "next";
import { notFound } from "next/navigation";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { SiteFooter } from "@/components/site-footer";
import { SiteHeader } from "@/components/site-header";
import { allLocales, findLocale } from "@/i18n/locales";
import { SITE_NAME, SITE_URL } from "@/lib/site";
import "../globals.css";

export const dynamicParams = false;

export function generateStaticParams() {
  return allLocales().map(({ tag }) => ({ locale: tag }));
}

export async function generateMetadata({ params }: LayoutProps<"/[locale]">): Promise<Metadata> {
  const { locale } = await params;
  const info = findLocale(locale);
  if (!info) {
    return {};
  }
  const t = await getTranslations({ locale, namespace: "web.meta" });
  return {
    metadataBase: new URL(SITE_URL),
    title: {
      default: t("site_title", { site: SITE_NAME }),
      template: t("title_template", { title: "%s", site: SITE_NAME }),
    },
    description: t("site_description"),
    applicationName: SITE_NAME,
    openGraph: { type: "website", siteName: SITE_NAME, url: SITE_URL, locale },
    twitter: { card: "summary_large_image" },
    // Locales that are not published (see docs/i18n.md) stay out of search results.
    ...(info.published ? {} : { robots: { index: false, follow: true } }),
  };
}

export const viewport: Viewport = {
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: "#f6f4ee" },
    { media: "(prefers-color-scheme: dark)", color: "#171a16" },
  ],
};

// Applies the stored or system theme before the first paint.
const THEME_SCRIPT = `try{var t=localStorage.getItem("ariadshift-theme");var d=t==="dark"||(t!=="light"&&matchMedia("(prefers-color-scheme: dark)").matches);document.documentElement.classList.toggle("dark",d)}catch(e){}`;

export default async function LocaleLayout({ children, params }: LayoutProps<"/[locale]">) {
  const { locale } = await params;
  const info = findLocale(locale);
  if (!info) {
    notFound();
  }
  setRequestLocale(locale);
  const t = await getTranslations("web.shell");
  return (
    <html lang={locale} dir={info.dir} suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: THEME_SCRIPT }} />
      </head>
      <body className="flex min-h-screen flex-col">
        <a
          href="#main"
          className="sr-only focus:not-sr-only focus:absolute focus:start-4 focus:top-4 focus:z-50 focus:rounded-md focus:bg-primary focus:px-3 focus:py-2 focus:text-primary-foreground"
        >
          {t("skip_to_content")}
        </a>
        <SiteHeader locale={locale} />
        <main id="main" className="mx-auto w-full max-w-5xl flex-1 px-4 py-10">
          {children}
        </main>
        <SiteFooter locale={locale} />
      </body>
    </html>
  );
}
