import type { Metadata } from "next";
import { getFormatter, getTranslations, setRequestLocale } from "next-intl/server";
import { engineLabel } from "@/components/route-summary";
import { pageAlternates } from "@/i18n/metadata";
import { loadCapabilities } from "@/lib/catalog";
import { formatDuration, formatScore } from "@/lib/format-bytes";
import { formatLabel } from "@/lib/formats";
import { REPOSITORY_URL } from "@/lib/site";

export async function generateMetadata({
  params,
}: PageProps<"/[locale]/quality">): Promise<Metadata> {
  const { locale } = await params;
  const t = await getTranslations({ locale, namespace: "web.quality" });
  return {
    title: t("meta_title"),
    description: t("meta_description"),
    alternates: pageAlternates(locale, "/quality"),
  };
}

const RUNTIMES = new Set(["wasm", "local", "cloud"]);

const COLUMNS = [
  "step",
  "engine",
  "runs_in",
  "fidelity",
  "editability",
  "median_time",
  "samples",
] as const;

export default async function QualityPage({ params }: PageProps<"/[locale]/quality">) {
  const { locale } = await params;
  setRequestLocale(locale);
  const [t, common, intl] = await Promise.all([
    getTranslations({ locale, namespace: "web.quality" }),
    getTranslations({ locale, namespace: "common" }),
    getFormatter({ locale }),
  ]);
  const caps = loadCapabilities();
  const text = { missing: common("score.not_measured"), underOneMs: common("duration.under_1_ms") };
  const runtime = (id: string) => (RUNTIMES.has(id) ? common(`runtime.${id}`) : id);
  return (
    <div className="flex flex-col gap-8">
      <header className="flex flex-col gap-3">
        <h1 className="text-4xl font-bold tracking-tight">{t("title")}</h1>
        <p className="max-w-2xl text-lg text-muted-foreground">
          {t("lede", { count: caps.bench.fixture_count })}
        </p>
        <p className="text-sm text-muted-foreground">
          {t("generated", {
            date: new Date(caps.generated_at).toISOString().slice(0, 10),
            pandoc: caps.bench.pandoc_version ?? "none",
          })}{" "}
          {t.rich("methodology", {
            link: (chunks) => (
              <a className="underline" href={`${REPOSITORY_URL}/blob/main/docs/bench.md`}>
                {chunks}
              </a>
            ),
          })}
        </p>
      </header>
      <div className="overflow-x-auto rounded-xl border border-border">
        <table className="w-full text-start text-sm">
          <caption className="sr-only">{t("caption")}</caption>
          <thead className="bg-muted">
            <tr>
              {COLUMNS.map((column) => (
                <th key={column} scope="col" className="px-4 py-3 text-start font-semibold">
                  {t(`column.${column}`)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {caps.edges.map((edge) => (
              <tr key={`${edge.from}-${edge.to}-${edge.engine}`} className="border-t border-border">
                <th scope="row" className="px-4 py-3 text-start font-medium">
                  {formatLabel(edge.from)} → {formatLabel(edge.to)}
                </th>
                <td className="px-4 py-3">
                  {engineLabel(edge.engine, common)}
                  <span className="block text-xs text-muted-foreground">{edge.license}</span>
                </td>
                <td className="px-4 py-3">
                  {intl.list(edge.runtime.map(runtime), { type: "unit" })}
                </td>
                <td className="px-4 py-3">
                  {formatScore(edge.metrics?.fidelity ?? null, locale, text.missing)}
                </td>
                <td className="px-4 py-3">
                  {formatScore(edge.metrics?.editability ?? null, locale, text.missing)}
                </td>
                <td className="px-4 py-3">
                  {formatDuration(edge.metrics?.p50_ms ?? null, locale, text)}
                </td>
                <td className="px-4 py-3">
                  {edge.metrics ? intl.number(edge.metrics.samples) : "–"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
