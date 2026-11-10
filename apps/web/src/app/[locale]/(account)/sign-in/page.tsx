import type { Metadata } from "next";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { SignInPanel } from "@/components/account/sign-in-panel";

export async function generateMetadata({
  params,
}: PageProps<"/[locale]/sign-in">): Promise<Metadata> {
  const { locale } = await params;
  const t = await getTranslations({ locale, namespace: "web.account_meta.sign_in" });
  return { title: t("title"), description: t("description") };
}

export default async function Page({ params }: PageProps<"/[locale]/sign-in">) {
  setRequestLocale((await params).locale);
  return <SignInPanel />;
}
