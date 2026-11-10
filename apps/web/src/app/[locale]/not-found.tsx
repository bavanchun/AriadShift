import { buttonVariants } from "@ariadshift/ui/components/button";
import Link from "next/link";
import { getLocale, getTranslations } from "next-intl/server";
import { localePath } from "@/i18n/paths";

export default async function NotFound() {
  const [t, locale] = await Promise.all([getTranslations("web.not_found"), getLocale()]);
  return (
    <div className="flex flex-col items-start gap-4">
      <h1 className="text-3xl font-bold tracking-tight">{t("title")}</h1>
      <p className="text-muted-foreground">{t("body")}</p>
      <Link href={localePath(locale, "/")} className={buttonVariants()}>
        {t("home_link")}
      </Link>
    </div>
  );
}
