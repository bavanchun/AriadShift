import { Badge } from "@ariadshift/ui/components/badge";
import { buttonVariants } from "@ariadshift/ui/components/button";
import { Card, CardContent, CardHeader, CardTitle } from "@ariadshift/ui/components/card";
import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { RouteSummary } from "@/components/route-summary";
import { loadPairs, type PairRoute } from "@/lib/catalog";
import { pairSlug } from "@/lib/formats";

export const dynamicParams = false;

export function generateStaticParams() {
  return loadPairs().map(({ from, to }) => ({ pair: pairSlug(from.id, to.id) }));
}

const CLI_FORMAT: Record<string, string> = {
  markdown: "md",
  html: "html",
  docx: "docx",
  epub: "epub",
};

/** The equivalent `ashift` command for a pair. */
function cliCommand(from: string, to: string, extension: string): string {
  if (from === "ariad-ir+json") {
    return `ashift __write input.ir.json --to ${CLI_FORMAT[to]} -o output.${CLI_FORMAT[to]}`;
  }
  if (to === "ariad-ir+json") {
    return `ashift __ir input.${extension} -o output.ir.json`;
  }
  return `ashift convert input.${extension} --to ${CLI_FORMAT[to]}`;
}

function findPair(slug: string): PairRoute | undefined {
  return loadPairs().find(({ from, to }) => pairSlug(from.id, to.id) === slug);
}

export async function generateMetadata({
  params,
}: PageProps<"/convert/[pair]">): Promise<Metadata> {
  const pair = findPair((await params).pair);
  if (!pair) {
    return {};
  }
  const where = pair.browser ? "in your browser, privately" : "on your own computer";
  return {
    title: `Convert ${pair.from.label} to ${pair.to.label}`,
    description: `Convert a ${pair.from.noun} to ${pair.to.label} ${where} with AriadShift. Measured fidelity, no uploads.`,
    alternates: { canonical: `/convert/${pairSlug(pair.from.id, pair.to.id)}` },
  };
}

export default async function PairPage({ params }: PageProps<"/convert/[pair]">) {
  const pair = findPair((await params).pair);
  if (!pair) {
    notFound();
  }
  const { from, to, browser, best } = pair;
  const appLink = `/app?from=${encodeURIComponent(from.id)}&to=${encodeURIComponent(to.id)}`;
  const related = loadPairs().filter(
    (other) => other !== pair && (other.from === from || other.to === to),
  );

  return (
    <article className="flex flex-col gap-10">
      <header className="flex flex-col gap-4">
        <Badge variant={browser ? "default" : "secondary"}>
          {browser ? "Runs in your browser" : "Runs with the AriadShift CLI"}
        </Badge>
        <h1 className="text-4xl font-bold tracking-tight">
          Convert {from.label} to {to.label}
        </h1>
        <p className="max-w-2xl text-lg text-muted-foreground">
          {browser
            ? `Turn a ${from.noun} into ${to.label} without uploading it. AriadShift reads the file in your browser, maps it to its document model and writes ${to.label} on your device.`
            : `Turn a ${from.noun} into ${to.label} on your own computer with the free AriadShift command-line tool. The browser version does not run this route yet.`}
        </p>
        <div className="flex flex-wrap gap-3">
          {browser ? (
            <Link href={appLink} className={buttonVariants({ size: "lg" })}>
              Convert {from.label} to {to.label} now
            </Link>
          ) : (
            <Link href="/download" className={buttonVariants({ size: "lg" })}>
              Install the CLI
            </Link>
          )}
        </div>
      </header>

      <Card>
        <CardHeader>
          <CardTitle>
            <h2 className="text-lg">The route AriadShift takes</h2>
          </CardTitle>
        </CardHeader>
        <CardContent>
          <RouteSummary route={browser ?? best} />
        </CardContent>
      </Card>

      <section className="grid gap-6 md:grid-cols-2">
        <div className="flex flex-col gap-2">
          <h2 className="text-lg font-semibold">From {from.label}</h2>
          <p className="text-sm text-muted-foreground">{from.description}</p>
        </div>
        <div className="flex flex-col gap-2">
          <h2 className="text-lg font-semibold">To {to.label}</h2>
          <p className="text-sm text-muted-foreground">{to.description}</p>
        </div>
      </section>

      <section className="flex flex-col gap-3">
        <h2 className="text-lg font-semibold">From the command line</h2>
        <pre className="overflow-x-auto rounded-lg bg-muted p-4 text-sm">
          <code>{cliCommand(from.id, to.id, from.extensions[0] ?? "txt")}</code>
        </pre>
        {(from.id === "ariad-ir+json" || to.id === "ariad-ir+json") && (
          <p className="text-sm text-muted-foreground">
            IR JSON goes through the CLI's pipeline commands, which read or write one step of a
            route.
          </p>
        )}
      </section>

      {related.length > 0 && (
        <nav aria-labelledby="related-heading" className="flex flex-col gap-3">
          <h2 id="related-heading" className="text-lg font-semibold">
            Related conversions
          </h2>
          <ul className="flex flex-wrap gap-2">
            {related.map((other) => (
              <li key={pairSlug(other.from.id, other.to.id)}>
                <Link
                  className={buttonVariants({ variant: "secondary", size: "sm" })}
                  href={`/convert/${pairSlug(other.from.id, other.to.id)}`}
                >
                  {other.from.label} to {other.to.label}
                </Link>
              </li>
            ))}
          </ul>
        </nav>
      )}
    </article>
  );
}
