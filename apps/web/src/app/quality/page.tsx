import type { Metadata } from "next";
import { engineLabel } from "@/components/route-summary";
import { loadCapabilities } from "@/lib/catalog";
import { formatDuration, formatScore } from "@/lib/format-bytes";
import { formatLabel } from "@/lib/formats";
import { REPOSITORY_URL } from "@/lib/site";

export const metadata: Metadata = {
  title: "Measured quality",
  description:
    "Fidelity, editability and speed of every AriadShift conversion step, generated from a public benchmark.",
  alternates: { canonical: "/quality" },
};

const RUNTIME_LABELS: Record<string, string> = {
  wasm: "Browser",
  local: "CLI and desktop",
  cloud: "Cloud",
};

export default function QualityPage() {
  const caps = loadCapabilities();
  const generated = new Date(caps.generated_at);
  return (
    <div className="flex flex-col gap-8">
      <header className="flex flex-col gap-3">
        <h1 className="text-4xl font-bold tracking-tight">Measured quality</h1>
        <p className="max-w-2xl text-lg text-muted-foreground">
          These scores are not hand-written. The benchmark converts {caps.bench.fixture_count}{" "}
          public fixture documents through every step and compares the result with known truth. The
          planner reads the same numbers when it picks a route.
        </p>
        <p className="text-sm text-muted-foreground">
          Generated on {generated.toISOString().slice(0, 10)}
          {caps.bench.pandoc_version ? ` with Pandoc ${caps.bench.pandoc_version}` : ""}.{" "}
          <a className="underline" href={`${REPOSITORY_URL}/blob/main/docs/bench.md`}>
            Read the methodology
          </a>
          .
        </p>
      </header>
      <div className="overflow-x-auto rounded-xl border border-border">
        <table className="w-full text-left text-sm">
          <caption className="sr-only">Benchmark scores per conversion step</caption>
          <thead className="bg-muted">
            <tr>
              <th scope="col" className="px-4 py-3 font-semibold">
                Step
              </th>
              <th scope="col" className="px-4 py-3 font-semibold">
                Engine
              </th>
              <th scope="col" className="px-4 py-3 font-semibold">
                Runs in
              </th>
              <th scope="col" className="px-4 py-3 font-semibold">
                Fidelity
              </th>
              <th scope="col" className="px-4 py-3 font-semibold">
                Editability
              </th>
              <th scope="col" className="px-4 py-3 font-semibold">
                Median time
              </th>
              <th scope="col" className="px-4 py-3 font-semibold">
                Samples
              </th>
            </tr>
          </thead>
          <tbody>
            {caps.edges.map((edge) => (
              <tr key={`${edge.from}-${edge.to}-${edge.engine}`} className="border-t border-border">
                <th scope="row" className="px-4 py-3 font-medium">
                  {formatLabel(edge.from)} → {formatLabel(edge.to)}
                </th>
                <td className="px-4 py-3">
                  {engineLabel(edge.engine)}
                  <span className="block text-xs text-muted-foreground">{edge.license}</span>
                </td>
                <td className="px-4 py-3">
                  {edge.runtime.map((runtime) => RUNTIME_LABELS[runtime] ?? runtime).join(", ")}
                </td>
                <td className="px-4 py-3">{formatScore(edge.metrics?.fidelity ?? null)}</td>
                <td className="px-4 py-3">{formatScore(edge.metrics?.editability ?? null)}</td>
                <td className="px-4 py-3">{formatDuration(edge.metrics?.p50_ms ?? null)}</td>
                <td className="px-4 py-3">{edge.metrics?.samples ?? "–"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
