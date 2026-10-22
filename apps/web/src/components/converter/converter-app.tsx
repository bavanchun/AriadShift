"use client";

import type { FormatId, Plan, ProfileId } from "@ariadshift/ariad-wasm";
import { Alert, AlertDescription, AlertTitle } from "@ariadshift/ui/components/alert";
import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@ariadshift/ui/components/card";
import { FileDropZone } from "@ariadshift/ui/components/file-drop-zone";
import { Label } from "@ariadshift/ui/components/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@ariadshift/ui/components/select";
import {
  DownloadIcon,
  HardDriveIcon,
  LoaderCircleIcon,
  RotateCcwIcon,
  ShieldCheckIcon,
  TriangleAlertIcon,
} from "lucide-react";
import Link from "next/link";
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { RouteSummary } from "@/components/route-summary";
import { ConverterClient, downloadOutput } from "@/converter/client";
import { enableOffline } from "@/converter/offline";
import type {
  ConvertedFile,
  ConverterError,
  IngestResult,
  InitResult,
  OutputLocation,
} from "@/converter/protocol";
import { formatBytes } from "@/lib/format-bytes";
import { ACCEPT, formatLabel } from "@/lib/formats";
import { isProfile, PROFILES } from "@/lib/profiles";

type Phase = "idle" | "ingesting" | "ready" | "converting" | "done";

type ConvertedSummary = Omit<ConvertedFile, "location">;

function asError(error: unknown): ConverterError {
  if (typeof error === "object" && error !== null && "message" in error) {
    return error as ConverterError;
  }
  return { code: "internal_error", message: String(error) };
}

function whenIdle(run: () => void): () => void {
  if ("requestIdleCallback" in window) {
    const handle = window.requestIdleCallback(run, { timeout: 3000 });
    return () => window.cancelIdleCallback(handle);
  }
  const handle = setTimeout(run, 1000);
  return () => clearTimeout(handle);
}

const COUNT_LABELS: Array<[keyof NonNullable<IngestResult["inspection"]["counts"]>, string]> = [
  ["paragraphs", "Paragraphs"],
  ["tables", "Tables"],
  ["images", "Images"],
  ["links", "Links"],
  ["footnotes", "Footnotes"],
  ["words", "Words"],
];

export function ConverterApp() {
  const clientRef = useRef<ConverterClient | null>(null);
  const outputRef = useRef<OutputLocation | null>(null);
  const initRef = useRef<Promise<InitResult> | null>(null);
  const planRequest = useRef(0);
  const preselected = useRef<{ to: string | null; profile: string | null }>({
    to: null,
    profile: null,
  });

  const [engine, setEngine] = useState<InitResult | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  const [input, setInput] = useState<IngestResult | null>(null);
  const [target, setTarget] = useState<FormatId | null>(null);
  const [profile, setProfile] = useState<ProfileId>("editable");
  const [route, setRoute] = useState<Plan | null>(null);
  const [routeError, setRouteError] = useState<ConverterError | null>(null);
  const [output, setOutput] = useState<ConvertedSummary | null>(null);
  const [error, setError] = useState<ConverterError | null>(null);
  const [status, setStatus] = useState("");

  const targetLabelId = useId();
  const profileLabelId = useId();

  const client = useCallback(() => {
    clientRef.current ??= new ConverterClient();
    return clientRef.current;
  }, []);

  const start = useCallback(() => {
    initRef.current ??= client()
      .request({ type: "init" })
      .then((info) => {
        setEngine(info);
        void enableOffline(info.resources);
        return info;
      })
      .catch((cause) => {
        initRef.current = null;
        throw cause;
      });
    return initRef.current;
  }, [client]);

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    preselected.current = { to: params.get("to"), profile: params.get("profile") };
    const requested = params.get("profile");
    if (isProfile(requested)) {
      setProfile(requested);
    }
    // Load the engine once the page is idle, so it is ready (and cached for offline use)
    // before the first file arrives without competing with the page itself.
    let cancel: (() => void) | undefined;
    const schedule = () => {
      cancel = whenIdle(() => void start().catch(() => undefined));
    };
    if (document.readyState === "complete") {
      schedule();
    } else {
      window.addEventListener("load", schedule, { once: true });
    }
    return () => {
      window.removeEventListener("load", schedule);
      cancel?.();
      clientRef.current?.terminate();
      clientRef.current = null;
      initRef.current = null;
    };
  }, [start]);

  const handleFiles = useCallback(
    async (files: File[]) => {
      const file = files[0];
      if (!file) {
        return;
      }
      setError(null);
      setOutput(null);
      setRoute(null);
      setRouteError(null);
      outputRef.current = null;
      setPhase("ingesting");
      setStatus(`Reading ${file.name} on this device.`);
      try {
        await start();
        const result = await client().request({ type: "ingest", file });
        const reachable = result.inspection.reachable;
        const wanted = preselected.current.to;
        setInput(result);
        setTarget(
          wanted && reachable.includes(wanted)
            ? wanted
            : (reachable.find((format) => format !== "ariad-ir+json") ?? reachable[0] ?? null),
        );
        setPhase("ready");
        setStatus(`${file.name} is ${formatLabel(result.inspection.format)}.`);
      } catch (cause) {
        setInput(null);
        setTarget(null);
        setError(asError(cause));
        setPhase("idle");
        setStatus("The file could not be read.");
      }
    },
    [client, start],
  );

  useEffect(() => {
    if (!input || !target) {
      setRoute(null);
      return;
    }
    const request = ++planRequest.current;
    client()
      .request({ type: "plan", to: target, profile })
      .then((plan) => {
        if (request === planRequest.current) {
          setRoute(plan);
          setRouteError(null);
        }
      })
      .catch((cause) => {
        if (request === planRequest.current) {
          setRoute(null);
          setRouteError(asError(cause));
        }
      });
  }, [client, input, target, profile]);

  const convert = useCallback(async () => {
    if (!target) {
      return;
    }
    setError(null);
    setPhase("converting");
    setStatus(`Converting to ${formatLabel(target)} on this device.`);
    try {
      const { location, ...summary } = await client().request({
        type: "convert",
        to: target,
        profile,
      });
      outputRef.current = location;
      setOutput(summary);
      setPhase("done");
      setStatus(`${summary.name} is ready to download.`);
    } catch (cause) {
      const failure = asError(cause);
      setError(failure);
      setPhase(failure.fatal ? "idle" : "ready");
      if (failure.fatal) {
        setInput(null);
        initRef.current = null;
      }
      setStatus("The conversion failed.");
    }
  }, [client, profile, target]);

  const download = useCallback(async () => {
    if (!output || !outputRef.current) {
      return;
    }
    try {
      await downloadOutput(outputRef.current, output.name);
      setStatus(`Saved ${output.name}.`);
    } catch (cause) {
      setError(asError(cause));
    }
  }, [output]);

  const reset = useCallback(async () => {
    planRequest.current += 1;
    outputRef.current = null;
    setInput(null);
    setTarget(null);
    setRoute(null);
    setRouteError(null);
    setOutput(null);
    setError(null);
    setPhase("idle");
    try {
      if (initRef.current) {
        await client().request({ type: "reset" });
      }
      setStatus("Files were removed from this device.");
    } catch (cause) {
      setError(asError(cause));
    }
  }, [client]);

  const busy = phase === "ingesting" || phase === "converting";
  const inspection = input?.inspection;
  const headingCount = inspection?.counts
    ? Object.values(inspection.counts.headings).reduce((sum, count) => sum + count, 0)
    : 0;

  return (
    <div className="flex flex-col gap-6">
      <p className="sr-only" role="status" aria-live="polite">
        {status}
      </p>

      <div
        className="flex flex-wrap items-center gap-3 rounded-xl border border-border bg-secondary px-4 py-3 text-secondary-foreground"
        data-testid="device-indicator"
      >
        <ShieldCheckIcon aria-hidden="true" className="size-5 shrink-0" />
        <p className="text-sm">
          <strong className="font-semibold">Processed on this device.</strong> Files never leave
          your browser and are not uploaded.
        </p>
        {engine && (
          <Badge variant="outline" className="ml-auto">
            <HardDriveIcon aria-hidden="true" />
            {engine.storage === "opfs" ? "Private browser storage" : "In memory"}
          </Badge>
        )}
      </div>

      {engine?.storage === "memory" && (
        <Alert>
          <TriangleAlertIcon aria-hidden="true" />
          <AlertTitle>On-device file storage is not available in this browser</AlertTitle>
          <AlertDescription>
            Files are kept in memory instead, so inputs up to {formatBytes(engine.memoryLimitBytes)}{" "}
            are supported. Nothing is uploaded either way.
          </AlertDescription>
        </Alert>
      )}

      {error && (
        <Alert variant="destructive" data-testid="converter-error">
          <TriangleAlertIcon aria-hidden="true" />
          <AlertTitle>
            {error.code === "no_route" ? "Not available here" : "Something went wrong"}
          </AlertTitle>
          <AlertDescription>{error.message}</AlertDescription>
        </Alert>
      )}

      {!input ? (
        <FileDropZone
          onFiles={handleFiles}
          accept={ACCEPT}
          disabled={busy}
          buttonLabel={busy ? "Reading…" : "Choose a file"}
          label="Document to convert"
        >
          <div className="flex flex-col gap-1">
            <p className="text-lg font-semibold">Drop a document here</p>
            <p className="text-sm text-muted-foreground">
              Markdown, HTML or AriadShift IR JSON. It stays on this device.
            </p>
          </div>
        </FileDropZone>
      ) : (
        <Card>
          <CardHeader>
            <CardTitle>
              <h2 className="text-lg" data-testid="input-name">
                {input.name}
              </h2>
            </CardTitle>
            <CardDescription>
              {formatBytes(input.size)} ·{" "}
              <span data-testid="detected-format">{formatLabel(inspection?.format ?? "")}</span>
              {inspection?.meta?.title ? ` · “${inspection.meta.title}”` : ""}
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            {inspection?.counts && (
              <dl className="grid grid-cols-2 gap-3 text-sm sm:grid-cols-4" data-testid="counts">
                <div>
                  <dt className="text-muted-foreground">Headings</dt>
                  <dd className="font-semibold">{headingCount}</dd>
                </div>
                {COUNT_LABELS.map(([key, label]) => (
                  <div key={key}>
                    <dt className="text-muted-foreground">{label}</dt>
                    <dd className="font-semibold">{String(inspection.counts?.[key] ?? 0)}</dd>
                  </div>
                ))}
              </dl>
            )}
            {inspection && inspection.warnings.length > 0 && (
              <ul className="list-disc pl-5 text-sm text-muted-foreground">
                {inspection.warnings.map((warning, index) => (
                  <li key={index}>{warning.message}</li>
                ))}
              </ul>
            )}
            {inspection && inspection.reachable.length === 0 && (
              <p className="text-sm">
                {formatLabel(inspection.format)} cannot be converted in the browser yet.{" "}
                <Link className="underline" href="/download">
                  The AriadShift CLI
                </Link>{" "}
                converts it on your computer.
              </p>
            )}
          </CardContent>
        </Card>
      )}

      {input && inspection && inspection.reachable.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>
              <h2 className="text-lg">Convert</h2>
            </CardTitle>
            <CardDescription>
              The planner picks the route with the best measured score.
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-5">
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="flex flex-col gap-2">
                <Label id={targetLabelId}>Convert to</Label>
                <Select
                  value={target}
                  onValueChange={(value) => setTarget(value as FormatId)}
                  items={inspection.reachable.map((format) => ({
                    value: format,
                    label: formatLabel(format),
                  }))}
                  disabled={busy}
                >
                  <SelectTrigger
                    className="w-full"
                    aria-labelledby={targetLabelId}
                    data-testid="target-select"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {inspection.reachable.map((format) => (
                      <SelectItem key={format} value={format}>
                        {formatLabel(format)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="flex flex-col gap-2">
                <Label id={profileLabelId}>Profile</Label>
                <Select
                  value={profile}
                  onValueChange={(value) => setProfile(value as ProfileId)}
                  items={PROFILES.map((item) => ({ value: item.id, label: item.label }))}
                  disabled={busy}
                >
                  <SelectTrigger
                    className="w-full"
                    aria-labelledby={profileLabelId}
                    data-testid="profile-select"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {PROFILES.map((item) => (
                      <SelectItem key={item.id} value={item.id}>
                        {item.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <p className="text-sm text-muted-foreground">
                  {PROFILES.find((item) => item.id === profile)?.description}
                </p>
              </div>
            </div>

            {route && (
              <section aria-labelledby="route-heading" data-testid="route">
                <h3 id="route-heading" className="mb-2 text-sm font-semibold">
                  Why this route
                </h3>
                <RouteSummary route={route} />
              </section>
            )}
            {routeError && <p className="text-sm text-destructive">{routeError.message}</p>}

            {inspection.reachable_elsewhere.length > 0 && (
              <p className="text-sm text-muted-foreground">
                {inspection.reachable_elsewhere.map(formatLabel).join(", ")}{" "}
                {inspection.reachable_elsewhere.length === 1 ? "needs" : "need"} the{" "}
                <Link className="underline" href="/download">
                  AriadShift CLI
                </Link>{" "}
                for now.
              </p>
            )}

            <div className="flex flex-wrap gap-3">
              <Button size="lg" onClick={convert} disabled={busy || !route}>
                {phase === "converting" && (
                  <LoaderCircleIcon aria-hidden="true" className="animate-spin" />
                )}
                {phase === "converting" ? "Converting…" : "Convert"}
              </Button>
            </div>
          </CardContent>
        </Card>
      )}

      {output && (
        <Card data-testid="result">
          <CardHeader>
            <CardTitle>
              <h2 className="text-lg">Ready</h2>
            </CardTitle>
            <CardDescription>
              <span data-testid="output-name">{output.name}</span> · {formatBytes(output.size)} ·
              converted in {Math.max(1, Math.round(output.elapsedMs))} ms on this device
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            {output.warnings.length > 0 && (
              <ul className="list-disc pl-5 text-sm text-muted-foreground" data-testid="warnings">
                {output.warnings.map((warning, index) => (
                  <li key={index}>{warning.message}</li>
                ))}
              </ul>
            )}
            <div className="flex flex-wrap gap-3">
              <Button size="lg" onClick={download}>
                <DownloadIcon aria-hidden="true" />
                Download {output.name}
              </Button>
            </div>
          </CardContent>
        </Card>
      )}

      {(input || output || error) && (
        <div>
          <Button variant="outline" onClick={reset} disabled={busy}>
            <RotateCcwIcon aria-hidden="true" />
            Start over and delete files
          </Button>
        </div>
      )}
    </div>
  );
}
