import type { Metadata } from "next";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { ApiKeysPanel } from "@/components/account/api-keys-panel";

export async function generateMetadata({
  params,
}: PageProps<"/[locale]/api-keys">): Promise<Metadata> {
  const { locale } = await params;
  const t = await getTranslations({ locale, namespace: "web.account_meta.api_keys" });
  return { title: t("title"), description: t("description") };
}

export default async function Page({ params }: PageProps<"/[locale]/api-keys">) {
  setRequestLocale((await params).locale);
  return <ApiKeysPanel />;
}
