import { UiDevShowcase } from "@ariadshift/ui/components/dev-showcase";
import type { Metadata } from "next";
import { setRequestLocale } from "next-intl/server";
import { DEFAULT_LOCALE } from "@/i18n/locales";

// A maintainer aid for the shared component library, not a product page: one English route,
// kept out of search results and the sitemap.
export const metadata: Metadata = {
  title: "UI components",
  robots: { index: false, follow: false },
};

export const dynamicParams = false;

export function generateStaticParams() {
  return [{ locale: DEFAULT_LOCALE }];
}

export default async function DevUiPage({ params }: PageProps<"/[locale]/dev/ui">) {
  const { locale } = await params;
  setRequestLocale(locale);
  return <UiDevShowcase />;
}
