import { shellQuote } from "@/converter/handoff";
import type {
  AiAskOutput,
  AiAskParams,
  AiCommonParams,
  AiExtractTablesOutput,
  AiExtractTablesParams,
  AiParamsByTool,
  AiProvider,
  AiRecipeFromTextOutput,
  AiRecipeFromTextParams,
  AiSummarizeOutput,
  AiSummarizeParams,
  AiTable,
  AiToolId,
  AiTranslateOutput,
  AiTranslateParams,
  RecipeDraft,
  RecipeStep,
  SummaryLength,
} from "./ai-types";

export const MAX_CITATIONS_MIN = 1;
export const MAX_CITATIONS_MAX = 20;
export const DEFAULT_MAX_CITATIONS = 5;
/** The CLI reads a secret from stdin; this variable only ever lives in the user's own shell. */
export const API_KEY_ENV = "ASHIFT_AI_API_KEY";

export type AiValidationErrors = Record<string, string>;

export interface AiParamsResult<T> {
  params: T;
  errors: AiValidationErrors;
  isValid: boolean;
}

export interface AiConnection {
  provider: AiProvider;
  consent: boolean;
  apiKey?: string;
  model?: string;
}

function finish<T>(params: T, errors: AiValidationErrors): AiParamsResult<T> {
  return { params, errors, isValid: Object.keys(errors).length === 0 };
}

/**
 * Provider fields shared by every tool. Local runs carry no consent flag or key at all, so a
 * request that stays on the device cannot be mistaken for one that leaves it.
 */
export function commonParams(connection: AiConnection): AiCommonParams {
  const model = connection.model?.trim();
  if (connection.provider === "local") return { provider: "local", ...(model && { model }) };
  const apiKey = connection.apiKey?.trim();
  return {
    provider: "cloud",
    cloud_consent: connection.consent,
    ...(model && { model }),
    ...(apiKey && { api_key: apiKey }),
  };
}

export function buildSummarizeParams(
  input: { length?: SummaryLength; language?: string },
  connection: AiConnection,
): AiParamsResult<AiSummarizeParams> {
  const language = input.language?.trim();
  return finish(
    {
      length: input.length ?? "standard",
      ...(language && { language }),
      ...commonParams(connection),
    },
    {},
  );
}

export function buildAskParams(
  input: { question: string; maxCitations?: number },
  connection: AiConnection,
): AiParamsResult<AiAskParams> {
  const errors: AiValidationErrors = {};
  const question = input.question.trim();
  if (!question) errors.question = "must not be empty";
  const max = input.maxCitations;
  if (
    max !== undefined &&
    (!Number.isInteger(max) || max < MAX_CITATIONS_MIN || max > MAX_CITATIONS_MAX)
  )
    errors.max_citations = `must be between ${MAX_CITATIONS_MIN} and ${MAX_CITATIONS_MAX}`;
  return finish(
    {
      question,
      ...(max !== undefined && !errors.max_citations && { max_citations: max }),
      ...commonParams(connection),
    },
    errors,
  );
}

export function buildTranslateParams(
  input: { targetLang: string; sourceLang?: string; format?: AiTranslateParams["format"] },
  connection: AiConnection,
): AiParamsResult<AiTranslateParams> {
  const errors: AiValidationErrors = {};
  const target = input.targetLang.trim();
  if (!target) errors.target_lang = "must not be empty";
  const source = input.sourceLang?.trim();
  return finish(
    {
      target_lang: target,
      ...(source && { source_lang: source }),
      ...(input.format && { format: input.format }),
      ...commonParams(connection),
    },
    errors,
  );
}

export function buildExtractTablesParams(
  input: { pages?: string; format?: AiExtractTablesParams["format"] },
  connection: AiConnection,
  validatePages: (pages: string) => string | null,
): AiParamsResult<AiExtractTablesParams> {
  const errors: AiValidationErrors = {};
  const pages = input.pages?.trim();
  if (pages && pages !== "all") {
    const problem = validatePages(pages);
    if (problem) errors.pages = problem;
  }
  return finish(
    {
      ...(pages && pages !== "all" && !errors.pages && { pages }),
      ...(input.format && { format: input.format }),
      ...commonParams(connection),
    },
    errors,
  );
}

export function buildRecipeParams(
  input: { prompt: string },
  connection: AiConnection,
): AiParamsResult<AiRecipeFromTextParams> {
  const errors: AiValidationErrors = {};
  const prompt = input.prompt.trim();
  if (!prompt) errors.prompt = "must not be empty";
  return finish({ prompt, ...commonParams(connection) }, errors);
}

/** Removes every secret before parameters reach a command line, URL, recipe, log or storage. */
export function scrubSecrets<T extends AiCommonParams>(params: T): Omit<T, "api_key"> {
  const { api_key: _discarded, ...rest } = params;
  return rest;
}

const AI_TOOLS: readonly AiToolId[] = [
  "ai.summarize",
  "ai.ask",
  "ai.translate",
  "ai.extract_tables",
  "ai.recipe_from_text",
];

export function isAiTool(tool: string): tool is AiToolId {
  return (AI_TOOLS as readonly string[]).includes(tool);
}

export interface AiCliHandoff {
  command: string;
  /** True when the command expects the key on stdin; the key itself is never written. */
  readsApiKeyFromStdin: boolean;
}

/**
 * The exact desktop command for a request. Parameters are passed with `--param`; the API key is
 * read by the CLI from stdin (`--secret api_key=-`), fed from the caller's own environment.
 * `cloud_consent` is a value the CLI requires, never a secret.
 */
export function buildAiCliHandoff<T extends AiToolId>(
  tool: T,
  params: AiParamsByTool[T],
  inputName?: string,
): AiCliHandoff {
  const wantsKey = params.provider === "cloud" && Boolean(params.api_key);
  const { api_key: _discarded, ...visible } = params as AiCommonParams & Record<string, unknown>;
  const args = ["ashift", "tool", shellQuote(tool)];
  if (tool !== "ai.recipe_from_text" && inputName)
    args.push(shellQuote(inputName.startsWith("-") ? `./${inputName}` : inputName));
  for (const [key, value] of Object.entries(visible)) {
    if (value === undefined) continue;
    args.push("--param", shellQuote(`${key}=${typeof value === "string" ? value : String(value)}`));
  }
  if (wantsKey) args.push("--secret", shellQuote("api_key=-"));
  const command = args.join(" ");
  return wantsKey
    ? { command: `printf %s "$${API_KEY_ENV}" | ${command}`, readsApiKeyFromStdin: true }
    : { command, readsApiKeyFromStdin: false };
}

/** RFC 4180 CSV with spreadsheet formula-injection guarding (`=`, `@`, tab, CR, and non-numeric `+`/`-`). */
export function tablesToCsv(rows: string[][]): string {
  const guard = (cell: string) =>
    /^[=@\t\r]/.test(cell) || (/^[+-]/.test(cell) && Number.isNaN(Number(cell)))
      ? `'${cell}`
      : cell;
  const quote = (cell: string) => {
    const safe = guard(cell);
    return /[",\r\n]/.test(safe) ? `"${safe.replaceAll('"', '""')}"` : safe;
  };
  return rows.map((row) => row.map(quote).join(",")).join("\r\n");
}

export function tableRowCount(tables: AiTable[]): number {
  return tables.reduce((total, table) => total + table.rows.length, 0);
}

const UNIT_BYTES: Record<string, number> = { kb: 1024, mb: 1024 * 1024, gb: 1024 * 1024 * 1024 };

function sizeBytes(clause: string): number | undefined {
  const match = clause.match(/(\d+(?:\.\d+)?)\s*(kb|mb|gb)\b/i);
  if (!match?.[1] || !match[2]) return undefined;
  const bytes = Math.round(Number(match[1]) * (UNIT_BYTES[match[2].toLowerCase()] ?? 0));
  return bytes >= 1024 ? bytes : undefined;
}

function rotationAngle(clause: string): number {
  if (/\b(left|counter|anti|ccw)\b/i.test(clause) || /-\s*90\b/.test(clause)) return -90;
  const degrees = clause.match(/(\d{2,3})/)?.[1];
  return degrees === "180" || degrees === "270" ? Number(degrees) : 90;
}

function watermarkText(clause: string): string | undefined {
  const match = clause.match(/watermark\w*\s+(?:with\s+|as\s+|text\s+)?(.+)$/i);
  const text = match?.[1]
    ?.trim()
    .replace(/^["'“‘](.*)["'”’]$/, "$1")
    .trim();
  return text || undefined;
}

type ClauseRule = (clause: string) => RecipeStep | undefined;

const RULES: ClauseRule[] = [
  (c) =>
    /\b(merge|combine|join|concatenate)\b/i.test(c) ? { tool: "pdf.merge", params: {} } : undefined,
  (c) => (/\bocr\b|\bsearchable\b/i.test(c) ? { tool: "pdf.ocr", params: {} } : undefined),
  (c) => (/\bdeskew\b|\bstraighten\b/i.test(c) ? { tool: "pdf.deskew", params: {} } : undefined),
  (c) =>
    /\bcompress|\bshrink\b|\breduce\b.*\bsize\b/i.test(c)
      ? {
          tool: "pdf.compress",
          params: ((bytes) => (bytes ? { target_bytes: bytes } : { preset: "ebook" }))(
            sizeBytes(c),
          ),
        }
      : undefined,
  (c) =>
    /\brotate\b/i.test(c)
      ? { tool: "pdf.rotate", params: { pages: "all", angle: rotationAngle(c) } }
      : undefined,
  (c) => {
    if (!/\bwatermark/i.test(c)) return undefined;
    const text = watermarkText(c);
    return text ? { tool: "pdf.watermark", params: { kind: "text", text } } : undefined;
  },
];

/**
 * Deterministic, offline mapping of a plain sentence to a recipe draft. It recognises merge, OCR,
 * deskew, compress (with an optional size target), rotate and text watermark; anything else is
 * reported in `unmatched`. The draft is always `imported` and requires confirmation: it fills the
 * Recipe Builder for review and never runs by itself.
 */
export function parseNaturalLanguageRecipeFallback(
  prompt: string,
): (AiRecipeFromTextOutput & { unmatched: string[] }) | null {
  const clauses = prompt
    .split(/\s*(?:,|;|\bthen\b|\band then\b|\band\b|->|→|\n)\s*/i)
    .map((part) => part.trim())
    .filter(Boolean);
  const steps: RecipeStep[] = [];
  const unmatched: string[] = [];
  for (const clause of clauses) {
    let step: RecipeStep | undefined;
    for (const rule of RULES) {
      step = rule(clause);
      if (step) break;
    }
    if (step) steps.push(step);
    else unmatched.push(clause);
  }
  if (steps.length === 0) return null;
  return {
    recipe: { version: "ariad-recipe/1", steps },
    origin: "imported",
    requires_confirmation: true,
    unmatched,
  };
}

/** Narrow runtime guards: results come from a runner or a remote endpoint, not from this module. */
export function isAskOutput(value: unknown): value is AiAskOutput {
  const v = value as { answer?: unknown; citations?: unknown } | null;
  return (
    typeof v?.answer === "string" &&
    Array.isArray(v.citations) &&
    v.citations.every((c) => {
      const cite = c as { page?: unknown; quote?: unknown; score?: unknown };
      return (
        Number.isInteger(cite?.page) &&
        typeof cite.quote === "string" &&
        typeof cite.score === "number"
      );
    })
  );
}

export function isSummarizeOutput(value: unknown): value is AiSummarizeOutput {
  const v = value as { summary?: unknown; bullet_points?: unknown } | null;
  return typeof v?.summary === "string" && Array.isArray(v.bullet_points);
}

export function isTablesOutput(value: unknown): value is AiExtractTablesOutput {
  const v = value as { tables?: unknown } | null;
  return (
    Array.isArray(v?.tables) &&
    v.tables.every(
      (t) => Number.isInteger((t as AiTable)?.page) && Array.isArray((t as AiTable).rows),
    )
  );
}

export function isRecipeOutput(value: unknown): value is AiRecipeFromTextOutput {
  const v = value as Partial<AiRecipeFromTextOutput> | null;
  return (
    v?.origin === "imported" &&
    v.requires_confirmation === true &&
    v.recipe?.version === "ariad-recipe/1" &&
    Array.isArray(v.recipe.steps)
  );
}

export function isTranslateOutput(value: unknown): value is AiTranslateOutput {
  const a = (value as { artifact?: { name?: unknown; href?: unknown } } | null)?.artifact;
  return typeof a?.name === "string" && typeof a.href === "string";
}

/** Only same-document blob URLs and web URLs may become download links; never `javascript:`. */
export function safeArtifactHref(href: string): string | undefined {
  try {
    const url = new URL(href, "https://invalid.local");
    return ["blob:", "https:", "http:"].includes(url.protocol) && !href.startsWith("//")
      ? href
      : undefined;
  } catch {
    return undefined;
  }
}

/** A recipe draft is a shareable file, so any `api_key` a runner left in a step is dropped. */
export function scrubRecipeSecrets(recipe: RecipeDraft): RecipeDraft {
  return {
    ...recipe,
    steps: recipe.steps.map((step) => {
      const { api_key: _discarded, ...params } = step.params;
      return { ...step, params };
    }),
  };
}
