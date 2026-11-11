"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@ariadshift/ui/components/tabs";
import { cn } from "@ariadshift/ui/lib/utils";
import {
  CheckIcon,
  CloudIcon,
  CopyIcon,
  DownloadIcon,
  FileTextIcon,
  LanguagesIcon,
  MessageSquareIcon,
  MonitorIcon,
  ShieldCheckIcon,
  SparklesIcon,
  TableIcon,
  WorkflowIcon,
} from "lucide-react";
import { type ReactNode, useMemo, useRef, useState } from "react";
import { validatePageRange } from "@/components/tools/params-form";
import { type AiPanelLabels, resolveAiLabels } from "./ai-labels";
import {
  type AiConnection,
  type AiParamsResult,
  buildAiCliHandoff,
  buildAskParams,
  buildExtractTablesParams,
  buildRecipeParams,
  buildSummarizeParams,
  buildTranslateParams,
  DEFAULT_MAX_CITATIONS,
  isAskOutput,
  isRecipeOutput,
  isSummarizeOutput,
  isTablesOutput,
  isTranslateOutput,
  parseNaturalLanguageRecipeFallback,
  safeArtifactHref,
  scrubRecipeSecrets,
  tablesToCsv,
} from "./ai-params";
import type {
  AiAskOutput,
  AiCitation,
  AiExtractTablesOutput,
  AiOutput,
  AiRecipeFromTextOutput,
  AiRequest,
  AiSummarizeOutput,
  AiToolId,
  AiTranslateOutput,
  RecipeDraft,
  SummaryLength,
  TableFormat,
  TranslateFormat,
} from "./ai-types";
import { CloudConsentDialog } from "./cloud-consent-dialog";

type TabKey = "summarize" | "ask" | "translate" | "extractTables" | "recipe";

const TAB_TOOL: Record<TabKey, AiToolId> = {
  summarize: "ai.summarize",
  ask: "ai.ask",
  translate: "ai.translate",
  extractTables: "ai.extract_tables",
  recipe: "ai.recipe_from_text",
};

const TAB_ICON: Record<TabKey, typeof SparklesIcon> = {
  summarize: FileTextIcon,
  ask: MessageSquareIcon,
  translate: LanguagesIcon,
  extractTables: TableIcon,
  recipe: WorkflowIcon,
};

const TAB_ORDER: TabKey[] = ["summarize", "ask", "translate", "extractTables", "recipe"];
const LANGUAGES = ["en", "vi", "fr", "de", "es", "pt", "it", "ru", "ja", "ko", "zh", "ar"];
const TRANSLATE_FORMATS: TranslateFormat[] = ["pdf", "docx", "html", "md", "json"];
const TABLE_FORMATS: TableFormat[] = ["csv", "xlsx", "json"];
const LENGTHS: SummaryLength[] = ["brief", "standard", "detailed"];

export interface AiPanelProps {
  /** Name of the open document, used to build the Desktop command. */
  inputName?: string;
  /** Page count of the open document, used to validate custom page ranges. */
  pageCount?: number;
  /**
   * True when a local model (WebGPU or WASM) is loaded in this browser. Without it the panel
   * hands the request to Desktop instead of pretending to run it.
   */
  localRuntime?: boolean;
  /** Executes a request. Receives a cloud request only after explicit consent. */
  onExecute?: (request: AiRequest) => Promise<AiOutput | undefined> | AiOutput | undefined;
  onJumpToPage?: (page: number, quote: string) => void;
  onInsertNote?: (text: string) => void;
  /** Receives a recipe draft for review in the Recipe Builder. It is never executed by the panel. */
  onOpenRecipeDraft?: (draft: RecipeDraft) => void;
  /** Overrides how text is copied; defaults to the async clipboard. */
  copyText?: (text: string) => Promise<void>;
  /** Overrides how a generated file is saved; defaults to an anchor download. */
  saveFile?: (name: string, blob: Blob) => void;
  /** Locale used to name the languages in the selectors. */
  locale?: string;
  defaultTab?: TabKey;
  labels?: Partial<AiPanelLabels>;
  className?: string;
}

interface AskTurn {
  question: string;
  output: AiAskOutput;
}

function defaultSave(name: string, blob: Blob) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  anchor.click();
  URL.revokeObjectURL(url);
}

function languageName(code: string, locale: string): string {
  try {
    return new Intl.DisplayNames([locale], { type: "language" }).of(code) ?? code;
  } catch {
    return code;
  }
}

function Field({ label, children, error }: { label: string; children: ReactNode; error?: string }) {
  return (
    <div className="flex min-w-0 flex-col gap-1 text-sm">
      <span className="font-medium">{label}</span>
      {children}
      {error && (
        <span role="alert" className="text-xs text-destructive">
          {error}
        </span>
      )}
    </div>
  );
}

const CONTROL =
  "h-8 w-full min-w-0 rounded-lg border border-border bg-background px-2.5 text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50";

/** Offline-first AI side panel: summarise, ask, translate, extract tables, draft a recipe. */
export function AiPanel({
  inputName,
  pageCount,
  localRuntime = false,
  onExecute,
  onJumpToPage,
  onInsertNote,
  onOpenRecipeDraft,
  copyText = (text) => navigator.clipboard.writeText(text),
  saveFile = defaultSave,
  locale = "en",
  defaultTab = "summarize",
  labels: labelOverrides,
  className,
}: AiPanelProps) {
  const labels = useMemo(() => resolveAiLabels(labelOverrides), [labelOverrides]);
  const [tab, setTab] = useState<TabKey>(defaultTab);
  const [provider, setProvider] = useState<"local" | "cloud">("local");
  const [consent, setConsent] = useState<{ granted: boolean; apiKey?: string }>({ granted: false });
  const [dialogOpen, setDialogOpen] = useState(false);

  const [length, setLength] = useState<SummaryLength>("standard");
  const [language, setLanguage] = useState("");
  const [question, setQuestion] = useState("");
  const [maxCitations, setMaxCitations] = useState(DEFAULT_MAX_CITATIONS);
  const [targetLang, setTargetLang] = useState("vi");
  const [sourceLang, setSourceLang] = useState("");
  const [translateFormat, setTranslateFormat] = useState<TranslateFormat>("pdf");
  const [pagesMode, setPagesMode] = useState<"all" | "custom">("all");
  const [pages, setPages] = useState("");
  const [tableFormat, setTableFormat] = useState<TableFormat>("csv");
  const [prompt, setPrompt] = useState("");

  const [summary, setSummary] = useState<AiSummarizeOutput | null>(null);
  const [thread, setThread] = useState<AskTurn[]>([]);
  const [translation, setTranslation] = useState<AiTranslateOutput | null>(null);
  const [tables, setTables] = useState<AiExtractTablesOutput | null>(null);
  const [recipe, setRecipe] = useState<AiRecipeFromTextOutput | null>(null);
  const [recipeEmpty, setRecipeEmpty] = useState(false);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [copied, setCopied] = useState<string | null>(null);

  const connection: AiConnection = {
    provider,
    consent: consent.granted,
    apiKey: consent.apiKey,
  };

  const languageOptions = useMemo(
    () => LANGUAGES.map((code) => ({ code, name: languageName(code, locale) })),
    [locale],
  );

  function build(key: TabKey): AiParamsResult<unknown> & { tool: AiToolId } {
    const tool = TAB_TOOL[key];
    switch (key) {
      case "summarize":
        return { tool, ...buildSummarizeParams({ length, language }, connection) };
      case "ask":
        return { tool, ...buildAskParams({ question, maxCitations }, connection) };
      case "translate":
        return {
          tool,
          ...buildTranslateParams({ targetLang, sourceLang, format: translateFormat }, connection),
        };
      case "extractTables":
        return {
          tool,
          ...buildExtractTablesParams(
            { pages: pagesMode === "all" ? "all" : pages, format: tableFormat },
            connection,
            (value) => validatePageRange(value, pageCount),
          ),
        };
      case "recipe":
        return { tool, ...buildRecipeParams({ prompt }, connection) };
    }
  }

  const current = build(tab);
  const canRun = Boolean(onExecute) && (provider === "cloud" || localRuntime);
  const offlineRecipe = tab === "recipe" && provider === "local";
  const showHandoff = !canRun && !offlineRecipe;
  const handoff = useMemo(
    () =>
      current.isValid ? buildAiCliHandoff(current.tool, current.params as never, inputName) : null,
    [current.isValid, current.tool, current.params, inputName],
  );

  function revoke() {
    setConsent({ granted: false });
    setProvider("local");
  }

  function chooseProvider(next: "local" | "cloud") {
    if (next === "local") return revoke();
    if (consent.granted) return setProvider("cloud");
    setDialogOpen(true);
  }

  async function execute(key: TabKey) {
    if (busyRef.current) return;
    const built = build(key);
    setFailure(null);
    setErrors(built.errors);
    if (!built.isValid) return;

    // The gate: a cloud request never proceeds on a consent that is not currently granted.
    if (
      built.params &&
      (built.params as { provider?: string }).provider === "cloud" &&
      !consent.granted
    ) {
      setDialogOpen(true);
      return;
    }

    busyRef.current = true;
    setBusy(true);
    try {
      if (key === "recipe" && provider === "local") {
        const draft = parseNaturalLanguageRecipeFallback(prompt);
        setRecipeEmpty(draft === null);
        setRecipe(draft);
        return;
      }
      if (!onExecute) return;
      const output = await onExecute({ tool: built.tool, params: built.params } as AiRequest);
      accept(key, output);
    } catch (error) {
      setFailure(error instanceof Error && error.message ? error.message : labels.failed);
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }

  function accept(key: TabKey, output: AiOutput | undefined) {
    // A host that delivers the result itself, such as a cloud job that reports elsewhere, returns none.
    if (output === undefined) return;
    const mismatch = () => setFailure(labels.failed);
    if (key === "summarize") return isSummarizeOutput(output) ? setSummary(output) : mismatch();
    if (key === "ask")
      return isAskOutput(output)
        ? setThread((turns) => [...turns, { question: question.trim(), output }])
        : mismatch();
    if (key === "translate") return isTranslateOutput(output) ? setTranslation(output) : mismatch();
    if (key === "extractTables") return isTablesOutput(output) ? setTables(output) : mismatch();
    if (isRecipeOutput(output)) {
      setRecipeEmpty(false);
      setRecipe({ ...output, recipe: scrubRecipeSecrets(output.recipe) });
    } else mismatch();
  }

  async function runQuickCsv() {
    if (!onExecute || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setFailure(null);
    try {
      await onExecute({ tool: "pdf.tables_to_csv", params: {} });
    } catch (error) {
      setFailure(error instanceof Error && error.message ? error.message : labels.failed);
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }

  function confirmConsent({ apiKey }: { apiKey?: string }) {
    setConsent({ granted: true, apiKey });
    setProvider("cloud");
    setDialogOpen(false);
  }

  function cancelConsent() {
    setDialogOpen(false);
  }

  async function copy(id: string, text: string) {
    try {
      await copyText(text);
      setCopied(id);
    } catch {
      setFailure(labels.failed);
    }
  }

  function downloadCsv(output: AiExtractTablesOutput) {
    const body = output.tables
      .map((table) => `# ${labels.tablePage} ${table.page}\r\n${tablesToCsv(table.rows)}`)
      .join("\r\n\r\n");
    saveFile("tables.csv", new Blob([body], { type: "text/csv;charset=utf-8" }));
  }

  const runLabel = busy ? labels.running : labels.run;
  const operationName = labels.tabs[tab];

  const runButton = (key: TabKey, text = runLabel) =>
    canRun || (key === "recipe" && provider === "local") ? (
      <Button type="button" onClick={() => void execute(key)} disabled={busy}>
        <SparklesIcon aria-hidden="true" data-icon="inline-start" />
        {text}
      </Button>
    ) : null;

  const handoffCard = showHandoff && (
    <div
      data-slot="ai-handoff"
      className="flex flex-col gap-2 rounded-lg border border-border bg-bg-sunken p-3 text-sm"
    >
      <p className="m-0 flex items-center gap-1.5 font-medium">
        <MonitorIcon aria-hidden="true" className="size-4" />
        {labels.handoffTitle}
      </p>
      <p className="m-0 text-muted-foreground">{labels.handoffBody}</p>
      {handoff ? (
        <>
          <pre className="m-0 overflow-x-auto rounded-md border border-border bg-bg-surface p-2 text-xs whitespace-pre-wrap break-all">
            <code data-slot="ai-handoff-command">{handoff.command}</code>
          </pre>
          {handoff.readsApiKeyFromStdin && (
            <p className="m-0 text-xs text-muted-foreground">{labels.handoffKey}</p>
          )}
          <div>
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() => void copy("handoff", handoff.command)}
            >
              {copied === "handoff" ? (
                <CheckIcon aria-hidden="true" data-icon="inline-start" />
              ) : (
                <CopyIcon aria-hidden="true" data-icon="inline-start" />
              )}
              {copied === "handoff" ? labels.copied : labels.handoffCopy}
            </Button>
          </div>
        </>
      ) : (
        <p className="m-0 text-xs text-muted-foreground">{labels.invalid}</p>
      )}
    </div>
  );

  const failureNote = failure && (
    <p role="alert" className="m-0 text-sm text-destructive">
      {failure}
    </p>
  );

  function citationChip(citation: AiCitation, index: number) {
    return (
      <button
        key={`${citation.page}-${index}`}
        type="button"
        data-slot="ai-citation"
        aria-label={`${labels.citationJump} ${citation.page}`}
        onClick={() => onJumpToPage?.(citation.page, citation.quote)}
        className="flex w-full min-w-0 flex-col gap-0.5 rounded-lg border border-border bg-bg-surface p-2 text-start text-xs outline-none transition-colors hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50"
      >
        <span className="flex items-center gap-1.5">
          <Badge variant="secondary">p. {citation.page}</Badge>
          <Badge variant="outline">{Math.round(citation.score * 100)}%</Badge>
        </span>
        <q className="line-clamp-3 text-muted-foreground">{citation.quote}</q>
      </button>
    );
  }

  const artifactHref = translation ? safeArtifactHref(translation.artifact.href) : undefined;
  const tableArtifactHref = tables?.artifact ? safeArtifactHref(tables.artifact.href) : undefined;

  return (
    <aside
      data-slot="ai-panel"
      aria-label={labels.title}
      className={cn(
        "flex w-full min-w-0 flex-col gap-3 rounded-xl border border-border bg-bg-surface p-3 text-foreground",
        className,
      )}
    >
      <header className="flex items-center gap-2">
        <SparklesIcon aria-hidden="true" className="size-4 text-primary" />
        <h2 className="m-0 text-sm font-semibold">{labels.title}</h2>
      </header>

      <div
        role="status"
        data-slot="ai-privacy-badge"
        data-provider={provider}
        className="flex flex-wrap items-center gap-2 rounded-lg border border-border bg-bg-sunken px-2.5 py-1.5 text-xs"
      >
        {provider === "local" ? (
          <>
            <ShieldCheckIcon aria-hidden="true" className="size-3.5 text-success" />
            <Badge variant="secondary">{labels.badgeLocalDetail}</Badge>
            <span>{labels.badgeLocal}</span>
          </>
        ) : (
          <>
            <CloudIcon aria-hidden="true" className="size-3.5 text-warning" />
            <span>{labels.badgeCloud}</span>
            <Button type="button" size="xs" variant="outline" onClick={revoke}>
              {labels.revoke}
            </Button>
          </>
        )}
      </div>

      <div className="flex flex-col gap-1">
        <fieldset className="m-0 flex min-w-0 flex-col gap-1 border-0 p-0">
          <legend className="p-0 text-xs font-medium text-muted-foreground">
            {labels.providerLabel}
          </legend>
          <div className="flex gap-1">
            {(["local", "cloud"] as const).map((value) => (
              <button
                key={value}
                type="button"
                aria-pressed={provider === value}
                onClick={() => chooseProvider(value)}
                className={cn(
                  "inline-flex h-7 flex-1 items-center justify-center gap-1.5 rounded-md border border-border px-2 text-xs font-medium outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
                  provider === value
                    ? "bg-primary text-primary-foreground"
                    : "bg-background hover:bg-muted",
                )}
              >
                {value === "local" ? (
                  <MonitorIcon aria-hidden="true" className="size-3.5" />
                ) : (
                  <CloudIcon aria-hidden="true" className="size-3.5" />
                )}
                {value === "local" ? labels.providerLocal : labels.providerCloud}
              </button>
            ))}
          </div>
        </fieldset>
      </div>

      <Tabs value={tab} onValueChange={(value) => setTab(value as TabKey)}>
        <TabsList aria-label={labels.tablistLabel} className="w-full flex-wrap justify-start">
          {TAB_ORDER.map((key) => {
            const Icon = TAB_ICON[key];
            return (
              <TabsTrigger key={key} value={key}>
                <Icon aria-hidden="true" />
                {labels.tabs[key]}
              </TabsTrigger>
            );
          })}
        </TabsList>

        <TabsContent value="summarize" className="flex flex-col gap-3">
          <Field label={labels.length}>
            <select
              aria-label={labels.length}
              className={CONTROL}
              value={length}
              onChange={(event) => setLength(event.target.value as SummaryLength)}
            >
              {LENGTHS.map((value) => (
                <option key={value} value={value}>
                  {labels.lengths[value]}
                </option>
              ))}
            </select>
          </Field>
          <Field label={labels.outputLanguage}>
            <select
              aria-label={labels.outputLanguage}
              className={CONTROL}
              value={language}
              onChange={(event) => setLanguage(event.target.value)}
            >
              <option value="">{labels.autoLanguage}</option>
              {languageOptions.map((option) => (
                <option key={option.code} value={option.code}>
                  {option.name}
                </option>
              ))}
            </select>
          </Field>
          {runButton("summarize")}
          {handoffCard}
          {failureNote}
          {summary && (
            <article
              data-slot="ai-summary"
              className="flex flex-col gap-2 rounded-lg border border-border bg-background p-3 text-sm"
            >
              <h3 className="m-0 text-sm font-semibold">{labels.summaryHeading}</h3>
              <p className="m-0">{summary.summary}</p>
              {summary.bullet_points.length > 0 && (
                <ul className="m-0 flex list-disc flex-col gap-1 ps-5">
                  {summary.bullet_points.map((point) => (
                    <li key={point}>{point}</li>
                  ))}
                </ul>
              )}
              <div className="flex flex-wrap gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    void copy(
                      "summary",
                      [summary.summary, ...summary.bullet_points.map((p) => `- ${p}`)].join("\n"),
                    )
                  }
                >
                  <CopyIcon aria-hidden="true" data-icon="inline-start" />
                  {copied === "summary" ? labels.copied : labels.copy}
                </Button>
                {onInsertNote && (
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    onClick={() =>
                      onInsertNote(
                        [summary.summary, ...summary.bullet_points.map((p) => `- ${p}`)].join("\n"),
                      )
                    }
                  >
                    {labels.insertNote}
                  </Button>
                )}
              </div>
            </article>
          )}
        </TabsContent>

        <TabsContent value="ask" className="flex flex-col gap-3">
          <Field label={labels.question} error={errors.question && labels.invalid}>
            <input
              type="text"
              aria-label={labels.question}
              className={CONTROL}
              value={question}
              placeholder={labels.questionPlaceholder}
              onChange={(event) => setQuestion(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.nativeEvent.isComposing) {
                  event.preventDefault();
                  void execute("ask");
                }
              }}
            />
          </Field>
          <Field label={labels.maxCitations} error={errors.max_citations && labels.invalid}>
            <input
              type="number"
              aria-label={labels.maxCitations}
              className={CONTROL}
              min={1}
              max={20}
              step={1}
              value={Number.isNaN(maxCitations) ? "" : maxCitations}
              onChange={(event) => setMaxCitations(event.target.valueAsNumber)}
            />
          </Field>
          {runButton("ask", busy ? labels.running : labels.ask)}
          {handoffCard}
          {failureNote}
          <ol data-slot="ai-thread" className="m-0 flex list-none flex-col gap-3 p-0">
            {thread.map((turn, index) => (
              <li key={index} className="flex flex-col gap-2 text-sm">
                <p className="m-0 font-medium">{turn.question}</p>
                <div className="rounded-lg border border-border bg-background p-3">
                  <p className="m-0">{turn.output.answer}</p>
                </div>
                {turn.output.citations.length > 0 ? (
                  <div className="flex flex-col gap-1.5">
                    {turn.output.citations.map(citationChip)}
                  </div>
                ) : (
                  <p className="m-0 text-xs text-muted-foreground">{labels.noCitations}</p>
                )}
              </li>
            ))}
          </ol>
        </TabsContent>

        <TabsContent value="translate" className="flex flex-col gap-3">
          <Field label={labels.targetLanguage} error={errors.target_lang && labels.invalid}>
            <select
              aria-label={labels.targetLanguage}
              className={CONTROL}
              value={targetLang}
              onChange={(event) => setTargetLang(event.target.value)}
            >
              {languageOptions.map((option) => (
                <option key={option.code} value={option.code}>
                  {option.name}
                </option>
              ))}
            </select>
          </Field>
          <Field label={labels.sourceLanguage}>
            <select
              aria-label={labels.sourceLanguage}
              className={CONTROL}
              value={sourceLang}
              onChange={(event) => setSourceLang(event.target.value)}
            >
              <option value="">{labels.detectLanguage}</option>
              {languageOptions.map((option) => (
                <option key={option.code} value={option.code}>
                  {option.name}
                </option>
              ))}
            </select>
          </Field>
          <Field label={labels.outputFormat}>
            <select
              aria-label={labels.outputFormat}
              className={CONTROL}
              value={translateFormat}
              onChange={(event) => setTranslateFormat(event.target.value as TranslateFormat)}
            >
              {TRANSLATE_FORMATS.map((value) => (
                <option key={value} value={value}>
                  {value.toUpperCase()}
                </option>
              ))}
            </select>
          </Field>
          <p className="m-0 text-xs text-muted-foreground">{labels.translateNote}</p>
          {runButton("translate")}
          {handoffCard}
          {failureNote}
          {translation && (
            <div
              data-slot="ai-translation"
              className="flex items-center justify-between gap-2 rounded-lg border border-border bg-background p-3 text-sm"
            >
              <span>
                {labels.translateDone}: {translation.artifact.name}
              </span>
              {artifactHref && (
                <a
                  href={artifactHref}
                  download={translation.artifact.name}
                  className="inline-flex items-center gap-1 text-primary underline-offset-4 hover:underline"
                >
                  <DownloadIcon aria-hidden="true" className="size-4" />
                  {labels.downloadArtifact}
                </a>
              )}
            </div>
          )}
        </TabsContent>

        <TabsContent value="extractTables" className="flex flex-col gap-3">
          <Field label={labels.pages} error={errors.pages}>
            <select
              aria-label={labels.pages}
              className={CONTROL}
              value={pagesMode}
              onChange={(event) => setPagesMode(event.target.value as "all" | "custom")}
            >
              <option value="all">{labels.pagesAll}</option>
              <option value="custom">{labels.pagesCustom}</option>
            </select>
          </Field>
          {pagesMode === "custom" && (
            <input
              type="text"
              aria-label={labels.pagesCustom}
              className={CONTROL}
              value={pages}
              placeholder="1-3, 5"
              onChange={(event) => setPages(event.target.value)}
            />
          )}
          <Field label={labels.outputFormat}>
            <select
              aria-label={labels.outputFormat}
              className={CONTROL}
              value={tableFormat}
              onChange={(event) => setTableFormat(event.target.value as TableFormat)}
            >
              {TABLE_FORMATS.map((value) => (
                <option key={value} value={value}>
                  {value.toUpperCase()}
                </option>
              ))}
            </select>
          </Field>
          <div className="flex flex-wrap gap-2">
            {runButton("extractTables")}
            {onExecute && provider === "local" && (
              <Button
                type="button"
                variant="outline"
                onClick={() => void runQuickCsv()}
                disabled={busy}
              >
                <TableIcon aria-hidden="true" data-icon="inline-start" />
                {labels.quickCsv}
              </Button>
            )}
          </div>
          {handoffCard}
          {failureNote}
          {tables &&
            (tables.tables.length === 0 ? (
              <p className="m-0 text-sm text-muted-foreground">{labels.noTables}</p>
            ) : (
              <section data-slot="ai-tables" className="flex flex-col gap-3">
                <h3 className="m-0 text-sm font-semibold">{labels.tablesHeading}</h3>
                {tables.tables.map((table, index) => (
                  <figure key={index} className="m-0 flex flex-col gap-1">
                    <figcaption className="text-xs text-muted-foreground">
                      {labels.tablePage} {table.page}
                    </figcaption>
                    <div className="overflow-x-auto rounded-lg border border-border">
                      <table className="w-full border-collapse text-xs">
                        <tbody>
                          {table.rows.map((row, rowIndex) => (
                            <tr key={rowIndex} className="border-b border-border last:border-b-0">
                              {row.map((cell, cellIndex) => {
                                const Cell = rowIndex === 0 ? "th" : "td";
                                return (
                                  <Cell
                                    key={cellIndex}
                                    className="px-2 py-1 text-start font-normal"
                                  >
                                    {cell}
                                  </Cell>
                                );
                              })}
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  </figure>
                ))}
                <div className="flex flex-wrap gap-2">
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    onClick={() => downloadCsv(tables)}
                  >
                    <DownloadIcon aria-hidden="true" data-icon="inline-start" />
                    {labels.downloadCsv}
                  </Button>
                  {tableArtifactHref && tables.artifact && (
                    <a
                      href={tableArtifactHref}
                      download={tables.artifact.name}
                      className="inline-flex items-center gap-1 text-sm text-primary underline-offset-4 hover:underline"
                    >
                      <DownloadIcon aria-hidden="true" className="size-4" />
                      {tables.artifact.name}
                    </a>
                  )}
                </div>
              </section>
            ))}
        </TabsContent>

        <TabsContent value="recipe" className="flex flex-col gap-3">
          <Field label={labels.recipePrompt} error={errors.prompt && labels.invalid}>
            <textarea
              aria-label={labels.recipePrompt}
              className={cn(CONTROL, "h-20 py-1.5")}
              value={prompt}
              placeholder={labels.recipePlaceholder}
              onChange={(event) => setPrompt(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
                  event.preventDefault();
                  void execute("recipe");
                }
              }}
            />
          </Field>
          {runButton("recipe", busy ? labels.running : labels.recipeGenerate)}
          {failureNote}
          {recipeEmpty && <p className="m-0 text-sm text-muted-foreground">{labels.recipeNone}</p>}
          {recipe && (
            <section
              data-slot="ai-recipe"
              className="flex flex-col gap-2 rounded-lg border border-border bg-background p-3 text-sm"
            >
              <h3 className="m-0 text-sm font-semibold">{labels.recipeSteps}</h3>
              <ol className="m-0 flex list-none flex-col gap-1.5 p-0">
                {recipe.recipe.steps.map((step, index) => (
                  <li key={index} className="flex items-start gap-2">
                    <span
                      aria-hidden="true"
                      className="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full bg-thread text-xs font-semibold text-primary-foreground"
                    >
                      {index + 1}
                    </span>
                    <span className="min-w-0">
                      <code className="font-medium">{step.tool}</code>
                      {Object.keys(step.params).length > 0 && (
                        <code className="block text-xs break-all text-muted-foreground">
                          {JSON.stringify(step.params)}
                        </code>
                      )}
                    </span>
                  </li>
                ))}
              </ol>
              {recipe.unmatched && recipe.unmatched.length > 0 && (
                <p className="m-0 text-xs text-muted-foreground">
                  {labels.recipeUnmatched}: {recipe.unmatched.join(", ")}
                </p>
              )}
              <p className="m-0 text-xs text-muted-foreground">{labels.recipeReview}</p>
              {onOpenRecipeDraft && (
                <Button
                  type="button"
                  onClick={() => onOpenRecipeDraft(scrubRecipeSecrets(recipe.recipe))}
                >
                  <WorkflowIcon aria-hidden="true" data-icon="inline-start" />
                  {labels.recipeOpen}
                </Button>
              )}
            </section>
          )}
          {provider === "cloud" && !canRun && handoffCard}
        </TabsContent>
      </Tabs>

      <CloudConsentDialog
        open={dialogOpen}
        operation={operationName}
        onConfirm={confirmConsent}
        onCancel={cancelConsent}
        labels={labels.consent}
      />
    </aside>
  );
}
