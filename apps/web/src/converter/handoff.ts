import { type ToolDescriptor, toolCatalog } from "./tool-catalog";

export interface ToolHandoff {
  executionMode: "browser" | "cloud" | "desktop";
  cliCommand: string;
  installUrl: string;
  deepLinkUrl: string;
}

/** POSIX shell quoting, including quotes, substitutions, newlines and leading dashes. */
export function shellQuote(value: string): string {
  if (value.includes("\0")) throw new Error("Shell arguments cannot contain NUL bytes.");
  return `'${value.replaceAll("'", `'"'"'`)}'`;
}

function executionMode(tool: ToolDescriptor, cloudApiUrl?: string): ToolHandoff["executionMode"] {
  if (tool.runtimes.includes("wasm")) return "browser";
  if (tool.runtimes.includes("cloud") && cloudApiUrl?.trim()) return "cloud";
  return "desktop";
}

export function toolHandoff(
  tool: ToolDescriptor,
  params: Record<string, unknown>,
  inputNames: string[],
  options: { cloudApiUrl?: string; locale?: string; revealSecrets?: boolean } = {},
): ToolHandoff {
  const displayParams = Object.fromEntries(
    Object.entries(params).map(([key, value]) => [
      key,
      tool.secret_params.includes(key) && !options.revealSecrets ? "***" : value,
    ]),
  );
  // Prefix relative option-like filenames so the CLI cannot parse them as flags.
  const inputs = inputNames.map((name) => shellQuote(name.startsWith("-") ? `./${name}` : name));
  const args = ["ashift", "tool", shellQuote(tool.id), ...inputs];
  if (Object.keys(displayParams).length)
    args.push("--params", shellQuote(JSON.stringify(displayParams)));
  const locale =
    options.locale && /^[A-Za-z]{2,3}(?:-[A-Za-z0-9]{2,8})*$/.test(options.locale)
      ? `/${options.locale}`
      : "";
  return {
    executionMode: executionMode(tool, options.cloudApiUrl),
    cliCommand: args.join(" "),
    installUrl: `${locale}/download`,
    deepLinkUrl: `ariadshift://tool/${encodeURIComponent(tool.id)}`,
  };
}

export interface RecipeHandoffOptions {
  locale?: string;
  recipeName?: string;
  outputDir?: string;
}

export interface BatchHandoffOptions extends RecipeHandoffOptions {
  cloudApiUrl?: string;
  watchDir?: string;
}

export interface RecipeHandoff {
  cliCommand: string;
  /** Save these redacted bytes as recipeName before running the command. */
  recipeJson: string;
}

const recipeSecretNames = new Set([
  "password",
  "user_password",
  "owner_password",
  "passphrase",
  "api_key",
  "token",
  "pin",
  "secret",
  "pkcs12_password",
  "key_password",
  "p12_password",
  "pfx_password",
  "token_pin",
  ...toolCatalog().flatMap((tool) => tool.secret_params),
]);

function secretName(name: string, additional: string[] = []): boolean {
  return (
    recipeSecretNames.has(name.toLowerCase().replaceAll("-", "_")) ||
    additional.some((key) => key.toLowerCase() === name.toLowerCase())
  );
}

/** Clone rather than mutate a recipe that may still hold secrets in browser memory. */
function redactRecipe(value: unknown, additional: string[] = []): unknown {
  if (Array.isArray(value)) return value.map((entry) => redactRecipe(entry, additional));
  if (typeof value !== "object" || value === null) return value;
  if (Object.keys(value).some((key) => key.toLowerCase() === "$secret"))
    return { $secret: "prompt" };
  return Object.fromEntries(
    Object.entries(value).map(([key, entry]) => [
      key,
      secretName(key, additional) ? { $secret: "prompt" } : redactRecipe(entry, additional),
    ]),
  );
}

function pathArgument(path: string): string {
  return shellQuote(path.startsWith("-") ? `./${path}` : path);
}

export function recipeHandoff(
  recipeJson: string | Record<string, unknown>,
  inputNames: string[],
  options: RecipeHandoffOptions = {},
): RecipeHandoff {
  let recipe: unknown = recipeJson;
  if (typeof recipeJson === "string") {
    try {
      recipe = JSON.parse(recipeJson);
    } catch {
      throw new Error("The recipe JSON is invalid.");
    }
  }
  if (typeof recipe !== "object" || recipe === null || Array.isArray(recipe))
    throw new Error("A recipe must be a JSON object.");
  return {
    recipeJson: JSON.stringify(redactRecipe(recipe), null, 2),
    // Imported recipes deliberately retain the CLI's interactive confirmation gate.
    cliCommand: [
      "ashift",
      "recipe",
      "run",
      pathArgument(options.recipeName ?? "recipe.json"),
      ...inputNames.map(pathArgument),
      "-o",
      pathArgument(options.outputDir ?? "out/"),
    ].join(" "),
  };
}

export function batchHandoff(
  tool: ToolDescriptor,
  params: Record<string, unknown>,
  inputNames: string[],
  options: BatchHandoffOptions = {},
): ToolHandoff & RecipeHandoff {
  const redacted = redactRecipe(params, tool.secret_params) as Record<string, unknown>;
  const recipe = recipeHandoff(
    {
      version: "ariad-recipe/1",
      steps: [{ tool: tool.id, params: redacted }],
    },
    inputNames,
    options,
  );
  let cliCommand = recipe.cliCommand;
  if (options.watchDir !== undefined) {
    cliCommand = [
      "ashift",
      "recipe",
      "run",
      pathArgument(options.recipeName ?? "recipe.json"),
      "--watch",
      pathArgument(options.watchDir),
      "-o",
      pathArgument(options.outputDir ?? "out/"),
    ].join(" ");
  } else if (tool.inputs.arity !== "one" || inputNames.length <= 1) {
    const args = [
      "ashift",
      "tool",
      shellQuote(tool.id),
      ...inputNames.map(pathArgument),
      "-o",
      pathArgument(options.outputDir ?? "out/"),
    ];
    for (const [key, value] of Object.entries(redacted)) {
      if (secretName(key, tool.secret_params)) {
        // This is a local file placeholder, never an inline secret or shell expansion.
        const file = `${key.replace(/[^A-Za-z0-9_-]/g, "_")}.txt`;
        args.push("--secret", shellQuote(`${key}=@${file}`));
      } else args.push("--param", shellQuote(`${key}=${JSON.stringify(value)}`));
    }
    cliCommand = args.join(" ");
  }
  return {
    ...toolHandoff(tool, {}, [], options),
    ...recipe,
    cliCommand,
  };
}
