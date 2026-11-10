import type { Plan } from "@ariadshift/ariad-wasm";
import { Badge } from "@ariadshift/ui/components/badge";
import { ArrowRightIcon } from "lucide-react";
import { useLocale, useTranslations } from "next-intl";
import { formatDuration, formatScore } from "@/lib/format-bytes";
import { formatLabel } from "@/lib/formats";

/** Display names of the engines; only the three AriadShift-branded ones are translated. */
const ENGINE_LABELS: Record<string, string> = {
  "ariad-core": "AriadShift core",
  "ariad-pdf": "AriadShift PDF",
  pandoc: "Pandoc",
  pdf: "PDF text reader",
  typst: "Typst",
  libreoffice: "LibreOffice",
  docling: "Docling",
};

const TRANSLATED_ENGINES: Record<string, string> = {
  "ariad-core": "ariad_core",
  "ariad-pdf": "ariad_pdf",
  pdf: "pdf",
};

/**
 * A readable engine name. With a translator for the `common` namespace the branded names are
 * translated; without one the English names are used. Third-party names are never translated.
 */
export function engineLabel(engine: string, translate?: (key: string) => string): string {
  const key = TRANSLATED_ENGINES[engine];
  if (translate && key) {
    return translate(`engine.${key}`);
  }
  return ENGINE_LABELS[engine] ?? engine;
}

/** The planner's route with its measured scores, as plain markup. */
export function RouteSummary({ route }: { route: Plan }) {
  const t = useTranslations("web.route");
  const common = useTranslations("common");
  const locale = useLocale();
  const text = {
    missing: common("score.not_measured"),
    underOneMs: common("duration.under_1_ms"),
  };
  const first = route.steps[0];
  return (
    <div className="flex flex-col gap-3">
      <ol className="flex flex-wrap items-center gap-2 text-sm" aria-label={t("label")}>
        {first && (
          <li>
            <Badge variant="secondary">{formatLabel(first.from)}</Badge>
          </li>
        )}
        {route.steps.map((step) => (
          <li key={`${step.from}-${step.to}-${step.engine}`} className="flex items-center gap-2">
            <ArrowRightIcon
              aria-hidden="true"
              className="size-4 text-muted-foreground rtl:rotate-180"
            />
            <span className="sr-only">{t("then")}</span>
            <span className="text-muted-foreground">{engineLabel(step.engine, common)}</span>
            <ArrowRightIcon
              aria-hidden="true"
              className="size-4 text-muted-foreground rtl:rotate-180"
            />
            <Badge variant="secondary">{formatLabel(step.to)}</Badge>
          </li>
        ))}
      </ol>
      <dl className="grid grid-cols-3 gap-3 text-sm">
        <div>
          <dt className="text-muted-foreground">{t("fidelity")}</dt>
          <dd className="font-semibold">
            {formatScore(route.score.fidelity, locale, text.missing)}
          </dd>
        </div>
        <div>
          <dt className="text-muted-foreground">{t("editability")}</dt>
          <dd className="font-semibold">
            {formatScore(route.score.editability, locale, text.missing)}
          </dd>
        </div>
        <div>
          <dt className="text-muted-foreground">{t("estimated_time")}</dt>
          <dd className="font-semibold">
            {formatDuration(route.score.estimated_duration_ms, locale, text)}
          </dd>
        </div>
      </dl>
      {!route.measured && <p className="text-sm text-muted-foreground">{t("unmeasured")}</p>}
    </div>
  );
}
