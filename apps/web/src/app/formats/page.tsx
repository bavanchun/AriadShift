import { Badge } from "@ariadshift/ui/components/badge";
import type { Metadata } from "next";
import Link from "next/link";
import { engineLabel } from "@/components/route-summary";
import { loadCapabilities, loadPairs } from "@/lib/catalog";
import { FORMATS, formatLabel, pairSlug } from "@/lib/formats";

export const metadata: Metadata = {
  title: "Supported formats",
  description:
    "Which document formats AriadShift reads and writes, and which conversions run in your browser.",
  alternates: { canonical: "/formats" },
};

export default function FormatsPage() {
  const caps = loadCapabilities();
  const pairs = loadPairs();
  return (
    <div className="flex flex-col gap-10">
      <header className="flex flex-col gap-3">
        <h1 className="text-4xl font-bold tracking-tight">Supported formats</h1>
        <p className="max-w-2xl text-lg text-muted-foreground">
          Every conversion passes through AriadShift's document model. Formats marked “browser”
          convert entirely on this device; the others need the command-line tool for now.
        </p>
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
                  {inBrowser ? "Browser and CLI" : "CLI"}
                </Badge>
              </div>
              <p className="text-sm text-muted-foreground">{format.description}</p>
              {format.extensions.length > 0 && (
                <p className="text-sm">
                  Extensions: {format.extensions.map((extension) => `.${extension}`).join(", ")}
                </p>
              )}
              {format.id !== "ariad-ir+json" && (
                <p className="text-sm">
                  Read by{" "}
                  {reads.map((edge) => engineLabel(edge.engine)).join(", ") || "no engine yet"}
                  {"; written by "}
                  {writes.map((edge) => engineLabel(edge.engine)).join(", ") || "no engine yet"}.
                </p>
              )}
              {targets.length > 0 && (
                <p className="text-sm">
                  Convert to:{" "}
                  {targets.map((pair, index) => (
                    <span key={pair.to.id}>
                      {index > 0 && ", "}
                      <Link
                        className="underline"
                        href={`/convert/${pairSlug(format.id, pair.to.id)}`}
                      >
                        {formatLabel(pair.to.id)}
                      </Link>
                    </span>
                  ))}
                </p>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
