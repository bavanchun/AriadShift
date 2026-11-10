import type { Metadata } from "next";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { AccountPage } from "@/components/account/account-page";

export async function generateMetadata({
  params,
}: PageProps<"/[locale]/account">): Promise<Metadata> {
  const { locale } = await params;
  const t = await getTranslations({ locale, namespace: "web.account_meta.account" });
  return { title: t("title"), description: t("description") };
}

export default async function Page({ params }: PageProps<"/[locale]/account">) {
  setRequestLocale((await params).locale);
  return <AccountPage />;
}
