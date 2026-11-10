// Tool hub model: categories, runtime and tier badges, search, favourites, recents and the
// plain-sentence recipe parser. Everything derives from the tool registry (`toolCatalog()`),
// so a tool added to `listTools()` shows up in the hub without a change here.
import { useCallback, useSyncExternalStore } from "react";
import type { ToolDescriptor } from "../converter/tool-catalog";

/* ---------------------------------------------------------------- translation hook */

/**
 * Optional translation hook: returns the localised text for `key`, or `fallback` when absent.
 * `values` fill the `{name}` arguments and plural counts of a catalogue message.
 */
export type ToolsTranslate = (
  key: string,
  fallback: string,
  values?: Record<string, string | number>,
) => string;

export const identityTranslate: ToolsTranslate = (_key, fallback) => fallback;

/* ---------------------------------------------------------------- categories */

export const TOOL_CATEGORIES = [
  "organize",
  "convert",
  "compress",
  "edit",
  "forms_annotations",
  "security",
  "ocr_scan",
  "metadata_inspect",
  "automation_ai",
] as const;

export type ToolCategory = (typeof TOOL_CATEGORIES)[number];

export const CATEGORY_LABELS: Record<ToolCategory, string> = {
  organize: "Organise & assemble",
  convert: "Convert",
  compress: "Compress & repair",
  edit: "Edit & mark up",
  forms_annotations: "Forms & annotations",
  security: "Security & signing",
  ocr_scan: "OCR & scan",
  metadata_inspect: "Inspect & compare",
  automation_ai: "Automation & AI",
};

/** Registry `group` -> hub category. Groups the registry adds later fall through to inference. */
const GROUP_CATEGORY: Record<string, ToolCategory> = {
  organize: "organize",
  optimize: "compress",
  overlay: "edit",
  convert: "convert",
  security: "security",
  ocr: "ocr_scan",
  annotate: "forms_annotations",
  forms: "forms_annotations",
  redact: "security",
  sign: "security",
  compare: "metadata_inspect",
  edit: "edit",
  ai: "automation_ai",
};

/** Tools whose purpose is closer to another category than their registry group suggests. */
const ID_CATEGORY: Record<string, ToolCategory> = {
  "pdf.info": "metadata_inspect",
  "pdf.metadata": "metadata_inspect",
  "pdf.attachments": "metadata_inspect",
  "pdf.page_labels": "metadata_inspect",
  "pdf.viewer_prefs": "metadata_inspect",
  "pdf.layers": "metadata_inspect",
  "pdf.text_layout": "metadata_inspect",
  "pdf.extract_images": "metadata_inspect",
  "pdf.outline": "edit",
  "pdf.detect_scanned": "ocr_scan",
  "pdf.remove_blank": "ocr_scan",
  "pdf.scanner_effect": "ocr_scan",
  "pdf.sanitize": "compress",
  "pdf.watermark_remove_raster": "automation_ai",
};

/** Keyword inference for a tool with an unknown group and no explicit mapping. First match wins. */
const INFERENCE: Array<[RegExp, ToolCategory]> = [
  [/^ai\.|recipe|batch|watch|summar/, "automation_ai"],
  [/ocr|scan|deskew|dewarp|denoise|binar/, "ocr_scan"],
  [/encrypt|decrypt|redact|sign|cert|permission|password|timestamp/, "security"],
  [/compress|optimi|linear|repair|sanitize|decompress/, "compress"],
  [/form|annot|comment|highlight/, "forms_annotations"],
  [/convert|render|\.to_|_to_|export|thumbnail/, "convert"],
  [/info|meta|inspect|compare|validate|font|attach|extract|report|diff/, "metadata_inspect"],
  [
    /watermark|stamp|header|footer|bates|background|link|bookmark|outline|edit|text|overlay/,
    "edit",
  ],
];

/** Every tool maps to exactly one category, deterministically. */
export function toolCategory(tool: { id: string; group: string }): ToolCategory {
  const byId = ID_CATEGORY[tool.id];
  if (byId) {
    return byId;
  }
  const byGroup = GROUP_CATEGORY[tool.group];
  if (byGroup) {
    return byGroup;
  }
  const haystack = `${tool.id} ${tool.group}`.toLowerCase();
  for (const [pattern, category] of INFERENCE) {
    if (pattern.test(haystack)) {
      return category;
    }
  }
  return "organize";
}

/* ---------------------------------------------------------------- titles and keywords */

const WORD_LABELS: Record<string, string> = {
  pdf: "PDF",
  pdfa: "PDF/A",
  pdfua: "PDF/UA",
  ocr: "OCR",
  qr: "QR",
  csv: "CSV",
  cbz: "CBZ",
  ai: "AI",
  nup: "N-up",
  toc: "TOC",
  html: "HTML",
  web: "Web",
  image: "Image",
  api: "API",
};

/** A human title from the id: `pdf.extract_pages` -> "Extract pages". */
export function toolTitle(tool: Pick<ToolDescriptor, "id">): string {
  const [namespace = "", ...rest] = tool.id.split(".");
  const name = rest.join(" ") || namespace;
  const words = name.split("_").filter(Boolean);
  // `pdf.*` and `edit.*` are implied by the surface; other namespaces carry meaning.
  if (rest.length > 0 && namespace !== "pdf" && namespace !== "edit") {
    words.unshift(namespace);
  }
  const labelled = words.map((word) => WORD_LABELS[word] ?? word);
  const [first = "", ...others] = labelled;
  const sentence = [first.charAt(0).toUpperCase() + first.slice(1), ...others];
  return sentence.join(" ");
}

/** Plain-language synonyms people search with, keyed by a word of the tool id. */
const SYNONYMS: Record<string, string[]> = {
  merge: ["combine", "join", "concatenate"],
  split: ["separate", "divide", "break apart"],
  rotate: ["turn", "flip", "orientation"],
  compress: ["shrink", "reduce size", "smaller", "optimize"],
  reorder: ["rearrange", "sort", "reverse"],
  delete: ["remove pages", "drop"],
  extract: ["pull out", "save pages"],
  crop: ["trim", "margins"],
  scale: ["resize", "fit"],
  ocr: ["recognize text", "searchable", "scan to text"],
  encrypt: ["password", "protect", "lock"],
  decrypt: ["unlock", "remove password"],
  watermark: ["stamp", "confidential", "draft"],
  redact: ["blackout", "censor", "hide text"],
  sign: ["signature", "sign document"],
  info: ["properties", "details", "page count"],
  metadata: ["author", "title", "properties"],
  forms: ["fields", "fill"],
  flatten: ["bake in", "merge layers"],
  render: ["screenshot", "rasterize"],
  images: ["png", "jpg", "pictures"],
  repair: ["fix", "recover", "corrupt"],
  grayscale: ["black and white", "greyscale"],
  summarize: ["summary", "tl;dr", "digest"],
  translate: ["language", "localise"],
  compare: ["diff", "differences"],
  booklet: ["imposition", "print"],
  nup: ["multiple pages per sheet", "handout"],
  bates: ["legal numbering", "stamp numbers"],
  linearize: ["fast web view", "web optimize"],
};

/** Words a tool is found by: its id parts, formats, engine and synonyms of those words. */
export function toolKeywords(tool: ToolDescriptor): string[] {
  const words = tool.id.split(/[._]/).filter(Boolean);
  const keywords = new Set<string>(words);
  for (const word of words) {
    for (const synonym of SYNONYMS[word] ?? []) {
      keywords.add(synonym);
    }
  }
  for (const format of tool.input_formats) {
    keywords.add(format);
  }
  keywords.add(tool.output_format);
  keywords.add(tool.engine);
  return [...keywords];
}

/* ---------------------------------------------------------------- runtime and tier badges */

export type ToolRuntimeBadge = "browser" | "desktop" | "cloud";
export type ToolTierBadge = "free" | "pro";

export const RUNTIME_LABELS: Record<ToolRuntimeBadge, string> = {
  browser: "Browser",
  desktop: "Desktop",
  cloud: "Cloud",
};

export const TIER_LABELS: Record<ToolTierBadge, string> = { free: "Free", pro: "Pro" };

const RUNTIME_BADGE: Record<string, ToolRuntimeBadge> = {
  wasm: "browser",
  local: "desktop",
  cloud: "cloud",
};

/**
 * Where a tool runs first: `browser` when it has a WebAssembly runtime, `cloud` when it is
 * eligible for cloud jobs without one, `desktop` when only the local engines can run it.
 */
export function primaryRuntime(tool: Pick<ToolDescriptor, "runtimes">): ToolRuntimeBadge {
  if (tool.runtimes.includes("wasm")) {
    return "browser";
  }
  return tool.runtimes.includes("cloud") ? "cloud" : "desktop";
}

/** Every runtime the tool supports, in Browser, Desktop, Cloud order. */
export function toolRuntimes(tool: Pick<ToolDescriptor, "runtimes">): ToolRuntimeBadge[] {
  const found = new Set<ToolRuntimeBadge>();
  for (const runtime of tool.runtimes) {
    const badge = RUNTIME_BADGE[runtime];
    if (badge) {
      found.add(badge);
    }
  }
  return (["browser", "desktop", "cloud"] as const).filter((badge) => found.has(badge));
}

export function toolTier(tool: Pick<ToolDescriptor, "tier">): ToolTierBadge {
  return tool.tier === "pro" ? "pro" : "free";
}

/* ---------------------------------------------------------------- hub entries and search */

/** A catalog tool prepared for the hub: derived once, then searched and rendered. */
export interface HubTool {
  tool: ToolDescriptor;
  id: string;
  title: string;
  summary: string;
  category: ToolCategory;
  runtime: ToolRuntimeBadge;
  runtimes: ToolRuntimeBadge[];
  tier: ToolTierBadge;
  keywords: string[];
}

/** With `translate`, titles and summaries come from the catalogue, so search matches them too. */
export function describeTools(
  catalog: ToolDescriptor[],
  translate: ToolsTranslate = identityTranslate,
): HubTool[] {
  return catalog.map((tool) => ({
    tool,
    id: tool.id,
    title: translate(`tools.title.${tool.id}`, toolTitle(tool)),
    summary: translate(`tools.summary.${tool.id}`, tool.summary),
    category: toolCategory(tool),
    runtime: primaryRuntime(tool),
    runtimes: toolRuntimes(tool),
    tier: toolTier(tool),
    keywords: toolKeywords(tool),
  }));
}

export interface ToolFilters {
  category?: ToolCategory | "all";
  /** Matches the runtime badge a card shows: its primary runtime. */
  runtime?: ToolRuntimeBadge | "all";
  tier?: ToolTierBadge | "all";
}

function matchesFilters(entry: HubTool, filters: ToolFilters): boolean {
  return (
    (!filters.category || filters.category === "all" || entry.category === filters.category) &&
    (!filters.runtime || filters.runtime === "all" || entry.runtime === filters.runtime) &&
    (!filters.tier || filters.tier === "all" || entry.tier === filters.tier)
  );
}

const normalize = (text: string) => text.toLowerCase().normalize("NFKD").replace(/\p{M}/gu, "");

/** Letters of `needle` appear in order in `haystack` (typo-tolerant search for longer words). */
function isSubsequence(needle: string, haystack: string): boolean {
  let index = 0;
  for (const char of haystack) {
    if (char === needle[index]) {
      index += 1;
      if (index === needle.length) {
        return true;
      }
    }
  }
  return false;
}

/**
 * Scores one query word against one field: whole word beats word-prefix beats substring. With
 * `fuzzy`, a word that starts with the same letter and contains the query letters in order
 * ("mrg" for "merge") also matches, so typos still find the tool.
 */
function scoreWord(word: string, field: string, fuzzy: boolean): number {
  if (!field) {
    return 0;
  }
  if (field === word) {
    return 10;
  }
  const parts = field.split(/[^a-z0-9]+/);
  if (parts.some((part) => part === word)) {
    return 8;
  }
  if (parts.some((part) => part.startsWith(word))) {
    return 6;
  }
  if (field.includes(word)) {
    return 4;
  }
  const typo =
    fuzzy &&
    word.length >= 3 &&
    parts.some((part) => part[0] === word[0] && isSubsequence(word, part));
  return typo ? 1 : 0;
}

/** Field weights: a title hit outranks a keyword hit, which outranks the summary. */
function scoreEntry(entry: HubTool, words: string[]): number {
  // The summary is long prose where almost any letters occur in order, so it is never fuzzy.
  const fields: Array<[number, string, boolean]> = [
    [5, normalize(entry.title), true],
    [4, normalize(entry.id), true],
    [3, normalize(entry.keywords.join(" ")), true],
    [2, normalize(CATEGORY_LABELS[entry.category]), false],
    [1, normalize(entry.summary), false],
  ];
  let total = 0;
  for (const word of words) {
    let best = 0;
    for (const [weight, field, fuzzy] of fields) {
      best = Math.max(best, scoreWord(word, field, fuzzy) * weight);
    }
    if (best === 0) {
      return 0;
    }
    total += best;
  }
  return total;
}

/**
 * Filters and ranks tools. Every query word must match somewhere in the title, id, keywords,
 * category or summary; ties keep catalog order so the result is deterministic.
 */
export function searchTools(
  entries: HubTool[],
  query: string,
  filters: ToolFilters = {},
): HubTool[] {
  const words = normalize(query).split(/\s+/).filter(Boolean);
  const pool = entries.filter((entry) => matchesFilters(entry, filters));
  if (words.length === 0) {
    return pool;
  }
  return pool
    .map((entry, index) => ({ entry, index, score: scoreEntry(entry, words) }))
    .filter((hit) => hit.score > 0)
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map((hit) => hit.entry);
}

/** Groups entries by category in canonical order, omitting empty categories. */
export function groupByCategory(
  entries: HubTool[],
): Array<{ category: ToolCategory; tools: HubTool[] }> {
  return TOOL_CATEGORIES.map((category) => ({
    category,
    tools: entries.filter((entry) => entry.category === category),
  })).filter((group) => group.tools.length > 0);
}

/* ---------------------------------------------------------------- favourites and recents */

export const FAVORITES_KEY = "ariadshift.tools.favorites.v1";
export const RECENTS_KEY = "ariadshift.tools.recents.v1";
export const RECENTS_LIMIT = 12;

const NO_IDS: readonly string[] = Object.freeze([]);

function parseIds(raw: string | null, limit: number): readonly string[] {
  if (!raw) {
    return NO_IDS;
  }
  try {
    const value: unknown = JSON.parse(raw);
    if (!Array.isArray(value)) {
      return NO_IDS;
    }
    const ids = [...new Set(value.filter((item): item is string => typeof item === "string"))];
    return ids.slice(0, limit);
  } catch {
    return NO_IDS;
  }
}

/**
 * A localStorage-backed list of tool ids that React can subscribe to. Reads and writes never
 * throw (private mode, quota, server render): when storage fails the list lives in memory.
 */
function createIdStore(key: string, limit: number) {
  const listeners = new Set<() => void>();
  let lastRaw: string | null = null;
  let current: readonly string[] = NO_IDS;
  // Set once a write fails: storage cannot be trusted, so the in-memory list is the truth.
  let memoryOnly = false;

  const readRaw = (): string | null => {
    try {
      return window.localStorage.getItem(key);
    } catch {
      return lastRaw;
    }
  };

  const emit = () => {
    for (const listener of listeners) {
      listener();
    }
  };

  const get = (): readonly string[] => {
    if (typeof window === "undefined") {
      return NO_IDS;
    }
    if (memoryOnly) {
      return current;
    }
    const raw = readRaw();
    if (raw !== lastRaw) {
      lastRaw = raw;
      current = parseIds(raw, limit);
    }
    return current;
  };

  const set = (ids: readonly string[]) => {
    const next = ids.slice(0, limit);
    current = next.length === 0 ? NO_IDS : next;
    lastRaw = next.length === 0 ? null : JSON.stringify(next);
    try {
      if (lastRaw === null) {
        window.localStorage.removeItem(key);
      } else {
        window.localStorage.setItem(key, lastRaw);
      }
      memoryOnly = false;
    } catch {
      // Storage is unavailable: the in-memory list still serves this page view.
      memoryOnly = true;
    }
    emit();
  };

  const subscribe = (listener: () => void) => {
    listeners.add(listener);
    const onStorage = (event: StorageEvent) => {
      if (event.key === key || event.key === null) {
        listener();
      }
    };
    if (typeof window !== "undefined") {
      window.addEventListener("storage", onStorage);
    }
    return () => {
      listeners.delete(listener);
      if (typeof window !== "undefined") {
        window.removeEventListener("storage", onStorage);
      }
    };
  };

  return { get, set, subscribe, server: () => NO_IDS };
}

const favoritesStore = createIdStore(FAVORITES_KEY, Number.MAX_SAFE_INTEGER);
const recentsStore = createIdStore(RECENTS_KEY, RECENTS_LIMIT);

export const readFavorites = (): readonly string[] => favoritesStore.get();
export const readRecents = (): readonly string[] => recentsStore.get();

/** Adds or removes a favourite; returns whether the tool is now a favourite. */
export function toggleFavorite(toolId: string): boolean {
  const ids = favoritesStore.get();
  if (ids.includes(toolId)) {
    favoritesStore.set(ids.filter((id) => id !== toolId));
    return false;
  }
  favoritesStore.set([toolId, ...ids]);
  return true;
}

/** Records a tool as the most recently used; the list keeps the newest 12. */
export function recordRecent(toolId: string): void {
  recentsStore.set([toolId, ...recentsStore.get().filter((id) => id !== toolId)]);
}

export function clearRecents(): void {
  recentsStore.set([]);
}

export interface ToolFavorites {
  favorites: readonly string[];
  isFavorite: (toolId: string) => boolean;
  toggle: (toolId: string) => boolean;
}

export function useToolFavorites(): ToolFavorites {
  const favorites = useSyncExternalStore(
    favoritesStore.subscribe,
    favoritesStore.get,
    favoritesStore.server,
  );
  const isFavorite = useCallback((toolId: string) => favorites.includes(toolId), [favorites]);
  return { favorites, isFavorite, toggle: toggleFavorite };
}

export interface ToolRecents {
  recents: readonly string[];
  record: (toolId: string) => void;
  clear: () => void;
}

export function useRecentTools(): ToolRecents {
  const recents = useSyncExternalStore(
    recentsStore.subscribe,
    recentsStore.get,
    recentsStore.server,
  );
  return { recents, record: recordRecent, clear: clearRecents };
}

/* ---------------------------------------------------------------- recipe parser */

export interface RecipeStepDraft {
  toolId: string;
  tool: ToolDescriptor;
  params: Record<string, unknown>;
  /** What the step does, in words, for the preview ("Rotate 90°"). */
  label: string;
}

export interface RecipeDraft {
  steps: RecipeStepDraft[];
  /** True when running the recipe leaves the browser, needs Pro, uses AI or cannot be undone. */
  requiresConfirmation: boolean;
}

interface Intent {
  tool: string;
  pattern: RegExp;
  /** Parameters read from the clause; `original` keeps the user's casing. */
  params?: (clause: string, original: string) => Record<string, unknown>;
  label?: (params: Record<string, unknown>) => string;
}

const OCR_LANGUAGES: Record<string, string> = {
  english: "eng",
  vietnamese: "vie",
  french: "fra",
  german: "deu",
  spanish: "spa",
  italian: "ita",
  portuguese: "por",
  russian: "rus",
  japanese: "jpn",
  korean: "kor",
  arabic: "ara",
  hindi: "hin",
  dutch: "nld",
  polish: "pol",
  turkish: "tur",
  thai: "tha",
  ukrainian: "ukr",
  indonesian: "ind",
  "simplified chinese": "chi_sim",
  "traditional chinese": "chi_tra",
  chinese: "chi_sim",
};

const LANGUAGE_NAMES = Object.keys(OCR_LANGUAGES).sort((a, b) => b.length - a.length);
const LANGUAGE_PATTERN = new RegExp(`\\b(${LANGUAGE_NAMES.join("|")})\\b`, "g");

function findLanguages(clause: string): string[] {
  const found: string[] = [];
  for (const match of clause.matchAll(LANGUAGE_PATTERN)) {
    const name = match[1];
    const code = name ? OCR_LANGUAGES[name] : undefined;
    if (code && !found.includes(code)) {
      found.push(code);
    }
  }
  return found;
}

/** One page or range ("4", "2-6"), and a comma list of them ("1-3, 5"). */
const PAGE_ITEM = String.raw`\d+(?:\s*[-–]\s*\d+)?`;
const PAGE_LIST = String.raw`${PAGE_ITEM}(?:\s*,\s*${PAGE_ITEM})*`;
const PAGES_AFTER_WORD = new RegExp(String.raw`\bpages?\s+(${PAGE_LIST})`, "g");

/** `1-3, 5` after the word "page(s)", or the words even / odd. */
function findPages(clause: string): string | undefined {
  const range = new RegExp(PAGES_AFTER_WORD.source).exec(clause);
  if (range?.[1]) {
    return range[1].replace(/\s*[-–]\s*/g, "-").replace(/\s*,\s*/g, ", ");
  }
  const keyword = /\b(even|odd)\s+pages?\b/.exec(clause);
  return keyword?.[1];
}

function rotationAngle(input: string): number | undefined {
  // A page range such as "pages 90-100" is not an angle.
  const clause = input.replace(PAGES_AFTER_WORD, " ");
  const degrees = /(-?\d{2,3})\s*(?:°|deg(?:rees?)?)?/.exec(clause);
  const value = degrees?.[1] ? Number(degrees[1]) : undefined;
  const counter = /counter[\s-]?clockwise|anti[\s-]?clockwise|\bleft\b/.test(clause);
  if (value === 90 || value === 180 || value === 270 || value === -90) {
    if (counter && value === 90) {
      return -90;
    }
    return value;
  }
  if (value === -270) {
    return 90;
  }
  if (/upside[\s-]?down/.test(clause)) {
    return 180;
  }
  if (counter) {
    return -90;
  }
  return /\b(?:clockwise|right)\b/.test(clause) ? 90 : undefined;
}

/** Language codes when the whole clause is just language names ("and english"), else none. */
function languagesOnly(clause: string): string[] {
  const lower = clause.toLowerCase();
  const rest = lower
    .replace(LANGUAGE_PATTERN, " ")
    .replace(FILLER, " ")
    .replace(/\s+/g, " ")
    .trim();
  return rest === "" ? findLanguages(lower) : [];
}

/** Text after the cue word, with quotes removed and the user's casing kept. */
function textAfter(original: string, cue: RegExp): string | undefined {
  const quoted = /["“]([^"”]+)["”]/.exec(original);
  if (quoted?.[1]?.trim()) {
    return quoted[1].trim();
  }
  const match = cue.exec(original);
  if (!match) {
    return undefined;
  }
  const rest = original
    .slice(match.index + match[0].length)
    .replace(/^\s*(?:with|saying|reading|that says|text|of|as|:)\s+/i, "")
    .replace(/^\s*(?:with|saying|reading|text|:)\s+/i, "")
    .trim();
  return rest || undefined;
}

const INTENTS: Intent[] = [
  // Specific phrases first: "remove password" is decrypt, not delete.
  { tool: "pdf.watermark_remove", pattern: /\b(?:remove|delete|erase)\s+(?:the\s+)?watermarks?\b/ },
  { tool: "pdf.decrypt", pattern: /\b(?:decrypt|unlock|remove\s+(?:the\s+)?password)\b/ },
  {
    tool: "pdf.remove_blank",
    pattern: /\b(?:remove|delete|drop)\s+(?:the\s+)?blank\s+pages?\b|\bblank\s+pages?\b/,
  },
  { tool: "pdf.sanitize", pattern: /\b(?:sanitize|sanitise|scrub)\b/ },
  { tool: "pdf.forms_flatten", pattern: /\bflatten\b/ },
  { tool: "pdf.merge", pattern: /\b(?:merge|combine|join|concatenate)\b/ },
  { tool: "pdf.split", pattern: /\b(?:split|separate)\b/ },
  {
    tool: "pdf.delete_pages",
    pattern: /\b(?:delete|remove|drop)\s+(?:the\s+)?pages?\b/,
    params: (clause) => {
      const pages = findPages(clause);
      return pages ? { pages } : {};
    },
    label: (params) => (params.pages ? `Delete pages ${params.pages}` : "Delete pages"),
  },
  {
    tool: "pdf.extract_pages",
    pattern: /\bextract\s+(?:the\s+)?pages?\b/,
    params: (clause) => {
      const pages = findPages(clause);
      return pages ? { pages } : {};
    },
    label: (params) => (params.pages ? `Extract pages ${params.pages}` : "Extract pages"),
  },
  {
    tool: "pdf.rotate",
    pattern: /\brotate\b|\bturn\s+(?:the\s+)?pages?\b/,
    params: (clause) => {
      const angle = rotationAngle(clause);
      const pages = findPages(clause);
      return { ...(angle === undefined ? {} : { angle }), ...(pages ? { pages } : {}) };
    },
    label: (params) => (params.angle === undefined ? "Rotate" : `Rotate ${params.angle}°`),
  },
  {
    tool: "pdf.compress",
    pattern: /\b(?:compress|shrink|optimi[sz]e)\b|\breduce\s+(?:the\s+)?size\b/,
  },
  { tool: "pdf.grayscale", pattern: /\b(?:grayscale|greyscale|black[\s-]+and[\s-]+white)\b/ },
  {
    tool: "pdf.watermark",
    pattern: /\bwatermark\b/,
    params: (_clause, original) => {
      const text = textAfter(original, /\bwatermark\b/i);
      return text ? { text } : {};
    },
    label: (params) => (params.text ? `Watermark “${params.text}”` : "Add watermark"),
  },
  {
    tool: "pdf.ocr",
    pattern: /\bocr\b|\brecogni[sz]e\s+text\b|\bsearchable\b/,
    params: (clause) => {
      const languages = findLanguages(clause);
      return languages.length > 0 ? { languages } : {};
    },
    label: (params) =>
      Array.isArray(params.languages) ? `OCR (${params.languages.join(", ")})` : "OCR",
  },
  { tool: "ai.summarize", pattern: /\b(?:summari[sz]e|summary|tl;?dr)\b/ },
  {
    tool: "ai.translate",
    pattern: /\btranslate\b/,
    params: (clause) => {
      const target = /\b(?:to|into)\s+([a-z]+(?:\s+[a-z]+)?)/.exec(clause)?.[1];
      return target ? { target_lang: target } : {};
    },
    label: (params) => (params.target_lang ? `Translate to ${params.target_lang}` : "Translate"),
  },
  { tool: "pdf.encrypt", pattern: /\b(?:encrypt|password[\s-]protect)\b/ },
  { tool: "pdf.redact", pattern: /\bredact\b/ },
  { tool: "pdf.sign", pattern: /\b(?:digitally|digital)\s+sign\b/ },
  { tool: "pdf.sign_visual", pattern: /\bsign\b/ },
  { tool: "pdf.page_numbers", pattern: /\bpage\s+numbers?\b|\bnumber\s+(?:the\s+)?pages\b/ },
];

/** Words that carry no tool meaning when a clause is matched by search instead of an intent. */
const FILLER =
  /\b(?:please|pdf|file|document|doc|my|the|this|it|then|also|add|apply|run|do|and|to|a|an|all|in|on|of)\b/g;

const SEPARATOR =
  /\s*(?:,|;|->|→|=>|&|\band\s+then\b|\bthen\b|\bafter\s+that\b|\bafterwards\b|\bnext\b|\bfinally\b|\bplus\b|\band\b)\s*/i;

/** Text that holds a connector without being one: quotes, page lists, "black and white". */
const PROTECTED = new RegExp(
  String.raw`["“][^"”]*["”]|black\s+and\s+white|${PAGE_ITEM}(?:\s*,\s*${PAGE_ITEM})+`,
  "gi",
);

/** Marks protected text while the sentence is split (a private-use character, never typed). */
const MASK = "\uE000";
const MASKED = new RegExp(`${MASK}(\\d+)${MASK}`, "g");

/** Splits a sentence into one clause per step, leaving protected text whole. */
function splitClauses(query: string): string[] {
  const kept: string[] = [];
  const masked = query.replace(PROTECTED, (match) => {
    kept.push(match);
    return `${MASK}${kept.length - 1}${MASK}`;
  });
  return masked
    .split(SEPARATOR)
    .map((clause) => clause.replace(MASKED, (_match, index: string) => kept[Number(index)] ?? ""))
    .map((clause) => clause.trim())
    .filter(Boolean);
}

/** Keys of a tool's params schema, so a parsed parameter is only kept when the tool has it. */
function schemaKeys(tool: ToolDescriptor): Set<string> {
  const properties = (tool.params_schema as { properties?: Record<string, unknown> } | undefined)
    ?.properties;
  return new Set(Object.keys(properties ?? {}));
}

function resolveClause(
  original: string,
  catalog: ToolDescriptor[],
  entries: HubTool[],
): RecipeStepDraft | null {
  const clause = original.toLowerCase();
  const byId = new Map(catalog.map((tool) => [tool.id, tool]));
  for (const intent of INTENTS) {
    const tool = byId.get(intent.tool);
    if (!tool || !intent.pattern.test(clause)) {
      continue;
    }
    const keys = schemaKeys(tool);
    const parsed = intent.params?.(clause, original) ?? {};
    const params = Object.fromEntries(Object.entries(parsed).filter(([key]) => keys.has(key)));
    return {
      toolId: tool.id,
      tool,
      params,
      label: intent.label?.(params) ?? toolTitle(tool),
    };
  }
  // No intent phrase: accept the best search hit only when the clause is a real tool query.
  const stripped = clause.replace(FILLER, " ").replace(/\s+/g, " ").trim();
  const [best] = stripped ? searchTools(entries, stripped) : [];
  return best ? { toolId: best.id, tool: best.tool, params: {}, label: best.title } : null;
}

/**
 * Turns a plain sentence such as "rotate 90 degrees then compress" into recipe steps.
 *
 * Returns `null` when the text is an ordinary single-tool search (one tool, no parameters) or
 * when any part of it names no tool, so the palette falls back to normal search. A multi-step
 * sentence, or a single step with a parameter ("rotate 90"), yields a draft to review.
 */
export function parseNaturalLanguageRecipe(
  query: string,
  catalog: ToolDescriptor[] = [],
): RecipeDraft | null {
  const text = query.trim();
  if (!text || catalog.length === 0) {
    return null;
  }
  const entries = describeTools(catalog);
  const steps: RecipeStepDraft[] = [];
  for (const clause of splitClauses(text)) {
    // "ocr in chinese and english": the second clause only adds a language to the OCR step.
    const previousStep = steps[steps.length - 1];
    const extra = previousStep?.toolId === "pdf.ocr" ? languagesOnly(clause) : [];
    if (previousStep && extra.length > 0) {
      const known = Array.isArray(previousStep.params.languages)
        ? previousStep.params.languages
        : [];
      previousStep.params = {
        ...previousStep.params,
        languages: [...new Set([...known, ...extra])],
      };
      previousStep.label = `OCR (${(previousStep.params.languages as string[]).join(", ")})`;
      continue;
    }
    const step = resolveClause(clause, catalog, entries);
    if (!step) {
      return null;
    }
    const previous = steps[steps.length - 1];
    if (previous && previous.toolId === step.toolId) {
      previous.params = { ...previous.params, ...step.params };
      continue;
    }
    steps.push(step);
  }
  const parameterized = steps.some((step) => Object.keys(step.params).length > 0);
  if (steps.length === 0 || (steps.length === 1 && !parameterized)) {
    return null;
  }
  return { steps, requiresConfirmation: steps.some(stepNeedsConfirmation) };
}

const IRREVERSIBLE = new Set(["pdf.redact", "pdf.sanitize", "pdf.encrypt", "pdf.decrypt"]);

function stepNeedsConfirmation(step: RecipeStepDraft): boolean {
  return (
    primaryRuntime(step.tool) !== "browser" ||
    toolTier(step.tool) === "pro" ||
    step.toolId.startsWith("ai.") ||
    IRREVERSIBLE.has(step.toolId)
  );
}
