/** Wire contracts of the five AI tools, mirroring `ariad-core::tools::params::ai` (snake_case JSON). */

export type AiProvider = "local" | "cloud";
export type AiToolId =
  | "ai.summarize"
  | "ai.ask"
  | "ai.translate"
  | "ai.extract_tables"
  | "ai.recipe_from_text";

/** Fields shared by every AI tool. `api_key` is a secret and only ever travels in memory. */
export interface AiCommonParams {
  provider?: AiProvider;
  model?: string;
  cloud_consent?: boolean;
  api_key?: string;
}

export type SummaryLength = "brief" | "standard" | "detailed";
export type SummaryFormat = "json" | "md";
export type TranslateFormat = "pdf" | "docx" | "html" | "md" | "json";
export type TableFormat = "csv" | "xlsx" | "json";

export interface AiSummarizeParams extends AiCommonParams {
  length?: SummaryLength;
  language?: string;
  format?: SummaryFormat;
}
export interface AiAskParams extends AiCommonParams {
  question: string;
  max_citations?: number;
}
export interface AiTranslateParams extends AiCommonParams {
  target_lang: string;
  source_lang?: string;
  format?: TranslateFormat;
}
export interface AiExtractTablesParams extends AiCommonParams {
  pages?: string;
  format?: TableFormat;
}
export interface AiRecipeFromTextParams extends AiCommonParams {
  prompt: string;
}

export interface AiParamsByTool {
  "ai.summarize": AiSummarizeParams;
  "ai.ask": AiAskParams;
  "ai.translate": AiTranslateParams;
  "ai.extract_tables": AiExtractTablesParams;
  "ai.recipe_from_text": AiRecipeFromTextParams;
}

export interface AiSummarizeOutput {
  summary: string;
  bullet_points: string[];
  language: string;
  length: string;
}
export interface AiCitation {
  page: number;
  quote: string;
  score: number;
}
export interface AiAskOutput {
  answer: string;
  citations: AiCitation[];
}
export interface AiTable {
  page: number;
  rows: string[][];
}
export interface AiArtifact {
  name: string;
  href: string;
}
export interface AiExtractTablesOutput {
  tables: AiTable[];
  /** Present for formats the browser cannot synthesise itself, such as `xlsx`. */
  artifact?: AiArtifact;
}
export interface AiTranslateOutput {
  artifact: AiArtifact;
}

export interface RecipeStep {
  tool: string;
  params: Record<string, unknown>;
}
export interface RecipeDraft {
  version: "ariad-recipe/1";
  name?: string;
  steps: RecipeStep[];
}
export interface AiRecipeFromTextOutput {
  recipe: RecipeDraft;
  origin: "imported";
  requires_confirmation: true;
  /** Clauses of the prompt the offline parser could not map to a tool. */
  unmatched?: string[];
}

export interface AiOutputByTool {
  "ai.summarize": AiSummarizeOutput;
  "ai.ask": AiAskOutput;
  "ai.translate": AiTranslateOutput;
  "ai.extract_tables": AiExtractTablesOutput;
  "ai.recipe_from_text": AiRecipeFromTextOutput;
}

/** A request the host application executes: an AI tool, or the local `pdf.tables_to_csv` shortcut. */
export type AiRequest =
  | {
      [T in AiToolId]: { tool: T; params: AiParamsByTool[T] };
    }[AiToolId]
  | { tool: "pdf.tables_to_csv"; params: Record<string, never> };

export type AiOutput = AiOutputByTool[AiToolId] | AiExtractTablesOutput;
