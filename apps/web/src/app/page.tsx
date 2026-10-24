import { buttonVariants } from "@ariadshift/ui/components/button";
import { Card, CardContent, CardHeader, CardTitle } from "@ariadshift/ui/components/card";
import { BotIcon, GaugeIcon, ShieldCheckIcon } from "lucide-react";
import Link from "next/link";
import { loadPairs } from "@/lib/catalog";
import { pairSlug } from "@/lib/formats";

const DIFFERENTIATORS = [
  {
    icon: ShieldCheckIcon,
    title: "Private by default",
    body: "The converter runs inside your browser with WebAssembly. Your document is stored in the browser's private file system and never uploaded.",
  },
  {
    icon: GaugeIcon,
    title: "Measured quality",
    body: "Every route carries fidelity and editability scores from a public benchmark. The planner picks the best one and tells you why.",
  },
  {
    icon: BotIcon,
    title: "Ready for AI agents",
    body: "Turn documents into clean Markdown or structured JSON for LLM context. The CLI ships an MCP server, so agents can convert files themselves.",
  },
] as const;

export default function HomePage() {
  const browserPairs = loadPairs().filter((pair) => pair.browser !== null);
  return (
    <div className="flex flex-col gap-16">
      <section className="flex flex-col items-start gap-6 pt-6">
        <img src="/brand/ariadshift-app-icon.svg" alt="" width={88} height={88} />
        <h1 className="max-w-3xl text-4xl font-bold tracking-tight sm:text-5xl">
          Convert documents on your own device
        </h1>
        <p className="max-w-2xl text-lg text-muted-foreground">
          AriadShift converts Markdown, HTML, Word, EPUB and structured JSON right here in your
          browser. Read digital PDF text and create PDFs on your device. One measured planner, no
          uploads.
        </p>
        <div className="flex flex-wrap gap-3">
          <a href="/app" className={buttonVariants({ size: "lg" })}>
            Open the converter
          </a>
          <Link href="/download" className={buttonVariants({ size: "lg", variant: "secondary" })}>
            Install the CLI
          </Link>
        </div>
      </section>

      <section aria-labelledby="why-heading" className="flex flex-col gap-6">
        <h2 id="why-heading" className="text-2xl font-bold tracking-tight">
          Why AriadShift
        </h2>
        <div className="grid gap-4 md:grid-cols-3">
          {DIFFERENTIATORS.map(({ icon: Icon, title, body }) => (
            <Card key={title}>
              <CardHeader>
                <Icon aria-hidden="true" className="size-6 text-primary" />
                <CardTitle>
                  <h3 className="text-lg">{title}</h3>
                </CardTitle>
              </CardHeader>
              <CardContent>
                <p className="text-sm text-muted-foreground">{body}</p>
              </CardContent>
            </Card>
          ))}
        </div>
      </section>

      <section aria-labelledby="how-heading" className="flex flex-col gap-4">
        <h2 id="how-heading" className="text-2xl font-bold tracking-tight">
          How it works
        </h2>
        <ol className="grid gap-3 text-sm sm:grid-cols-4">
          {[
            "You pick a file. The browser copies it into its private file system.",
            "A background worker loads the AriadShift engine compiled to WebAssembly.",
            "The planner chooses the route with the best measured score for your goal.",
            "You download the result. Starting over deletes every stored file.",
          ].map((step, index) => (
            <li key={step} className="rounded-xl border border-border bg-card p-4">
              <span className="mb-2 block font-semibold text-primary">Step {index + 1}</span>
              {step}
            </li>
          ))}
        </ol>
      </section>

      <section aria-labelledby="pairs-heading" className="flex flex-col gap-4">
        <h2 id="pairs-heading" className="text-2xl font-bold tracking-tight">
          Convert in your browser
        </h2>
        <ul className="flex flex-wrap gap-3">
          {browserPairs.map(({ from, to }) => (
            <li key={pairSlug(from.id, to.id)}>
              <Link
                href={`/convert/${pairSlug(from.id, to.id)}`}
                className={buttonVariants({ variant: "secondary" })}
              >
                {from.label} to {to.label}
              </Link>
            </li>
          ))}
        </ul>
        <p className="text-sm text-muted-foreground">
          See every{" "}
          <Link className="underline" href="/formats">
            supported format
          </Link>{" "}
          and the{" "}
          <Link className="underline" href="/quality">
            measured quality
          </Link>{" "}
          of each route.
        </p>
      </section>
    </div>
  );
}
