import type { Metadata } from "next";
import { setRequestLocale } from "next-intl/server";
import type { ReactNode } from "react";
import { AccountShell } from "@/components/account/account-shell";
import { RouteIntl } from "@/i18n/route-intl";

// Client-rendered against the auth service and `/v1`, so the static export
// carries no account data. Personal pages stay out of search results.
export const metadata: Metadata = {
  robots: { index: false, follow: false },
};

export default async function AccountLayout({
  children,
  params,
}: {
  children: ReactNode;
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await params;
  setRequestLocale(locale);
  return (
    <RouteIntl locale={locale} namespaces={["web.account"]}>
      <AccountShell>{children}</AccountShell>
    </RouteIntl>
  );
}
