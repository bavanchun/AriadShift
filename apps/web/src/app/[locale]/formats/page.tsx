import { Badge } from "@ariadshift/ui/components/badge";
import type { Metadata } from "next";
import Link from "next/link";
import { getFormatter, getTranslations, setRequestLocale } from "next-intl/server";
import { engineLabel } from "@/components/route-summary";
import { pageAlternates } from "@/i18n/metadata";
import { localePath } from "@/i18n/paths";
import { loadCapabilities, loadPairs } from "@/lib/catalog";
import { FORMATS, formatLabel, formatMessageKey, pairSlug } from "@/lib/formats";

export async function generateMetadata({
  params,
}: PageProps<"/[locale]/formats">): Promise<Metadata> {
  const { locale } = await params;
  const t = await getTranslations({ locale, namespace: "web.formats" });
  return {
    title: t("meta_title"),
    description: t("meta_description"),
    alternates: pageAlternates(locale, "/formats"),
  };
}

export default async function FormatsPage({ params }: PageProps<"/[locale]/formats">) {
  const { locale } = await params;
  setRequestLocale(locale);
  const [t, common, intl] = await Promise.all([
    getTranslations({ locale, namespace: "web.formats" }),
    getTranslations({ locale, namespace: "common" }),
    getFormatter({ locale }),
  ]);
  const caps = loadCapabilities();
  const pairs = loadPairs();
  const engines = (edges: Array<{ engine: string }>) =>
    edges.length > 0
      ? intl.list(
          edges.map((edge) => engineLabel(edge.engine, common)),
          { type: "unit" },
        )
      : t("no_engine");
  return (
    <div className="flex flex-col gap-10">
      <header className="flex flex-col gap-3">
        <h1 className="text-4xl font-bold tracking-tight">{t("title")}</h1>
        <p className="max-w-2xl text-lg text-muted-foreground">{t("lede")}</p>
      </header>
      <ul className="grid gap-6 md:grid-cols-2">
        {FORMATS.filter((format) => !format.internal).map((format) => {
          const reads = caps.edges.filter(
            (edge) => edge.from === format.id && edge.to === "ariad-ir+json",
          );
          const writes = caps.edges.filter(
            (edge) => edge.to === format.id && edge.from === "ariad-ir+json",
          );
          const inBrowser =
            format.id === "ariad-ir+json" ||
            [...reads, ...writes].some((edge) => edge.runtime.includes("wasm"));
          const targets = pairs.filter((pair) => pair.from === format);
          return (
            <li
              key={format.id}
              className="flex flex-col gap-3 rounded-xl border border-border bg-card p-5"
            >
              <div className="flex flex-wrap items-center gap-2">
                <h2 className="text-xl font-semibold">{format.label}</h2>
                <Badge variant={inBrowser ? "default" : "secondary"}>
                  {inBrowser ? t("badge_browser") : t("badge_cli")}
                </Badge>
              </div>
              <p className="text-sm text-muted-foreground">
                {common(`format.${formatMessageKey(format)}.description`)}
              </p>
              {format.extensions.length > 0 && (
                <p className="text-sm">
                  {t("extensions", {
                    list: intl.list(
                      format.extensions.map((extension) => `.${extension}`),
                      { type: "unit" },
                    ),
                  })}
                </p>
              )}
              {format.id !== "ariad-ir+json" && (
                <p className="text-sm">
                  {t("engines", { readers: engines(reads), writers: engines(writes) })}
                </p>
              )}
              {targets.length > 0 && (
                <p className="text-sm">
                  {t.rich("convert_to", {
                    targets: () => (
                      <>
                        {intl.list(
                          targets.map((pair) => (
                            <Link
                              key={pair.to.id}
                              className="underline"
                              href={localePath(
                                locale,
                                `/convert/${pairSlug(format.id, pair.to.id)}`,
                              )}
                            >
                              {formatLabel(pair.to.id)}
                            </Link>
                          )),
                          { type: "unit" },
                        )}
                      </>
                    ),
                  })}
                </p>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
