import type { ToolDescriptor } from "./tool-catalog";

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
