/** English defaults for every string in the AI panel; the workspace passes localized overrides. */
export interface AiPanelLabels {
  title: string;
  tablistLabel: string;
  tabs: {
    summarize: string;
    ask: string;
    translate: string;
    extractTables: string;
    recipe: string;
  };
  badgeLocal: string;
  badgeLocalDetail: string;
  badgeCloud: string;
  revoke: string;
  providerLabel: string;
  providerLocal: string;
  providerCloud: string;
  run: string;
  running: string;
  copy: string;
  copied: string;
  insertNote: string;
  length: string;
  lengths: { brief: string; standard: string; detailed: string };
  outputLanguage: string;
  autoLanguage: string;
  summaryHeading: string;
  question: string;
  questionPlaceholder: string;
  maxCitations: string;
  ask: string;
  answerHeading: string;
  citationJump: string;
  noCitations: string;
  targetLanguage: string;
  sourceLanguage: string;
  detectLanguage: string;
  outputFormat: string;
  translateNote: string;
  translateDone: string;
  pages: string;
  pagesAll: string;
  pagesCustom: string;
  quickCsv: string;
  tablesHeading: string;
  tablePage: string;
  downloadCsv: string;
  downloadArtifact: string;
  noTables: string;
  recipePrompt: string;
  recipePlaceholder: string;
  recipeGenerate: string;
  recipeSteps: string;
  recipeUnmatched: string;
  recipeNone: string;
  recipeOpen: string;
  recipeReview: string;
  handoffTitle: string;
  handoffBody: string;
  handoffKey: string;
  handoffCopy: string;
  invalid: string;
  failed: string;
  consent: {
    title: string;
    description: string;
    leaves: string;
    leavesDocument: string;
    leavesOperation: string;
    leavesProvider: string;
    stays: string;
    checkbox: string;
    apiKey: string;
    apiKeyHint: string;
    confirm: string;
    cancel: string;
    close: string;
  };
}

export const DEFAULT_AI_LABELS: AiPanelLabels = {
  title: "AI assistant",
  tablistLabel: "AI tools",
  tabs: {
    summarize: "Summarise",
    ask: "Ask",
    translate: "Translate",
    extractTables: "Extract tables",
    recipe: "Recipe",
  },
  badgeLocal: "On this device · 0 bytes uploaded",
  badgeLocalDetail: "Local",
  badgeCloud: "Cloud AI (consent granted for session)",
  revoke: "Revoke & switch to Local",
  providerLabel: "Where it runs",
  providerLocal: "On this device",
  providerCloud: "Cloud AI",
  run: "Run",
  running: "Working…",
  copy: "Copy",
  copied: "Copied",
  insertNote: "Insert as note",
  length: "Length",
  lengths: { brief: "Brief", standard: "Standard", detailed: "Detailed" },
  outputLanguage: "Output language",
  autoLanguage: "Same as document",
  summaryHeading: "Summary",
  question: "Question",
  questionPlaceholder: "Ask anything about this document",
  maxCitations: "Citations (1–20)",
  ask: "Ask",
  answerHeading: "Answer",
  citationJump: "Go to page",
  noCitations: "No page citations were returned.",
  targetLanguage: "Translate into",
  sourceLanguage: "From",
  detectLanguage: "Detect automatically",
  outputFormat: "Output format",
  translateNote: "Structure, IDs, images and styles are preserved. Only the text changes.",
  translateDone: "Translation ready",
  pages: "Pages",
  pagesAll: "All pages",
  pagesCustom: "Custom range",
  quickCsv: "Quick CSV (local)",
  tablesHeading: "Extracted tables",
  tablePage: "Page",
  downloadCsv: "Download CSV",
  downloadArtifact: "Download",
  noTables: "No tables found in the selected pages.",
  recipePrompt: "Describe the steps",
  recipePlaceholder: "merge, OCR, compress under 5 MB",
  recipeGenerate: "Draft recipe",
  recipeSteps: "Draft steps",
  recipeUnmatched: "Not understood",
  recipeNone: "No known steps found. Try words such as merge, OCR, compress, rotate or deskew.",
  recipeOpen: "Open in Recipe Builder (review required)",
  recipeReview: "This draft never runs by itself. You review and confirm it in the Recipe Builder.",
  handoffTitle: "Run on Desktop",
  handoffBody:
    "No local AI model is loaded in this browser. Run this command on your computer, or switch to Cloud AI, which asks for your consent first.",
  handoffKey:
    "Your API key is read from the environment variable below and is never shown in this command.",
  handoffCopy: "Copy command",
  invalid: "Check the highlighted fields.",
  failed: "The request failed.",
  consent: {
    title: "Send this document to Cloud AI?",
    description: "Nothing leaves your device unless you agree. Local processing stays the default.",
    leaves: "What leaves your device",
    leavesDocument: "The text and pages of this document",
    leavesOperation: "The selected operation and your prompt",
    leavesProvider: "To the configured Cloud AI endpoint",
    stays:
      "Your API key is kept in memory for this session only. It is never saved or put in a link.",
    checkbox:
      "I consent to sending this document's content to the configured Cloud AI endpoint for this request",
    apiKey: "API key (optional)",
    apiKeyHint: "Bring your own key. Leave empty to use your plan.",
    confirm: "Confirm and continue",
    cancel: "Cancel",
    close: "Close",
  },
};

export function resolveAiLabels(overrides?: Partial<AiPanelLabels>): AiPanelLabels {
  if (!overrides) return DEFAULT_AI_LABELS;
  return {
    ...DEFAULT_AI_LABELS,
    ...overrides,
    tabs: { ...DEFAULT_AI_LABELS.tabs, ...overrides.tabs },
    lengths: { ...DEFAULT_AI_LABELS.lengths, ...overrides.lengths },
    consent: { ...DEFAULT_AI_LABELS.consent, ...overrides.consent },
  };
}
