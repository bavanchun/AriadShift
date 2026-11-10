import type { Metadata } from "next";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { JobsPanel } from "@/components/account/jobs-panel";

export async function generateMetadata({ params }: PageProps<"/[locale]/jobs">): Promise<Metadata> {
  const { locale } = await params;
  const t = await getTranslations({ locale, namespace: "web.account_meta.jobs" });
  return { title: t("title"), description: t("description") };
}

export default async function Page({ params }: PageProps<"/[locale]/jobs">) {
  setRequestLocale((await params).locale);
  return <JobsPanel />;
}
