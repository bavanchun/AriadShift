// `ariad-recipe/1` in the browser: the wire types, secret redaction, the naming and condition
// grammars, and the URL-fragment codec. It mirrors `crates/ariad-core/src/tools/recipe.rs` and
// `secret.rs`, so a recipe built here is accepted by `ashift recipe run` unchanged.
//
// AD-18: a secret is never written to JSON, `localStorage` or a URL. Every secret parameter
// becomes `{"$secret":"prompt"}`, and anything loaded from a link or a file is `untrusted`
// until the user confirms it.

export const RECIPE_VERSION = "ariad-recipe/1";
export const MAX_RECIPE_STEPS = 32;
export const MAX_RECIPE_BYTES = 64 * 1024;
export const FRAGMENT_KEY = "recipe";

export type StepErrorPolicy = "stop" | "skip" | "continue";
export type ConvertProfile = "editable" | "faithful" | "fast" | "private";
const PROFILES: readonly string[] = ["editable", "faithful", "fast", "private"];
const ERROR_POLICIES: readonly string[] = ["stop", "skip", "continue"];

export interface RecipeToolStep {
  tool: string;
  params?: Record<string, unknown>;
  if?: string;
  on_error?: StepErrorPolicy;
}

export interface RecipeConvertStep {
  convert: { to: string; profile?: ConvertProfile };
  if?: string;
  on_error?: StepErrorPolicy;
}

export type RecipeStep = RecipeToolStep | RecipeConvertStep;

/**
 * The wire document. The Rust schema denies unknown fields, so there is deliberately no
 * display `name`: an exported file with one would be refused by `ashift recipe run`.
 */
export interface AriadRecipe {
  version: typeof RECIPE_VERSION;
  steps: RecipeStep[];
  naming?: string;
}

export type SecretRef = { $secret: "prompt" };
export const SECRET_PROMPT: Readonly<SecretRef> = Object.freeze({ $secret: "prompt" });

/** `KNOWN_SECRET_FIELD_NAMES` from `secret.rs`, plus the aliases older recipes used. */
export const SECRET_FIELD_NAMES: readonly string[] = [
  "certificate",
  "password",
  "user_password",
  "owner_password",
  "open_password",
  "passphrase",
  "key_password",
  "p12_password",
  "pkcs12_password",
  "pfx_password",
  "pin",
  "token_pin",
  "token",
  "secret",
  "api_key",
];

/** Tools an imported recipe may not run (`NETWORK_FETCH_TOOLS`, watch tools). */
const FORBIDDEN_IMPORTED_TOOLS: readonly string[] = [
  "pdf.from_url",
  "web.to_pdf",
  "automation.watch",
];
const NAMING_TOKENS: readonly string[] = ["stem", "n", "date", "title"];
const TOOL_ID = /^[a-z][a-z0-9_]*(\.[a-z0-9_]+)+$/;

export type RecipeErrorCode =
  | "too_large"
  | "invalid_encoding"
  | "invalid_json"
  | "invalid_version"
  | "empty_steps"
  | "too_many_steps"
  | "invalid_step"
  | "invalid_condition"
  | "invalid_naming"
  | "literal_secret"
  | "forbidden";

export class RecipeFormatError extends Error {
  readonly code: RecipeErrorCode;
  readonly stepIndex?: number;
  constructor(code: RecipeErrorCode, message: string, stepIndex?: number) {
    super(message);
    this.name = "RecipeFormatError";
    this.code = code;
    this.stepIndex = stepIndex;
  }
}

/** The part of a registry tool the recipe layer needs: which parameters are secret. */
export interface SecretAwareTool {
  id: string;
  secret_params?: readonly string[];
}

export interface RecipeOptions {
  /** Registry tools; their `secret_params` extend the built-in secret field names. */
  tools?: readonly SecretAwareTool[];
}

type Json = Record<string, unknown>;

const isObject = (value: unknown): value is Json =>
  typeof value === "object" && value !== null && !Array.isArray(value);

export const isConvertStep = (step: RecipeStep): step is RecipeConvertStep => "convert" in step;

/* ---------------------------------------------------------------- secrets */

function secretNames(step: RecipeStep, options: RecipeOptions): Set<string> {
  const names = new Set(SECRET_FIELD_NAMES);
  if (!isConvertStep(step)) {
    const tool = options.tools?.find((entry) => entry.id === step.tool);
    for (const field of tool?.secret_params ?? []) names.add(field.toLowerCase());
  }
  return names;
}

/** Whether a parameter value is the `{"$secret":"prompt"}` reference. */
export const isPromptSecret = (value: unknown): value is SecretRef =>
  isObject(value) && Object.keys(value).length === 1 && value.$secret === "prompt";

/** Replaces every secret field, at any depth, with `{"$secret":"prompt"}`. */
function redactValue(value: unknown, names: Set<string>): unknown {
  if (Array.isArray(value)) return value.map((item) => redactValue(item, names));
  if (!isObject(value)) return value;
  return Object.fromEntries(
    Object.keys(value)
      .sort()
      .map((key) => [
        key,
        names.has(key.toLowerCase()) ? { ...SECRET_PROMPT } : redactValue(value[key], names),
      ]),
  );
}

/** Whether `name` is a secret parameter name, built in or declared by `tool`. */
export function isSecretField(name: string, tool?: SecretAwareTool): boolean {
  const lower = name.toLowerCase();
  return (
    SECRET_FIELD_NAMES.includes(lower) ||
    (tool?.secret_params ?? []).some((field) => field.toLowerCase() === lower)
  );
}

/**
 * The recipe form of parameters entered in a form: a secret that was typed becomes
 * `{"$secret":"prompt"}`, an empty one is dropped, and the typed value is not kept.
 */
export function redactParams(
  params: Record<string, unknown>,
  tool: SecretAwareTool | undefined,
): Record<string, unknown> {
  const names = new Set(SECRET_FIELD_NAMES);
  for (const field of tool?.secret_params ?? []) names.add(field.toLowerCase());
  return Object.fromEntries(
    Object.entries(params).flatMap(([key, value]) => {
      if (!names.has(key.toLowerCase())) return [[key, redactValue(value, names)]];
      return value === undefined || value === "" || value === null
        ? []
        : [[key, { ...SECRET_PROMPT }]];
    }),
  );
}

/** Walks `params` and calls `visit` with the dotted path of each secret field. */
function visitSecrets(
  value: unknown,
  names: Set<string>,
  visit: (path: string, value: unknown) => void,
  path = "",
): void {
  if (Array.isArray(value)) {
    value.forEach((item, index) => {
      visitSecrets(item, names, visit, `${path}[${index}]`);
    });
    return;
  }
  if (!isObject(value)) return;
  for (const [key, child] of Object.entries(value)) {
    const here = path ? `${path}.${key}` : key;
    if (names.has(key.toLowerCase())) visit(here, child);
    else visitSecrets(child, names, visit, here);
  }
}

/**
 * The secrets a recipe asks the user for at run time, as `"<step index>:<parameter path>"`,
 * for example `"2:password"`. The values are collected in memory and never serialised.
 */
export function hasPromptSecrets(recipe: AriadRecipe, options: RecipeOptions = {}): string[] {
  const found: string[] = [];
  recipe.steps.forEach((step, index) => {
    if (isConvertStep(step)) return;
    visitSecrets(step.params, secretNames(step, options), (path, value) => {
      if (isPromptSecret(value)) found.push(`${index}:${path}`);
    });
  });
  return found;
}

/* ---------------------------------------------------------------- grammars */

/** Port of `validate_naming_template`; returns an error message, or `null` when valid. */
export function validateNamingTemplate(template: string): string | null {
  if (!template.trim()) return "The naming template must not be empty.";
  if (template.startsWith(".") || template.includes(".."))
    return "The naming template must not start with '.' or contain '..'.";
  for (let index = 0; index < template.length; index += 1) {
    const ch = template.charAt(index);
    if (ch === "{") {
      const close = template.indexOf("}", index + 1);
      const token = close < 0 ? "" : template.slice(index + 1, close);
      if (close < 0 || token.includes("{")) return "The naming template has unbalanced braces.";
      if (!NAMING_TOKENS.includes(token))
        return `Unknown token {${token}}; allowed tokens are {stem}, {n}, {date} and {title}.`;
      index = close;
    } else if (ch === "}") {
      return "The naming template has unbalanced braces.";
    } else if (!/[A-Za-z0-9._ -]/.test(ch)) {
      return `The character '${ch}' is not allowed; use letters, digits, '.', '_', '-' and spaces.`;
    }
  }
  return null;
}

/** Port of `parse_condition`; returns an error message, or `null` when valid. */
export function validateStepCondition(expression: string): string | null {
  const expr = expression.trim();
  const bare = expr.startsWith("!") ? expr.slice(1).trim() : expr;
  if (["needs_ocr", "is_encrypted", "has_signatures"].includes(bare)) return null;
  const match = /^([A-Za-z0-9_]+)\s*(>=|<=|==|!=|>|<)\s*(.*)$/.exec(expr);
  if (!match) return "Expected a known field and a comparison.";
  const [, field = "", op = "", value = ""] = match;
  if (field === "page_count" || field === "size_bytes")
    return /^[0-9]+$/.test(value) ? null : "Expected an unsigned integer.";
  if (op !== "==" && op !== "!=") return "This field supports only == and !=.";
  if (["needs_ocr", "is_encrypted", "has_signatures"].includes(field))
    return value === "true" || value === "false" ? null : "Expected true or false.";
  if (field === "format")
    return /^(['"])[A-Za-z0-9_]+\1$/.test(value) ? null : "Expected a quoted format identifier.";
  return `Unknown condition field ${field}.`;
}

export interface NamingExample {
  stem: string;
  n?: number;
  date?: string;
  title?: string;
  ext?: string;
}

/** The file name a template produces for an example input, as the host would render it. */
export function previewOutputName(template: string | undefined, example: NamingExample): string {
  const ext = example.ext ?? "pdf";
  if (!template) return `${example.stem}.${ext}`;
  const values: Record<string, string> = {
    stem: example.stem,
    n: String(example.n ?? 1).padStart(3, "0"),
    date: example.date ?? new Date().toISOString().slice(0, 10),
    title: example.title ?? example.stem,
  };
  return `${template.replace(/\{(\w+)\}/g, (_all, token: string) => values[token] ?? "")}.${ext}`;
}

/* ---------------------------------------------------------------- validation */

const ALLOWED_STEP_KEYS = new Set(["tool", "params", "if", "on_error", "convert"]);

function checkStep(
  raw: unknown,
  index: number,
  origin: "trusted" | "untrusted",
  options: RecipeOptions,
): RecipeStep {
  const fail = (code: RecipeErrorCode, message: string) => {
    throw new RecipeFormatError(code, `Step ${index + 1}: ${message}`, index);
  };
  if (!isObject(raw)) return fail("invalid_step", "must be an object.");
  for (const key of Object.keys(raw))
    if (!ALLOWED_STEP_KEYS.has(key)) fail("invalid_step", `unknown field "${key}".`);
  if (raw.if !== undefined) {
    if (typeof raw.if !== "string") fail("invalid_condition", "the condition must be a string.");
    const problem = validateStepCondition(raw.if as string);
    if (problem) fail("invalid_condition", `invalid condition "${raw.if}": ${problem}`);
  }
  if (raw.on_error !== undefined && !ERROR_POLICIES.includes(String(raw.on_error)))
    fail("invalid_step", 'on_error must be "stop", "skip" or "continue".');
  const common = {
    ...(raw.if === undefined ? {} : { if: raw.if as string }),
    ...(raw.on_error === undefined ? {} : { on_error: raw.on_error as StepErrorPolicy }),
  };

  if ("convert" in raw) {
    if ("tool" in raw || "params" in raw) fail("invalid_step", "a step is a tool or a convert.");
    const convert = raw.convert;
    if (!isObject(convert) || typeof convert.to !== "string" || !convert.to)
      return fail("invalid_step", "convert needs a target format in `to`.");
    for (const key of Object.keys(convert))
      if (key !== "to" && key !== "profile")
        fail("invalid_step", `unknown convert field "${key}".`);
    if (convert.profile !== undefined && !PROFILES.includes(String(convert.profile)))
      fail("invalid_step", "profile must be editable, faithful, fast or private.");
    return {
      convert: {
        to: convert.to,
        ...(convert.profile === undefined ? {} : { profile: convert.profile as ConvertProfile }),
      },
      ...common,
    };
  }

  if (typeof raw.tool !== "string" || !TOOL_ID.test(raw.tool))
    return fail("invalid_step", "needs a tool id such as pdf.merge.");
  if (raw.params !== undefined && !isObject(raw.params))
    return fail("invalid_step", `params of ${raw.tool} must be an object.`);
  if (origin === "untrusted" && FORBIDDEN_IMPORTED_TOOLS.includes(raw.tool))
    return fail("forbidden", `an imported recipe may not run ${raw.tool}.`);
  const step: RecipeToolStep = {
    tool: raw.tool,
    ...(raw.params === undefined ? {} : { params: raw.params }),
    ...common,
  };
  // Only `{"$secret":"prompt"}` may appear on a secret field: no literal, no handle.
  visitSecrets(step.params, secretNames(step, options), (path, value) => {
    if (!isPromptSecret(value))
      fail(
        "literal_secret",
        `${path} must be {"$secret":"prompt"}; recipes never carry a secret value.`,
      );
  });
  return step;
}

/**
 * Validates an untrusted value against the `ariad-recipe/1` contract and returns the recipe
 * with unknown-field-free steps. Throws {@link RecipeFormatError} on the first violation.
 */
export function parseRecipe(
  value: unknown,
  origin: "trusted" | "untrusted" = "untrusted",
  options: RecipeOptions = {},
): AriadRecipe {
  if (!isObject(value)) throw new RecipeFormatError("invalid_json", "A recipe is a JSON object.");
  for (const key of Object.keys(value)) {
    if (key === "watch" && origin === "untrusted")
      throw new RecipeFormatError(
        "forbidden",
        "An imported recipe may not configure watch-folder automation.",
      );
    if (!["version", "steps", "naming", "watch"].includes(key))
      throw new RecipeFormatError("invalid_json", `Unknown recipe field "${key}".`);
  }
  if (value.version !== RECIPE_VERSION)
    throw new RecipeFormatError(
      "invalid_version",
      `Unsupported recipe version ${JSON.stringify(value.version)}; expected "${RECIPE_VERSION}".`,
    );
  if (!Array.isArray(value.steps) || value.steps.length === 0)
    throw new RecipeFormatError("empty_steps", "A recipe needs at least one step.");
  if (value.steps.length > MAX_RECIPE_STEPS)
    throw new RecipeFormatError(
      "too_many_steps",
      `A recipe may have at most ${MAX_RECIPE_STEPS} steps.`,
    );
  let naming: string | undefined;
  if (value.naming !== undefined) {
    const problem =
      typeof value.naming === "string"
        ? validateNamingTemplate(value.naming)
        : "The naming template must be a string.";
    if (problem) throw new RecipeFormatError("invalid_naming", problem);
    naming = value.naming as string;
  }
  return {
    version: RECIPE_VERSION,
    steps: value.steps.map((step, index) => checkStep(step, index, origin, options)),
    ...(naming === undefined ? {} : { naming }),
  };
}

/* ---------------------------------------------------------------- serialisation */

/** The canonical, redacted wire object: Rust's field order, sorted `params` keys. */
function redactedRecipe(recipe: AriadRecipe, options: RecipeOptions): Json {
  const steps = recipe.steps.map((step) => {
    const common = {
      ...(step.if === undefined ? {} : { if: step.if }),
      ...(step.on_error === undefined ? {} : { on_error: step.on_error }),
    };
    if (isConvertStep(step)) return { convert: { ...step.convert }, ...common };
    return {
      tool: step.tool,
      params: redactValue(step.params ?? {}, secretNames(step, options)),
      ...common,
    };
  });
  return {
    version: RECIPE_VERSION,
    steps,
    ...(recipe.naming === undefined ? {} : { naming: recipe.naming }),
  };
}

/** `ariad-recipe/1` JSON, pretty-printed, with every secret replaced by `{"$secret":"prompt"}`. */
export function serializeRecipe(recipe: AriadRecipe, options: RecipeOptions = {}): string {
  return JSON.stringify(redactedRecipe(recipe, options), null, 2);
}

/* ---------------------------------------------------------------- URL fragment */

function toBase64Url(bytes: Uint8Array): string {
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000)
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  return btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
}

function fromBase64Url(text: string): Uint8Array {
  text = text.replace(/=+$/, "");
  if (!/^[A-Za-z0-9_-]*$/.test(text))
    throw new RecipeFormatError("invalid_encoding", "The recipe link is not valid base64url.");
  try {
    const binary = atob(text.replaceAll("-", "+").replaceAll("_", "/"));
    return Uint8Array.from(binary, (char) => char.charCodeAt(0));
  } catch {
    throw new RecipeFormatError("invalid_encoding", "The recipe link is not valid base64url.");
  }
}

/** `#recipe=<base64url>` for the redacted recipe. Nothing is sent to a server. */
export function encodeRecipeFragment(recipe: AriadRecipe, options: RecipeOptions = {}): string {
  const json = JSON.stringify(redactedRecipe(recipe, options));
  const bytes = new TextEncoder().encode(json);
  if (bytes.length > MAX_RECIPE_BYTES)
    throw new RecipeFormatError(
      "too_large",
      `The recipe is larger than ${MAX_RECIPE_BYTES / 1024} KiB and cannot be shared as a link.`,
    );
  return `#${FRAGMENT_KEY}=${toBase64Url(bytes)}`;
}

export interface DecodedRecipe {
  recipe: AriadRecipe;
  /** A recipe from a link or a file is never trusted: the UI must ask before it runs. */
  origin: "untrusted";
}

/** Parses pasted or dropped `ariad-recipe/1` JSON as an untrusted recipe. */
export function importRecipeJson(text: string, options: RecipeOptions = {}): DecodedRecipe {
  if (new TextEncoder().encode(text).length > MAX_RECIPE_BYTES)
    throw new RecipeFormatError(
      "too_large",
      `The recipe is larger than ${MAX_RECIPE_BYTES / 1024} KiB.`,
    );
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new RecipeFormatError("invalid_json", "The recipe is not valid JSON.");
  }
  return { recipe: parseRecipe(value, "untrusted", options), origin: "untrusted" };
}

/**
 * Reads `#recipe=<base64url>` (also `recipe=…` or the bare payload) from a link fragment.
 * The size is checked before decoding so an oversized link is refused cheaply.
 */
export function decodeRecipeFragment(
  fragmentOrHash: string,
  options: RecipeOptions = {},
): DecodedRecipe {
  let payload = fragmentOrHash.trim().replace(/^#/, "");
  if (payload.startsWith(`${FRAGMENT_KEY}=`)) payload = payload.slice(FRAGMENT_KEY.length + 1);
  if (Math.ceil((payload.length * 3) / 4) > MAX_RECIPE_BYTES + 3)
    throw new RecipeFormatError(
      "too_large",
      `The recipe link is larger than ${MAX_RECIPE_BYTES / 1024} KiB.`,
    );
  let text: string;
  try {
    text = new TextDecoder("utf-8", { fatal: true }).decode(fromBase64Url(payload));
  } catch (error) {
    if (error instanceof RecipeFormatError) throw error;
    throw new RecipeFormatError("invalid_encoding", "The recipe link is not valid UTF-8.");
  }
  return importRecipeJson(text, options);
}
