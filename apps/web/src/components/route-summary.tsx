import type { Plan } from "@ariadshift/ariad-wasm";
import { Badge } from "@ariadshift/ui/components/badge";
import { ArrowRightIcon } from "lucide-react";
import { formatDuration, formatScore } from "@/lib/format-bytes";
import { formatLabel } from "@/lib/formats";

const ENGINE_LABELS: Record<string, string> = {
  "ariad-core": "AriadShift core",
  pandoc: "Pandoc",
  pdf: "PDF text reader",
  typst: "Typst",
  libreoffice: "LibreOffice",
  docling: "Docling",
};

export function engineLabel(engine: string): string {
  return ENGINE_LABELS[engine] ?? engine;
}

/** The planner's route with its measured scores, as plain markup. */
export function RouteSummary({ route }: { route: Plan }) {
  const first = route.steps[0];
  return (
    <div className="flex flex-col gap-3">
      <ol className="flex flex-wrap items-center gap-2 text-sm" aria-label="Conversion route">
        {first && (
          <li>
            <Badge variant="secondary">{formatLabel(first.from)}</Badge>
          </li>
        )}
        {route.steps.map((step) => (
          <li key={`${step.from}-${step.to}-${step.engine}`} className="flex items-center gap-2">
            <ArrowRightIcon aria-hidden="true" className="size-4 text-muted-foreground" />
            <span className="sr-only">then</span>
            <span className="text-muted-foreground">{engineLabel(step.engine)}</span>
            <ArrowRightIcon aria-hidden="true" className="size-4 text-muted-foreground" />
            <Badge variant="secondary">{formatLabel(step.to)}</Badge>
          </li>
        ))}
      </ol>
      <dl className="grid grid-cols-3 gap-3 text-sm">
        <div>
          <dt className="text-muted-foreground">Fidelity</dt>
          <dd className="font-semibold">{formatScore(route.score.fidelity)}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Editability</dt>
          <dd className="font-semibold">{formatScore(route.score.editability)}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Estimated time</dt>
          <dd className="font-semibold">{formatDuration(route.score.estimated_duration_ms)}</dd>
        </div>
      </dl>
      {!route.measured && (
        <p className="text-sm text-muted-foreground">
          Part of this route has not been benchmarked yet, so the planner scores it cautiously.
        </p>
      )}
    </div>
  );
}
