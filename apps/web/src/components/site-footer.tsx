import { getTranslations } from "next-intl/server";
import { localePath } from "@/i18n/paths";
import { REPOSITORY_URL } from "@/lib/site";

export async function SiteFooter({ locale }: { locale: string }) {
  const t = await getTranslations({ locale, namespace: "web.shell.footer" });
  return (
    <footer className="mt-16 border-t border-border">
      <div className="mx-auto flex max-w-5xl flex-wrap items-center justify-between gap-4 px-4 py-6 text-sm text-muted-foreground">
        <p>{t("license")}</p>
        <ul className="flex flex-wrap gap-4">
          <li>
            <a className="hover:text-foreground" href={localePath(locale, "/privacy")}>
              {t("privacy")}
            </a>
          </li>
          <li>
            <a className="hover:text-foreground" href={localePath(locale, "/licenses")}>
              {t("licenses")}
            </a>
          </li>
          <li>
            <a className="hover:text-foreground" href={localePath(locale, "/quality")}>
              {t("quality")}
            </a>
          </li>
          <li>
            <a className="hover:text-foreground" href={REPOSITORY_URL}>
              {t("source")}
            </a>
          </li>
        </ul>
      </div>
    </footer>
  );
}
