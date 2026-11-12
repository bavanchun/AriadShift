import { Badge } from "@ariadshift/ui/components/badge";
import { buttonVariants } from "@ariadshift/ui/components/button";
import { Card, CardContent, CardHeader, CardTitle } from "@ariadshift/ui/components/card";
import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { getTranslations, setRequestLocale } from "next-intl/server";
import { RouteSummary } from "@/components/route-summary";
import { allLocales } from "@/i18n/locales";
import { pageAlternates } from "@/i18n/metadata";
import { localePath } from "@/i18n/paths";
import { RouteIntl } from "@/i18n/route-intl";
import { loadPairs, type PairRoute } from "@/lib/catalog";
import { formatMessageKey, pairSlug } from "@/lib/formats";

export const dynamicParams = false;

export function generateStaticParams() {
  return allLocales().flatMap(({ tag }) =>
    loadPairs().map(({ from, to }) => ({ locale: tag, pair: pairSlug(from.id, to.id) })),
  );
}

function findPair(slug: string): PairRoute | undefined {
  return loadPairs().find(({ from, to }) => pairSlug(from.id, to.id) === slug);
}

export async function generateMetadata({
  params,
}: PageProps<"/[locale]/convert/[pair]">): Promise<Metadata> {
  const { locale, pair: slug } = await params;
  const pair = findPair(slug);
  if (!pair) {
    return {};
  }
  const [t, common] = await Promise.all([
    getTranslations({ locale, namespace: "web.pair" }),
    getTranslations({ locale, namespace: "common" }),
  ]);
  const values = {
    from: pair.from.label,
    to: pair.to.label,
    noun: common(`format.${formatMessageKey(pair.from)}.noun`),
  };
  return {
    title: t("meta_title", values),
    description: t(pair.browser ? "meta_description_browser" : "meta_description_cli", values),
    alternates: pageAlternates(locale, `/convert/${slug}`),
    // The preview image is English-only and shared by every locale.
    openGraph: { images: [`/convert/${slug}/opengraph-image`] },
    twitter: { images: [`/convert/${slug}/opengraph-image`] },
  };
}

export default async function PairPage({ params }: PageProps<"/[locale]/convert/[pair]">) {
  const { locale, pair: slug } = await params;
  setRequestLocale(locale);
  const pair = findPair(slug);
  if (!pair) {
    notFound();
  }
  const [t, common] = await Promise.all([
    getTranslations({ locale, namespace: "web.pair" }),
    getTranslations({ locale, namespace: "common" }),
  ]);
  const { from, to, browser, best } = pair;
  const appLink = `${localePath(locale, "/app")}?from=${encodeURIComponent(from.id)}&to=${encodeURIComponent(to.id)}`;
  const related = loadPairs().filter(
    (other) =>
      !(other.from.id === from.id && other.to.id === to.id) &&
      (other.from.id === from.id || other.to.id === to.id),
  );
  const values = {
    from: from.label,
    to: to.label,
    noun: common(`format.${formatMessageKey(from)}.noun`),
  };

  return (
    <article className="flex flex-col gap-10">
      <header className="flex flex-col gap-4">
        <Badge variant={browser ? "default" : "secondary"}>
          {browser ? t("badge_browser") : t("badge_cli")}
        </Badge>
        <h1 className="text-4xl font-bold tracking-tight">{t("title", values)}</h1>
        <p className="max-w-2xl text-lg text-muted-foreground">
          {t(browser ? "lede_browser" : "lede_cli", values)}
        </p>
        <div className="flex flex-wrap gap-3">
          {browser ? (
            <a href={appLink} className={buttonVariants({ size: "lg" })}>
              {t("convert_now", values)}
            </a>
          ) : (
            <Link href={localePath(locale, "/download")} className={buttonVariants({ size: "lg" })}>
              {t("install_cli")}
            </Link>
          )}
        </div>
      </header>

      <Card>
        <CardHeader>
          <CardTitle>
            <h2 className="text-lg">{t("route_heading")}</h2>
          </CardTitle>
        </CardHeader>
        <CardContent>
          <RouteIntl locale={locale} namespaces={["web.route"]}>
            <RouteSummary route={browser ?? best} />
          </RouteIntl>
        </CardContent>
      </Card>

      <section className="grid gap-6 md:grid-cols-2">
        <div className="flex flex-col gap-2">
          <h2 className="text-lg font-semibold">{t("from_heading", { format: from.label })}</h2>
          <p className="text-sm text-muted-foreground">
            {common(`format.${formatMessageKey(from)}.description`)}
          </p>
        </div>
        <div className="flex flex-col gap-2">
          <h2 className="text-lg font-semibold">{t("to_heading", { format: to.label })}</h2>
          <p className="text-sm text-muted-foreground">
            {common(`format.${formatMessageKey(to)}.description`)}
          </p>
        </div>
      </section>

      <section className="flex flex-col gap-3">
        <h2 className="text-lg font-semibold">{t("cli_heading")}</h2>
        <pre className="overflow-x-auto rounded-lg bg-muted p-4 text-sm" dir="ltr">
          <code>{`ashift convert input.${from.extensions[0]} --to ${to.id === "ariad-ir+json" ? "json" : to.extensions[0]}`}</code>
        </pre>
      </section>

      {related.length > 0 && (
        <nav aria-labelledby="related-heading" className="flex flex-col gap-3">
          <h2 id="related-heading" className="text-lg font-semibold">
            {t("related_heading")}
          </h2>
          <ul className="flex flex-wrap gap-2">
            {related.map((other) => (
              <li key={pairSlug(other.from.id, other.to.id)}>
                <Link
                  className={buttonVariants({ variant: "secondary", size: "sm" })}
                  href={localePath(locale, `/convert/${pairSlug(other.from.id, other.to.id)}`)}
                >
                  {t("related_link", { from: other.from.label, to: other.to.label })}
                </Link>
              </li>
            ))}
          </ul>
        </nav>
      )}
    </article>
  );
}
