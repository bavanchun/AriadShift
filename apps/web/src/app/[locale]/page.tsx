import { buttonVariants } from "@ariadshift/ui/components/button";
import { Card, CardContent, CardHeader, CardTitle } from "@ariadshift/ui/components/card";
import { BotIcon, GaugeIcon, ShieldCheckIcon } from "lucide-react";
import type { Metadata } from "next";
import Link from "next/link";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { pageAlternates } from "@/i18n/metadata";
import { localePath } from "@/i18n/paths";
import { loadPairs } from "@/lib/catalog";
import { pairSlug } from "@/lib/formats";

const DIFFERENTIATORS = [
  { icon: ShieldCheckIcon, key: "private" },
  { icon: GaugeIcon, key: "quality" },
  { icon: BotIcon, key: "agents" },
] as const;

const STEPS = [1, 2, 3, 4] as const;

export async function generateMetadata({ params }: PageProps<"/[locale]">): Promise<Metadata> {
  const { locale } = await params;
  const t = await getTranslations({ locale, namespace: "web.home" });
  return {
    title: { absolute: t("meta_title") },
    alternates: pageAlternates(locale, "/"),
  };
}

export default async function HomePage({ params }: PageProps<"/[locale]">) {
  const { locale } = await params;
  setRequestLocale(locale);
  const t = await getTranslations({ locale, namespace: "web.home" });
  const href = (path: string) => localePath(locale, path);
  const browserPairs = loadPairs().filter((pair) => pair.browser !== null);
  return (
    <div className="flex flex-col gap-16">
      <section className="flex flex-col items-start gap-6 pt-6">
        <img src="/brand/ariadshift-app-icon.svg" alt="" width={88} height={88} />
        <h1 className="max-w-3xl text-4xl font-bold tracking-tight sm:text-5xl">{t("title")}</h1>
        <p className="max-w-2xl text-lg text-muted-foreground">{t("lede")}</p>
        <div className="flex flex-wrap gap-3">
          <a href={href("/app")} className={buttonVariants({ size: "lg" })}>
            {t("open_converter")}
          </a>
          <Link
            href={href("/download")}
            className={buttonVariants({ size: "lg", variant: "secondary" })}
          >
            {t("install_cli")}
          </Link>
        </div>
      </section>

      <section aria-labelledby="why-heading" className="flex flex-col gap-6">
        <h2 id="why-heading" className="text-2xl font-bold tracking-tight">
          {t("why_heading")}
        </h2>
        <div className="grid gap-4 md:grid-cols-3">
          {DIFFERENTIATORS.map(({ icon: Icon, key }) => (
            <Card key={key}>
              <CardHeader>
                <Icon aria-hidden="true" className="size-6 text-primary" />
                <CardTitle>
                  <h3 className="text-lg">{t(`why.${key}.title`)}</h3>
                </CardTitle>
              </CardHeader>
              <CardContent>
                <p className="text-sm text-muted-foreground">{t(`why.${key}.body`)}</p>
              </CardContent>
            </Card>
          ))}
        </div>
      </section>

      <section aria-labelledby="how-heading" className="flex flex-col gap-4">
        <h2 id="how-heading" className="text-2xl font-bold tracking-tight">
          {t("how_heading")}
        </h2>
        <ol className="grid gap-3 text-sm sm:grid-cols-4">
          {STEPS.map((step) => (
            <li key={step} className="rounded-xl border border-border bg-card p-4">
              <span className="mb-2 block font-semibold text-primary">
                {t("how.step_label", { number: step })}
              </span>
              {t(`how.step_${step}`)}
            </li>
          ))}
        </ol>
      </section>

      <section aria-labelledby="pairs-heading" className="flex flex-col gap-4">
        <h2 id="pairs-heading" className="text-2xl font-bold tracking-tight">
          {t("pairs_heading")}
        </h2>
        <ul className="flex flex-wrap gap-3">
          {browserPairs.map(({ from, to }) => (
            <li key={pairSlug(from.id, to.id)}>
              <Link
                href={href(`/convert/${pairSlug(from.id, to.id)}`)}
                className={buttonVariants({ variant: "secondary" })}
              >
                {t("pair_link", { from: from.label, to: to.label })}
              </Link>
            </li>
          ))}
        </ul>
        <p className="text-sm text-muted-foreground">
          {t.rich("see_more", {
            formats: (chunks) => (
              <Link className="underline" href={href("/formats")}>
                {chunks}
              </Link>
            ),
            quality: (chunks) => (
              <Link className="underline" href={href("/quality")}>
                {chunks}
              </Link>
            ),
          })}
        </p>
      </section>
    </div>
  );
}
