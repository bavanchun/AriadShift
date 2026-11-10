// Renders client components with the real catalogue, the way a locale page provides it.
import { render } from "@testing-library/react";
import { NextIntlClientProvider } from "next-intl";
import type { ReactElement, ReactNode } from "react";
import { loadMessages } from "@/i18n/messages";

export function IntlWrapper({ children, locale = "en" }: { children: ReactNode; locale?: string }) {
  return (
    <NextIntlClientProvider locale={locale} messages={loadMessages(locale)} timeZone="UTC">
      {children}
    </NextIntlClientProvider>
  );
}

/** `render` inside the provider; `rerender` keeps it. */
export function renderIntl(ui: ReactElement, locale = "en") {
  const wrapper = ({ children }: { children: ReactNode }) => (
    <IntlWrapper locale={locale}>{children}</IntlWrapper>
  );
  return render(ui, { wrapper });
}
