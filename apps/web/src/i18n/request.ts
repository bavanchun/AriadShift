// next-intl's per-request configuration. Every page calls setRequestLocale, so the locale
// comes from the URL segment and each locale is prerendered by generateStaticParams.
import { getRequestConfig } from "next-intl/server";
import { DEFAULT_LOCALE, isLocale } from "./locales";
import { loadMessages } from "./messages";

export default getRequestConfig(async ({ requestLocale }) => {
  const requested = await requestLocale;
  const locale = requested && isLocale(requested) ? requested : DEFAULT_LOCALE;
  return { locale, messages: loadMessages(locale), timeZone: "UTC" };
});
