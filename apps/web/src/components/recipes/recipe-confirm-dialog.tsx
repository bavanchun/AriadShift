"use client";

import { Button } from "@ariadshift/ui/components/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@ariadshift/ui/components/dialog";
import { ShieldAlertIcon } from "lucide-react";
import {
  type AriadRecipe,
  hasPromptSecrets,
  isConvertStep,
  type SecretAwareTool,
} from "@/lib/recipe";
import { toolTitle } from "@/lib/tools";

export interface RecipeConfirmCopy {
  title: string;
  description: string;
  step: (index: number) => string;
  condition: (expression: string) => string;
  noParams: string;
  naming: (template: string) => string;
  convertTo: (format: string, profile: string | undefined) => string;
  secretPrompt: (field: string) => string;
  confirm: string;
  cancel: string;
  close: string;
}

export const DEFAULT_RECIPE_CONFIRM_COPY: RecipeConfirmCopy = {
  title: "Load this recipe?",
  description:
    "This recipe came from a link or a file. Check every step before you load it; nothing runs until you choose to run it.",
  step: (index) => `Step ${index}`,
  condition: (expression) => `Runs only if ${expression}`,
  noParams: "No parameters",
  naming: (template) => `Output names: ${template}`,
  convertTo: (format, profile) =>
    profile ? `Convert to ${format} (${profile})` : `Convert to ${format}`,
  secretPrompt: (field) => `${field}: you will be asked in memory before it runs`,
  confirm: "Confirm & Load",
  cancel: "Cancel",
  close: "Close",
};

export interface RecipeConfirmDialogProps {
  /** The recipe awaiting a decision; `null` keeps the dialog closed. */
  recipe: AriadRecipe | null;
  /** Always `untrusted` for a link or an imported file (AD-18). */
  origin: "untrusted";
  /** Registry tools, so a tool's declared `secret_params` are listed as prompts. */
  tools?: readonly SecretAwareTool[];
  copy?: Partial<RecipeConfirmCopy>;
  onConfirm: () => void;
  onCancel: () => void;
}

/** The AD-18 gate: lists every step of an imported or shared recipe before it is loaded. */
export function RecipeConfirmDialog({
  recipe,
  origin,
  tools,
  copy: copyOverrides,
  onConfirm,
  onCancel,
}: RecipeConfirmDialogProps) {
  const copy = { ...DEFAULT_RECIPE_CONFIRM_COPY, ...copyOverrides };
  const secrets = recipe ? hasPromptSecrets(recipe, { tools }) : [];
  return (
    <Dialog open={recipe !== null} onOpenChange={(open) => !open && onCancel()}>
      <DialogContent closeLabel={copy.close} className="w-[min(40rem,calc(100%-2rem))]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ShieldAlertIcon aria-hidden="true" className="size-4 text-thread" />
            {copy.title}
          </DialogTitle>
          <DialogDescription>{copy.description}</DialogDescription>
        </DialogHeader>
        {recipe && (
          <div data-slot="recipe-confirm" data-origin={origin} className="grid gap-3">
            <ol className="m-0 grid list-none gap-2 p-0">
              {recipe.steps.map((step, index) => {
                const stepSecrets = secrets
                  .filter((entry) => entry.startsWith(`${index}:`))
                  .map((entry) => entry.slice(entry.indexOf(":") + 1));
                return (
                  <li key={index} className="grid gap-1 rounded-lg border px-3 py-2 text-sm">
                    <div className="flex flex-wrap items-baseline gap-x-2">
                      <span className="text-xs font-medium text-thread">
                        {copy.step(index + 1)}
                      </span>
                      <span className="font-medium">
                        {isConvertStep(step)
                          ? copy.convertTo(step.convert.to, step.convert.profile)
                          : toolTitle({ id: step.tool })}
                      </span>
                      {!isConvertStep(step) && (
                        <code className="text-xs text-muted-foreground">{step.tool}</code>
                      )}
                    </div>
                    {step.if && <p className="text-xs">{copy.condition(step.if)}</p>}
                    {!isConvertStep(step) &&
                      (step.params && Object.keys(step.params).length > 0 ? (
                        <pre className="m-0 overflow-x-auto rounded bg-muted p-2 text-xs">
                          {JSON.stringify(step.params, null, 2)}
                        </pre>
                      ) : (
                        <p className="text-xs text-muted-foreground">{copy.noParams}</p>
                      ))}
                    {stepSecrets.map((field) => (
                      <p key={field} className="text-xs font-medium text-thread">
                        {copy.secretPrompt(field)}
                      </p>
                    ))}
                  </li>
                );
              })}
            </ol>
            {recipe.naming && <p className="text-xs">{copy.naming(recipe.naming)}</p>}
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>
            {copy.cancel}
          </Button>
          <Button onClick={onConfirm}>{copy.confirm}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
