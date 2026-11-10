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
  CloudIcon,
  DownloadIcon,
  HardDriveIcon,
  LoaderCircleIcon,
  RotateCcwIcon,
  ShieldCheckIcon,
  TriangleAlertIcon,
} from "lucide-react";
import { useLocale, useTranslations } from "next-intl";
import {
  Component,
  type ComponentType,
  lazy,
  type ReactNode,
  type RefObject,
  Suspense,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import type { CloudPanelProps } from "@/components/cloud/cloud-panel";
import { type Destination, DestinationChoice } from "@/components/cloud/destination-choice";
import { ServerCopyNotice } from "@/components/cloud/server-copy-notice";
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
import { useLocalePath } from "@/i18n/use-locale-path";
import type { DiscardOutcome, ServerCopy } from "@/lib/cloud/cloud-job";
import { CLOUD } from "@/lib/cloud/config";
import { formatBytes } from "@/lib/format-bytes";
import { ACCEPT, desktopLink, formatLabel } from "@/lib/formats";
import { isProfile, PROFILES } from "@/lib/profiles";

type Phase = "idle" | "ingesting" | "ready" | "converting" | "done";

// Cloud mode loads only when the user picks the cloud for a file, so local
// conversion never downloads the upload code and keeps working offline.
const CloudPanel = lazy(() => import("@/components/cloud/cloud-panel"));

interface CloudErrorBoundaryProps {
  transferred: RefObject<boolean>;
  onUseDevice: () => void;
  children: ReactNode;
}

interface CloudErrorBoundaryState {
  hasError: boolean;
}

class CloudErrorBoundary extends Component<CloudErrorBoundaryProps, CloudErrorBoundaryState> {
  override state: CloudErrorBoundaryState = { hasError: false };

  static getDerivedStateFromError(): CloudErrorBoundaryState {
    return { hasError: true };
  }

  override render() {
    if (this.state.hasError) {
      return (
        <CloudLoadError
          transferred={this.props.transferred.current}
          onUseDevice={this.props.onUseDevice}
        />
      );
    }
    return this.props.children;
  }
}

function CloudLoadError({
  transferred,
  onUseDevice,
}: {
  transferred: boolean;
  onUseDevice: () => void;
}) {
  const t = useTranslations("web.converter");
  return (
    <Alert variant="destructive" data-testid="cloud-load-error">
      <TriangleAlertIcon aria-hidden="true" />
      <AlertTitle>
        {t(transferred ? "cloud_error.stopped_title" : "cloud_error.load_title")}
      </AlertTitle>
      <AlertDescription className="flex flex-col items-start gap-3">
        <p>{t(transferred ? "cloud_error.stopped_body" : "cloud_error.load_body")}</p>
        <Button variant="outline" onClick={onUseDevice}>
          {t("cloud_error.use_device")}
        </Button>
      </AlertDescription>
    </Alert>
  );
}

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

const COUNT_KEYS = ["paragraphs", "tables", "images", "links", "footnotes", "words"] as const;

function formatMilliseconds(ms: number, locale: string): string {
  return new Intl.NumberFormat(locale, {
    style: "unit",
    unit: "millisecond",
    unitDisplay: "short",
    maximumFractionDigits: 0,
  }).format(Math.max(1, Math.round(ms)));
}

function listFormats(formats: string[], locale: string): string {
  return new Intl.ListFormat(locale, { style: "short", type: "unit" }).format(
    formats.map(formatLabel),
  );
}

export interface ConverterAppProps {
  client?: ConverterClient;
  cloudPanel?: ComponentType<CloudPanelProps>;
}

export function ConverterApp({
  client: customClient,
  cloudPanel: CloudPanelComponent = CloudPanel,
}: ConverterAppProps = {}) {
  const t = useTranslations("web.converter");
  const tc = useTranslations("common");
  const locale = useLocale();
  const path = useLocalePath();
  const clientRef = useRef<ConverterClient | null>(customClient ?? null);
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
  // The picked file itself, for cloud mode; local mode streams it into the worker.
  const [staged, setStaged] = useState<File | null>(null);
  const [destination, setDestination] = useState<Destination>("device");
  const cloudCleanup = useRef<(() => Promise<DiscardOutcome>) | null>(null);
  const cloudTransferred = useRef(false);
  const cloudPanelRef = useRef<HTMLDivElement>(null);
  const cloudHost = CLOUD?.apiHost;
  // Server copies that "Start over" or leaving cloud mode could not delete; each
  // stays listed with its retention time and a retry until the server confirms.
  const [serverCopies, setServerCopies] = useState<ServerCopy[]>([]);
  const keepServerCopy = useCallback((copy: ServerCopy) => {
    setServerCopies((copies) => [...copies.filter((item) => item.id !== copy.id), copy]);
  }, []);
  const settleServerCopy = useCallback(
    (copy: ServerCopy, outcome: DiscardOutcome) => {
      setServerCopies((copies) => copies.filter((item) => item.id !== copy.id));
      if (outcome.status === "kept") {
        keepServerCopy(outcome);
      } else {
        setStatus(t("status.deleted_from_server", { name: copy.fileName }));
      }
    },
    [keepServerCopy, t],
  );

  const targetLabelId = useId();
  const profileLabelId = useId();

  const client = useCallback(() => {
    if (customClient) {
      return customClient;
    }
    clientRef.current ??= new ConverterClient();
    return clientRef.current;
  }, [customClient]);

  useEffect(() => client().onProgress(setStatus), [client]);

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
      if (!customClient) {
        clientRef.current?.terminate();
      }
      clientRef.current = null;
      initRef.current = null;
    };
  }, [customClient, start]);

  useEffect(() => {
    const preventFileNavigation = (event: DragEvent) => {
      if (event.dataTransfer?.types.includes("Files")) {
        event.preventDefault();
      }
    };
    window.addEventListener("dragover", preventFileNavigation);
    window.addEventListener("drop", preventFileNavigation);
    return () => {
      window.removeEventListener("dragover", preventFileNavigation);
      window.removeEventListener("drop", preventFileNavigation);
    };
  }, []);

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
      // Every file starts on this device; the cloud is chosen per file.
      cloudTransferred.current = false;
      setStaged(CLOUD ? file : null);
      setDestination("device");
      setPhase("ingesting");
      setStatus(t("status.reading", { name: file.name }));
      try {
        await start();
        const result = await client().request({ type: "ingest", file });
        const reachable = result.inspection.reachable.length
          ? result.inspection.reachable
          : result.inspection.reachable_elsewhere;
        const wanted = preselected.current.to;
        const preferred = result.inspection.format === "html" ? "markdown" : "html";
        setInput(result);
        setTarget(
          wanted && reachable.includes(wanted)
            ? wanted
            : reachable.includes(preferred)
              ? preferred
              : (reachable.find((format) => format !== "ariad-ir+json") ?? reachable[0] ?? null),
        );
        setPhase("ready");
        setStatus(
          t("status.detected", { name: file.name, format: formatLabel(result.inspection.format) }),
        );
      } catch (cause) {
        setInput(null);
        setTarget(null);
        setError(asError(cause));
        setPhase("idle");
        setStatus(t("status.read_failed"));
      }
    },
    [client, start, t],
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
    setOutput(null);
    outputRef.current = null;
    setPhase("converting");
    setStatus(t("status.converting", { format: formatLabel(target) }));
    try {
      const { location, ...summary } = await client().request({
        type: "convert",
        to: target,
        profile,
      });
      outputRef.current = location;
      setOutput(summary);
      void enableOffline(summary.resources ?? []);
      setPhase("done");
      setStatus(t("status.ready", { name: summary.name }));
    } catch (cause) {
      if (asError(cause).code === "cancelled") return;
      const failure = {
        ...asError(cause),
        format: input?.inspection.format,
        targets: input?.inspection.reachable,
      };
      setError(failure);
      setPhase(failure.fatal ? "idle" : "ready");
      if (failure.fatal) {
        setInput(null);
        initRef.current = null;
      }
      setStatus(t("status.convert_failed"));
    }
  }, [client, input, profile, target, t]);

  const download = useCallback(async () => {
    if (!output || !outputRef.current) {
      return;
    }
    try {
      await downloadOutput(outputRef.current, output.name);
      setStatus(t("status.saved", { name: output.name }));
    } catch (cause) {
      setError(asError(cause));
    }
  }, [output, t]);

  const reset = useCallback(async () => {
    if (phase === "converting") {
      client().cancel();
      initRef.current = null;
    }
    planRequest.current += 1;
    outputRef.current = null;
    setInput(null);
    setTarget(null);
    setRoute(null);
    setRouteError(null);
    setOutput(null);
    setError(null);
    setPhase("idle");
    const cloudFiles = cloudCleanup.current;
    cloudTransferred.current = false;
    setStaged(null);
    setDestination("device");
    try {
      // Queue device cleanup before another file can be selected.
      const [fromServer] = await Promise.all([
        (cloudFiles?.() ?? Promise.resolve<DiscardOutcome>({ status: "none" })).then((outcome) => {
          if (outcome.status === "kept") keepServerCopy(outcome);
          return outcome;
        }),
        client().request({ type: "reset" }),
      ]);
      setStatus(
        t("status.reset", {
          outcome: fromServer.status,
          name: fromServer.status === "kept" ? fromServer.fileName : "",
        }),
      );
    } catch (cause) {
      setError(asError(cause));
    }
  }, [client, keepServerCopy, phase, t]);

  const busy = phase === "ingesting" || phase === "converting";
  const inspection = input?.inspection;
  const handoffFormat =
    inspection?.format ?? (typeof error?.format === "string" ? error.format : null);
  const handoffTargets =
    inspection?.reachable_elsewhere ??
    (Array.isArray(error?.targets) ? (error.targets as string[]) : []);
  const needsDesktop =
    !!handoffFormat &&
    (inspection?.reachable.length === 0 ||
      inspection?.warnings.some((warning) => warning.code === "ocr_required") ||
      output?.warnings.some((warning) => warning.code === "ocr_required") ||
      error?.code === "limit_exceeded" ||
      error?.code === "engine_unavailable" ||
      error?.fatal);
  const desktopTarget =
    target ??
    (preselected.current.to && handoffTargets.includes(preselected.current.to)
      ? preselected.current.to
      : null) ??
    handoffTargets.find((id) => id === "html") ??
    handoffTargets.find((id) => id === "markdown") ??
    handoffTargets[0];
  const desktopUrl =
    handoffFormat && desktopTarget
      ? desktopLink(handoffFormat, desktopTarget, profile)
      : path("/download");
  const onCloud = destination === "cloud" && !!CLOUD && !!staged;
  const chooseCloud = () => {
    setDestination("cloud");
    setStatus(
      staged
        ? t("status.cloud_selected", { name: staged.name })
        : t("status.cloud_selected_unnamed"),
    );
    queueMicrotask(() => cloudPanelRef.current?.scrollIntoView({ block: "nearest" }));
  };
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
        {onCloud ? (
          <>
            <CloudIcon aria-hidden="true" className="size-5 shrink-0" />
            <p className="text-sm">
              {t.rich("device.cloud", {
                strong: (chunks) => <strong className="font-semibold">{chunks}</strong>,
              })}
            </p>
          </>
        ) : (
          <>
            <ShieldCheckIcon aria-hidden="true" className="size-5 shrink-0" />
            <p className="text-sm">
              {t.rich("device.local", {
                strong: (chunks) => <strong className="font-semibold">{chunks}</strong>,
              })}
            </p>
          </>
        )}
        {engine && (
          <Badge variant="outline" className="ml-auto">
            <HardDriveIcon aria-hidden="true" />
            {t("storage_badge", { storage: engine.storage })}
          </Badge>
        )}
      </div>

      {engine?.storage === "memory" && (
        <Alert>
          <TriangleAlertIcon aria-hidden="true" />
          <AlertTitle>{t("memory.title")}</AlertTitle>
          <AlertDescription>
            {t("memory.body", {
              limit: formatBytes(Math.min(engine.memoryLimitBytes, engine.inputLimitBytes), locale),
            })}
          </AlertDescription>
        </Alert>
      )}

      {cloudHost &&
        serverCopies.map((copy) => (
          <ServerCopyNotice
            key={copy.id}
            copy={copy}
            host={cloudHost}
            onSettled={settleServerCopy}
          />
        ))}

      {error && !onCloud && (
        <Alert variant="destructive" data-testid="converter-error">
          <TriangleAlertIcon aria-hidden="true" />
          <AlertTitle>{t("error.title", { code: error.code })}</AlertTitle>
          <AlertDescription>{error.message}</AlertDescription>
        </Alert>
      )}

      {needsDesktop && !onCloud && (
        <Alert data-testid="desktop-handoff">
          <HardDriveIcon aria-hidden="true" />
          <AlertTitle>{t("handoff.title")}</AlertTitle>
          <AlertDescription>
            <p>
              {t("handoff.reason", {
                reason:
                  inspection?.warnings.some((warning) => warning.code === "ocr_required") ||
                  output?.warnings.some((warning) => warning.code === "ocr_required")
                    ? "ocr"
                    : "engine",
              })}
            </p>
            <p>{t("handoff.picker")}</p>
            <a className="underline font-semibold" href={desktopUrl}>
              {t("handoff.open")}
            </a>
            {CLOUD && staged && (
              <p>
                {t("handoff.cloud_alternative", { host: CLOUD.apiHost })}{" "}
                <Button variant="outline" size="lg" className="mt-2" onClick={chooseCloud}>
                  <CloudIcon aria-hidden="true" />
                  {t("handoff.cloud_button")}
                </Button>
              </p>
            )}
          </AlertDescription>
        </Alert>
      )}

      {!input ? (
        <FileDropZone
          onFiles={handleFiles}
          accept={ACCEPT}
          disabled={busy}
          buttonLabel={t(busy ? "drop.button_busy" : "drop.button")}
          label={t("drop.label")}
        >
          <div className="flex flex-col gap-1">
            <p className="text-lg font-semibold">{t("drop.title")}</p>
            <p className="text-sm text-muted-foreground">
              {t("drop.hint", {
                limit: formatBytes(engine?.inputLimitBytes ?? 16 * 1024 * 1024, locale),
              })}
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
              {t.rich(inspection?.meta?.title ? "input.summary_titled" : "input.summary", {
                size: formatBytes(input.size, locale),
                format: formatLabel(inspection?.format ?? ""),
                title: inspection?.meta?.title ?? "",
                detected: (chunks) => <span data-testid="detected-format">{chunks}</span>,
              })}
            </CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            {inspection?.counts && (
              <dl className="grid grid-cols-2 gap-3 text-sm sm:grid-cols-4" data-testid="counts">
                <div>
                  <dt className="text-muted-foreground">{t("count.headings")}</dt>
                  <dd className="font-semibold">{headingCount}</dd>
                </div>
                {COUNT_KEYS.map((key) => (
                  <div key={key}>
                    <dt className="text-muted-foreground">{t(`count.${key}`)}</dt>
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
                {(inspection.reachable_elsewhere?.length ?? 0) > 0
                  ? t.rich("unreachable.cli", {
                      format: formatLabel(inspection.format),
                      link: (chunks) => (
                        <a className="underline" href={path("/download")}>
                          {chunks}
                        </a>
                      ),
                    })
                  : t("unreachable.none", { format: formatLabel(inspection.format) })}
              </p>
            )}
          </CardContent>
        </Card>
      )}

      {CLOUD && staged && (
        <DestinationChoice
          value={destination}
          onChange={(value) => (value === "cloud" ? chooseCloud() : setDestination("device"))}
          host={CLOUD.apiHost}
          disabled={busy}
        />
      )}

      {onCloud && CLOUD && staged && (
        <div ref={cloudPanelRef}>
          <CloudErrorBoundary
            transferred={cloudTransferred}
            onUseDevice={() => setDestination("device")}
          >
            <Suspense
              fallback={<p className="text-sm text-muted-foreground">{t("cloud.loading")}</p>}
            >
              <CloudPanelComponent
                config={CLOUD}
                file={staged}
                sourceFormat={inspection?.format ?? null}
                preferredTarget={target ?? preselected.current.to}
                profile={profile}
                onUseDevice={() => setDestination("device")}
                cleanup={cloudCleanup}
                onServerCopy={keepServerCopy}
                onTransferStart={() => {
                  cloudTransferred.current = true;
                }}
              />
            </Suspense>
          </CloudErrorBoundary>
        </div>
      )}

      {!onCloud && input && inspection && inspection.reachable.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>
              <h2 className="text-lg">{t("convert.title")}</h2>
            </CardTitle>
            <CardDescription>{t("convert.hint")}</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-5">
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="flex flex-col gap-2">
                <Label id={targetLabelId}>{t("convert.target_label")}</Label>
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
                <Label id={profileLabelId}>{t("convert.profile_label")}</Label>
                <Select
                  value={profile}
                  onValueChange={(value) => setProfile(value as ProfileId)}
                  items={PROFILES.map((item) => ({
                    value: item.id,
                    label: tc(`profile.${item.id}.label`),
                  }))}
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
                        {tc(`profile.${item.id}.label`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <p className="text-sm text-muted-foreground">
                  {tc(`profile.${profile}.description`)}
                </p>
              </div>
            </div>

            {route && (
              <section aria-labelledby="route-heading" data-testid="route">
                <h3 id="route-heading" className="mb-2 text-sm font-semibold">
                  {t("convert.route_heading")}
                </h3>
                <RouteSummary route={route} />
              </section>
            )}
            {routeError && <p className="text-sm text-destructive">{routeError.message}</p>}

            {inspection.reachable_elsewhere.length > 0 && (
              <p className="text-sm text-muted-foreground">
                {t.rich("convert.elsewhere", {
                  formats: listFormats(inspection.reachable_elsewhere, locale),
                  count: inspection.reachable_elsewhere.length,
                  link: (chunks) => (
                    <a className="underline" href={path("/download")}>
                      {chunks}
                    </a>
                  ),
                })}
              </p>
            )}

            <div className="flex flex-wrap gap-3">
              <Button size="lg" onClick={convert} disabled={busy || !route}>
                {phase === "converting" && (
                  <LoaderCircleIcon aria-hidden="true" className="animate-spin" />
                )}
                {t(phase === "converting" ? "convert.button_busy" : "convert.button")}
              </Button>
              {phase === "converting" && (
                <Button variant="outline" onClick={reset}>
                  {t("convert.cancel")}
                </Button>
              )}
            </div>
          </CardContent>
        </Card>
      )}

      {phase === "converting" && (
        <p className="text-sm text-muted-foreground" data-testid="engine-progress">
          {status}
        </p>
      )}

      {output && (
        <Card data-testid="result">
          <CardHeader>
            <CardTitle>
              <h2 className="text-lg">{t("result.title")}</h2>
            </CardTitle>
            <CardDescription>
              {t.rich("result.summary", {
                name: output.name,
                size: formatBytes(output.size, locale),
                duration: formatMilliseconds(output.elapsedMs, locale),
                output: (chunks) => <span data-testid="output-name">{chunks}</span>,
              })}
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
                {t("result.download", { name: output.name })}
              </Button>
            </div>
          </CardContent>
        </Card>
      )}

      {(input || output || error) && (
        <div>
          <Button variant="outline" onClick={reset} disabled={phase === "ingesting"}>
            <RotateCcwIcon aria-hidden="true" />
            {t("start_over")}
          </Button>
        </div>
      )}
    </div>
  );
}
