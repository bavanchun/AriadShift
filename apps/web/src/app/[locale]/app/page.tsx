import type { Metadata } from "next";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { Suspense } from "react";
import { AppShell } from "@/components/app/app-shell";
import { pageAlternates } from "@/i18n/metadata";
import { RouteIntl } from "@/i18n/route-intl";
import { CLOUD } from "@/lib/cloud/config";

export async function generateMetadata({ params }: PageProps<"/[locale]/app">): Promise<Metadata> {
  const { locale } = await params;
  const t = await getTranslations({ locale, namespace: "web.converter_page" });
  return {
    title: t("meta_title"),
    description: t("meta_description"),
    alternates: pageAlternates(locale, "/app"),
  };
}

export default async function ConverterPage({ params }: PageProps<"/[locale]/app">) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations({ locale, namespace: "web.converter_page" });
  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-2">
        <h1 className="text-3xl font-bold tracking-tight">{t("title")}</h1>
        <p className="text-muted-foreground">{t("lede", { cloud: CLOUD ? "yes" : "no" })}</p>
      </div>
      <RouteIntl
        locale={locale}
        namespaces={["web.route", "web.converter", "web.app", "web.tools", "web.workspace"]}
      >
        <Suspense fallback={null}>
          <AppShell />
        </Suspense>
      </RouteIntl>
    </div>
  );
}
