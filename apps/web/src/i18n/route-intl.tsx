import { NextIntlClientProvider } from "next-intl";
import type { ReactNode } from "react";
import { loadMessages, scopeMessages } from "./messages";

/**
 * Gives a route's client components their messages: `common` plus the namespaces the route
 * lists, for example `web.converter`. Other namespaces never reach the browser.
 */
export function RouteIntl({
  locale,
  namespaces,
  children,
}: {
  locale: string;
  namespaces: readonly string[];
  children: ReactNode;
}) {
  return (
    <NextIntlClientProvider
      locale={locale}
      messages={scopeMessages(loadMessages(locale), ["common", ...namespaces])}
      timeZone="UTC"
    >
      {children}
    </NextIntlClientProvider>
  );
}
