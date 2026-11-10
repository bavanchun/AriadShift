import type { Metadata } from "next";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { pageAlternates } from "@/i18n/metadata";

export async function generateMetadata({
  params,
}: PageProps<"/[locale]/privacy">): Promise<Metadata> {
  const { locale } = await params;
  const t = await getTranslations({ locale, namespace: "legal.privacy" });
  return {
    title: t("meta_title"),
    description: t("meta_description"),
    alternates: pageAlternates(locale, "/privacy"),
  };
}

export default async function PrivacyPage({ params }: PageProps<"/[locale]/privacy">) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations({ locale, namespace: "legal.privacy" });
  return (
    <article className="flex max-w-3xl flex-col gap-6">
      <h1 className="text-4xl font-bold tracking-tight">{t("title")}</h1>
      <p className="text-lg text-muted-foreground">{t("lede")}</p>
      <section className="flex flex-col gap-2">
        <h2 className="text-xl font-semibold">{t("documents_heading")}</h2>
        <p>{t("documents_storage")}</p>
        <p>{t("documents_memory")}</p>
      </section>
      <section className="flex flex-col gap-2">
        <h2 className="text-xl font-semibold">{t("deleting_heading")}</h2>
        <p>{t("deleting_body")}</p>
      </section>
      <section className="flex flex-col gap-2">
        <h2 className="text-xl font-semibold">{t("tracking_heading")}</h2>
        <p>{t("tracking_body")}</p>
      </section>
      <section className="flex flex-col gap-2">
        <h2 className="text-xl font-semibold">{t("hosting_heading")}</h2>
        <p>{t("hosting_body")}</p>
      </section>
    </article>
  );
}
