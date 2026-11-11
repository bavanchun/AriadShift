"use client";

import { Alert, AlertDescription } from "@ariadshift/ui/components/alert";
import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { cn } from "@ariadshift/ui/lib/utils";
import {
  ArrowDownIcon,
  ArrowUpIcon,
  CopyIcon,
  DownloadIcon,
  Link2Icon,
  PlayIcon,
  PlusIcon,
  ShieldCheckIcon,
  Trash2Icon,
  UploadIcon,
} from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { ParamsForm, type ParamsValidation } from "@/components/tools/params-form";
import type { ToolDescriptor } from "@/converter/tool-catalog";
import {
  type AriadRecipe,
  type ConvertProfile,
  type DecodedRecipe,
  encodeRecipeFragment,
  hasPromptSecrets,
  importRecipeJson,
  isConvertStep,
  isPromptSecret,
  isSecretField,
  previewOutputName,
  RECIPE_VERSION,
  RecipeFormatError,
  type RecipeStep,
  redactParams,
  SECRET_PROMPT,
  serializeRecipe,
  validateNamingTemplate,
  validateStepCondition,
} from "@/lib/recipe";
import { toolTitle } from "@/lib/tools";
import { type RecipeConfirmCopy, RecipeConfirmDialog } from "./recipe-confirm-dialog";

export interface RecipeBuilderCopy {
  title: string;
  steps: (count: number) => string;
  empty: string;
  step: (index: number) => string;
  addStep: string;
  addStepPicker: string;
  convertOption: string;
  convertTo: string;
  convertProfile: string;
  profileDefault: string;
  moveUp: (index: number) => string;
  moveDown: (index: number) => string;
  remove: (index: number) => string;
  condition: string;
  conditionHint: string;
  unknownTool: string;
  naming: string;
  namingHint: string;
  namingPreview: (name: string) => string;
  exportJson: string;
  shareLink: string;
  linkCopied: string;
  secretsReplaced: string;
  run: string;
  importTitle: string;
  importPaste: string;
  importButton: string;
  importFile: string;
  cliTitle: string;
  copyCommand: string;
  copied: string;
}

export const DEFAULT_RECIPE_BUILDER_COPY: RecipeBuilderCopy = {
  title: "Recipe builder",
  steps: (count) => `${count} ${count === 1 ? "step" : "steps"}`,
  empty: "Add a tool to start a recipe.",
  step: (index) => `Step ${index}`,
  addStep: "Add step",
  addStepPicker: "Tool to add",
  convertOption: "Convert to a format",
  convertTo: "Convert to",
  convertProfile: "Profile",
  profileDefault: "Default",
  moveUp: (index) => `Move step ${index} up`,
  moveDown: (index) => `Move step ${index} down`,
  remove: (index) => `Remove step ${index}`,
  condition: "Condition (optional)",
  conditionHint: "For example page_count > 1 or needs_ocr == true",
  unknownTool: "This tool is not available here; its parameters are kept as they are.",
  naming: "Output name template",
  namingHint: "Tokens: {stem}, {n}, {date}, {title}",
  namingPreview: (name) => `Preview: ${name}`,
  exportJson: "Export JSON",
  shareLink: "Share link",
  linkCopied: "Link copied",
  secretsReplaced: 'Secrets replaced with {"$secret":"prompt"}',
  run: "Run recipe",
  importTitle: "Import a recipe",
  importPaste: "Paste ariad-recipe/1 JSON",
  importButton: "Import",
  importFile: "Import from file",
  cliTitle: "Run it on your computer",
  copyCommand: "Copy command",
  copied: "Copied",
};

export interface RecipeBuilderProps {
  /** The tool registry; steps choose from it and edit their parameters through `ParamsForm`. */
  tools: ToolDescriptor[];
  initialRecipe?: AriadRecipe;
  /** A recipe from a link or a file; it waits behind the confirmation gate (AD-18). */
  incoming?: DecodedRecipe | null;
  /** The page URL a shared link is built on; defaults to the current location without its hash. */
  shareBaseUrl?: string;
  copy?: Partial<RecipeBuilderCopy>;
  confirmCopy?: Partial<RecipeConfirmCopy>;
  onChange?: (recipe: AriadRecipe) => void;
  /** Called once the confirmation gate is decided, by loading the recipe or by cancelling. */
  onPendingResolved?: () => void;
  /** Runs the recipe. Offered only when the host can run it; disabled while a recipe awaits confirmation. */
  onRun?: (recipe: AriadRecipe) => void;
  /** Saves the exported JSON; defaults to a browser download named `recipe.json`. */
  onExportJson?: (json: string, filename: string) => void;
  className?: string;
}

interface Entry {
  id: string;
  step: RecipeStep;
  valid: boolean;
}

const CONVERT = "__convert__";
const PROFILES: ConvertProfile[] = ["editable", "faithful", "fast", "private"];
const FIELD =
  "h-8 w-full rounded-lg border bg-background px-2.5 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 aria-invalid:border-destructive";

function downloadText(text: string, filename: string): void {
  const url = URL.createObjectURL(new Blob([text], { type: "application/json" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.click();
  URL.revokeObjectURL(url);
}

/** Visual pipeline editor for `ariad-recipe/1`: steps, parameters, naming, share and import. */
export function RecipeBuilder({
  tools,
  initialRecipe,
  incoming = null,
  shareBaseUrl,
  copy: copyOverrides,
  confirmCopy,
  onChange,
  onPendingResolved,
  onRun,
  onExportJson = downloadText,
  className,
}: RecipeBuilderProps) {
  const copy = { ...DEFAULT_RECIPE_BUILDER_COPY, ...copyOverrides };
  const prefix = useId();
  const nextId = useRef(0);
  const makeEntry = (step: RecipeStep): Entry => {
    nextId.current += 1;
    return { id: `${prefix}-step-${nextId.current}`, step, valid: true };
  };
  const [entries, setEntries] = useState<Entry[]>(() =>
    (initialRecipe?.steps ?? []).map(makeEntry),
  );
  const [naming, setNaming] = useState(initialRecipe?.naming ?? "");
  const [pending, setPending] = useState<DecodedRecipe | null>(incoming);
  const [pick, setPick] = useState("");
  const [pasted, setPasted] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [notice, setNotice] = useState<"link" | "command" | null>(null);

  useEffect(() => {
    if (incoming) setPending(incoming);
  }, [incoming]);

  const toolById = useMemo(() => new Map(tools.map((tool) => [tool.id, tool])), [tools]);
  const sortedTools = useMemo(() => [...tools].sort((a, b) => a.id.localeCompare(b.id)), [tools]);
  const options = { tools };
  const recipe: AriadRecipe = useMemo(
    () => ({
      version: RECIPE_VERSION,
      steps: entries.map((entry) => entry.step),
      ...(naming.trim() ? { naming: naming.trim() } : {}),
    }),
    [entries, naming],
  );
  const onChangeRef = useRef(onChange);
  useEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);
  const lastEmitted = useRef<string | null>(null);
  useEffect(() => {
    const json = JSON.stringify(recipe);
    if (lastEmitted.current === json) return;
    lastEmitted.current = json;
    onChangeRef.current?.(recipe);
  }, [recipe]);

  const namingProblem = naming.trim() ? validateNamingTemplate(naming.trim()) : null;
  const conditionProblems = entries.map((entry) =>
    entry.step.if ? validateStepCondition(entry.step.if) : null,
  );
  const ready =
    entries.length > 0 &&
    !namingProblem &&
    conditionProblems.every((entry) => entry === null) &&
    entries.every((entry) => entry.valid);
  const promptCount = hasPromptSecrets(recipe, options).length;

  function updateStep(id: string, update: (step: RecipeStep) => RecipeStep, valid?: boolean) {
    setEntries((previous) =>
      previous.map((entry) =>
        entry.id === id
          ? { ...entry, step: update(entry.step), valid: valid ?? entry.valid }
          : entry,
      ),
    );
  }

  function setParams(id: string, params: Record<string, unknown>, validation: ParamsValidation) {
    setEntries((previous) =>
      previous.map((entry) => {
        if (entry.id !== id || isConvertStep(entry.step)) return entry;
        const descriptor = toolById.get(entry.step.tool);
        const next = redactParams(params, descriptor);
        // A required secret, or one already marked, stays a prompt: the value is asked for
        // in memory when the recipe runs and is never part of the recipe.
        const required = (descriptor?.params_schema as { required?: string[] } | undefined)
          ?.required;
        for (const key of required ?? [])
          if (isSecretField(key, descriptor) && !(key in next)) next[key] = { ...SECRET_PROMPT };
        for (const [key, value] of Object.entries(entry.step.params ?? {}))
          if (isPromptSecret(value) && !(key in next)) next[key] = { ...SECRET_PROMPT };
        const valid = Object.keys(validation.errors).every((key) => isSecretField(key, descriptor));
        const same = JSON.stringify(entry.step.params ?? {}) === JSON.stringify(next);
        return same && entry.valid === valid
          ? entry
          : { ...entry, valid, step: { ...entry.step, params: next } };
      }),
    );
  }

  function addStep() {
    if (!pick) return;
    const step: RecipeStep =
      pick === CONVERT ? { convert: { to: "pdf" } } : { tool: pick, params: {} };
    setEntries((previous) => [...previous, makeEntry(step)]);
    setPick("");
  }

  function move(index: number, by: -1 | 1) {
    setEntries((previous) => {
      const next = [...previous];
      const target = index + by;
      const [moved] = next.splice(index, 1);
      if (!moved || target < 0 || target > previous.length - 1) return previous;
      next.splice(target, 0, moved);
      return next;
    });
  }

  function load(loaded: AriadRecipe) {
    setEntries(loaded.steps.map(makeEntry));
    setNaming(loaded.naming ?? "");
    setProblem(null);
  }

  function importText(text: string) {
    try {
      setPending(importRecipeJson(text, options));
      setProblem(null);
    } catch (error) {
      setProblem(error instanceof RecipeFormatError ? error.message : String(error));
    }
  }

  async function share() {
    try {
      const fragment = encodeRecipeFragment(recipe, options);
      const base = shareBaseUrl ?? `${location.origin}${location.pathname}`;
      await navigator.clipboard.writeText(`${base}${fragment}`);
      setProblem(null);
      setNotice("link");
    } catch (error) {
      setProblem(error instanceof Error ? error.message : String(error));
    }
  }

  const command = "ashift recipe run recipe.json input.pdf -o out/";
  const exampleName = previewOutputName(naming.trim() || undefined, { stem: "report" });

  return (
    <section
      data-slot="recipe-builder"
      aria-label={copy.title}
      className={cn("grid gap-4 rounded-xl border bg-card p-4 text-card-foreground", className)}
    >
      <header className="flex flex-wrap items-center gap-2">
        <h2 className="text-sm font-semibold">{copy.title}</h2>
        <Badge variant="secondary">{copy.steps(entries.length)}</Badge>
        {promptCount > 0 && (
          <Badge variant="outline">
            <ShieldCheckIcon aria-hidden="true" />
            {copy.secretsReplaced}
          </Badge>
        )}
      </header>

      {entries.length === 0 ? (
        <p data-slot="recipe-empty" className="text-sm text-muted-foreground">
          {copy.empty}
        </p>
      ) : (
        <ol data-slot="recipe-steps" className="m-0 grid list-none gap-0 p-0">
          {entries.map((entry, index) => {
            const { step } = entry;
            const descriptor = isConvertStep(step) ? undefined : toolById.get(step.tool);
            const conditionProblem = conditionProblems[index];
            const fieldId = `${entry.id}-if`;
            return (
              <li key={entry.id} data-slot="recipe-step" className="relative flex gap-3 pb-4">
                <div className="relative flex flex-col items-center">
                  <span className="z-10 flex size-6 shrink-0 items-center justify-center rounded-full border border-thread bg-card text-xs font-medium text-thread">
                    {index + 1}
                  </span>
                  {index < entries.length - 1 && (
                    <span
                      aria-hidden="true"
                      data-slot="recipe-thread"
                      className="absolute top-6 -bottom-4 w-0.5 rounded-full bg-thread"
                    />
                  )}
                </div>
                <div className="grid min-w-0 flex-1 gap-2 rounded-lg border p-3">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="sr-only">{copy.step(index + 1)}</span>
                    <span className="min-w-0 flex-1 truncate text-sm font-medium">
                      {isConvertStep(step) ? copy.convertOption : toolTitle({ id: step.tool })}
                    </span>
                    {!isConvertStep(step) && (
                      <code className="text-xs text-muted-foreground">{step.tool}</code>
                    )}
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      aria-label={copy.moveUp(index + 1)}
                      disabled={index === 0}
                      onClick={() => move(index, -1)}
                    >
                      <ArrowUpIcon aria-hidden="true" />
                    </Button>
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      aria-label={copy.moveDown(index + 1)}
                      disabled={index === entries.length - 1}
                      onClick={() => move(index, 1)}
                    >
                      <ArrowDownIcon aria-hidden="true" />
                    </Button>
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      aria-label={copy.remove(index + 1)}
                      onClick={() => setEntries((previous) => previous.filter((e) => e !== entry))}
                    >
                      <Trash2Icon aria-hidden="true" />
                    </Button>
                  </div>

                  {isConvertStep(step) ? (
                    <div className="grid gap-2 sm:grid-cols-2">
                      <label className="grid gap-1 text-xs">
                        {copy.convertTo}
                        <input
                          className={FIELD}
                          value={step.convert.to}
                          onChange={(event) =>
                            updateStep(
                              entry.id,
                              (current) =>
                                isConvertStep(current)
                                  ? {
                                      ...current,
                                      convert: { ...current.convert, to: event.target.value },
                                    }
                                  : current,
                              Boolean(event.target.value.trim()),
                            )
                          }
                        />
                      </label>
                      <label className="grid gap-1 text-xs">
                        {copy.convertProfile}
                        <select
                          className={FIELD}
                          value={step.convert.profile ?? ""}
                          onChange={(event) =>
                            updateStep(entry.id, (current) => {
                              if (!isConvertStep(current)) return current;
                              const { profile: _drop, ...rest } = current.convert;
                              return {
                                ...current,
                                convert: event.target.value
                                  ? { ...rest, profile: event.target.value as ConvertProfile }
                                  : rest,
                              };
                            })
                          }
                        >
                          <option value="">{copy.profileDefault}</option>
                          {PROFILES.map((profile) => (
                            <option key={profile} value={profile}>
                              {profile}
                            </option>
                          ))}
                        </select>
                      </label>
                    </div>
                  ) : descriptor ? (
                    <ParamsForm
                      tool={descriptor}
                      initialParams={Object.fromEntries(
                        Object.entries(step.params ?? {}).filter(
                          ([, value]) => !isPromptSecret(value),
                        ),
                      )}
                      onChange={(params, validation) => setParams(entry.id, params, validation)}
                    />
                  ) : (
                    <div className="grid gap-1">
                      <p className="text-xs text-muted-foreground">{copy.unknownTool}</p>
                      <pre className="m-0 overflow-x-auto rounded bg-muted p-2 text-xs">
                        {JSON.stringify(step.params ?? {}, null, 2)}
                      </pre>
                    </div>
                  )}

                  <label htmlFor={fieldId} className="grid gap-1 text-xs">
                    {copy.condition}
                    <input
                      id={fieldId}
                      className={FIELD}
                      value={step.if ?? ""}
                      placeholder="page_count > 1"
                      aria-invalid={conditionProblem ? true : undefined}
                      aria-describedby={`${fieldId}-hint`}
                      onChange={(event) =>
                        updateStep(entry.id, (current) => {
                          const { if: _drop, ...rest } = current;
                          return (
                            event.target.value ? { ...rest, if: event.target.value } : rest
                          ) as RecipeStep;
                        })
                      }
                    />
                  </label>
                  <p
                    id={`${fieldId}-hint`}
                    className={cn(
                      "text-xs",
                      conditionProblem ? "text-destructive" : "text-muted-foreground",
                    )}
                  >
                    {conditionProblem ?? copy.conditionHint}
                  </p>
                </div>
              </li>
            );
          })}
        </ol>
      )}

      <div className="flex flex-wrap items-end gap-2">
        <label className="grid min-w-48 flex-1 gap-1 text-xs">
          {copy.addStepPicker}
          <select className={FIELD} value={pick} onChange={(event) => setPick(event.target.value)}>
            <option value="" />
            <option value={CONVERT}>{copy.convertOption}</option>
            {sortedTools.map((tool) => (
              <option key={tool.id} value={tool.id}>
                {toolTitle(tool)} ({tool.id})
              </option>
            ))}
          </select>
        </label>
        <Button size="sm" variant="outline" disabled={!pick} onClick={addStep}>
          <PlusIcon aria-hidden="true" />
          {copy.addStep}
        </Button>
      </div>

      <div className="grid gap-1">
        <label className="grid gap-1 text-xs">
          {copy.naming}
          <input
            className={FIELD}
            value={naming}
            placeholder="{stem}"
            aria-invalid={namingProblem ? true : undefined}
            onChange={(event) => setNaming(event.target.value)}
          />
        </label>
        <p
          data-slot="recipe-naming-preview"
          className={cn("text-xs", namingProblem ? "text-destructive" : "text-muted-foreground")}
        >
          {namingProblem ?? `${copy.namingHint} · ${copy.namingPreview(exampleName)}`}
        </p>
      </div>

      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          disabled={entries.length === 0}
          onClick={() => onExportJson(`${serializeRecipe(recipe, options)}\n`, "recipe.json")}
        >
          <DownloadIcon aria-hidden="true" />
          {copy.exportJson}
        </Button>
        <Button size="sm" variant="outline" disabled={!ready} onClick={share}>
          <Link2Icon aria-hidden="true" />
          {copy.shareLink}
        </Button>
        {onRun && (
          <Button size="sm" disabled={!ready || pending !== null} onClick={() => onRun(recipe)}>
            <PlayIcon aria-hidden="true" />
            {copy.run}
          </Button>
        )}
      </div>
      {notice === "link" && (
        <p role="status" data-slot="recipe-share-status" className="flex flex-wrap gap-2 text-xs">
          <span>{copy.linkCopied}</span>
          <Badge variant="outline">
            <ShieldCheckIcon aria-hidden="true" />
            {copy.secretsReplaced}
          </Badge>
        </p>
      )}

      {problem && (
        <Alert variant="destructive" data-slot="recipe-problem">
          <AlertDescription>{problem}</AlertDescription>
        </Alert>
      )}

      <div className="grid gap-2">
        <h3 className="text-xs font-semibold">{copy.importTitle}</h3>
        <textarea
          className={cn(FIELD, "h-20 py-1.5 font-mono text-xs")}
          aria-label={copy.importPaste}
          value={pasted}
          onChange={(event) => setPasted(event.target.value)}
        />
        <div className="flex flex-wrap gap-2">
          <Button
            size="sm"
            variant="outline"
            disabled={!pasted.trim()}
            onClick={() => importText(pasted)}
          >
            <UploadIcon aria-hidden="true" />
            {copy.importButton}
          </Button>
          <label className="inline-flex h-7 cursor-pointer items-center gap-1 rounded-lg border px-2.5 text-[0.8rem] font-medium hover:bg-muted">
            {copy.importFile}
            <input
              type="file"
              accept="application/json,.json"
              className="sr-only"
              onChange={async (event) => {
                const file = event.target.files?.[0];
                event.target.value = "";
                if (file) importText(await file.text());
              }}
            />
          </label>
        </div>
      </div>

      <div className="grid gap-1.5">
        <h3 className="text-xs font-semibold">{copy.cliTitle}</h3>
        <div className="flex items-center gap-2">
          <code
            data-slot="recipe-command"
            className="min-w-0 flex-1 truncate rounded bg-muted px-2 py-1 text-xs"
          >
            {command}
          </code>
          <Button
            size="icon-sm"
            variant="ghost"
            aria-label={copy.copyCommand}
            onClick={async () => {
              await navigator.clipboard.writeText(command);
              setNotice("command");
            }}
          >
            <CopyIcon aria-hidden="true" />
          </Button>
          {notice === "command" && (
            <span role="status" className="text-xs text-muted-foreground">
              {copy.copied}
            </span>
          )}
        </div>
      </div>

      <RecipeConfirmDialog
        recipe={pending?.recipe ?? null}
        origin="untrusted"
        tools={tools}
        copy={confirmCopy}
        onCancel={() => {
          setPending(null);
          onPendingResolved?.();
        }}
        onConfirm={() => {
          if (pending) load(pending.recipe);
          setPending(null);
          onPendingResolved?.();
        }}
      />
    </section>
  );
}
