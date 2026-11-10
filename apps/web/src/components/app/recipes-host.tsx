"use client";
// Loaded on demand by the app shell: the visual recipe builder and the confirmation gate for a
// recipe that arrived through a link (AD-18). The tool registry is static, so it needs no worker,
// and it stays here so the app route's first load does not carry it.
import { Alert, AlertDescription } from "@ariadshift/ui/components/alert";
import { useMemo } from "react";
import { toolCatalog } from "@/converter/tool-catalog";
import {
  type AriadRecipe,
  decodeRecipeFragment,
  RECIPE_VERSION,
  RecipeFormatError,
  redactParams,
} from "@/lib/recipe";
import { RecipeBuilder } from "../recipes";

export interface RecipeDraftStep {
  toolId: string;
  params: Record<string, unknown>;
}

export interface RecipeDraft {
  steps: RecipeDraftStep[];
  /** A new nonce replaces the builder's steps; the builder otherwise keeps its own state. */
  nonce: number;
}

export interface RecipesHostProps {
  /** Steps handed over from the workspace or the command palette. */
  draft: RecipeDraft | null;
  /** The page address fragment (`#recipe=…`); it waits behind the confirmation gate. */
  fragment: string | null;
  onResolved: () => void;
}

export default function RecipesHost({ draft, fragment, onResolved }: RecipesHostProps) {
  const tools = useMemo(() => toolCatalog(), []);
  const initial = useMemo<AriadRecipe | undefined>(
    () =>
      draft
        ? {
            version: RECIPE_VERSION,
            // A typed secret never becomes part of a recipe.
            steps: draft.steps.map((step) => ({
              tool: step.toolId,
              params: redactParams(
                step.params,
                tools.find((tool) => tool.id === step.toolId),
              ),
            })),
          }
        : undefined,
    [draft, tools],
  );
  const link = useMemo(() => {
    if (!fragment) return { incoming: null, problem: null };
    try {
      return { incoming: decodeRecipeFragment(fragment, { tools }), problem: null };
    } catch (error) {
      return {
        incoming: null,
        problem: error instanceof RecipeFormatError ? error.message : String(error),
      };
    }
  }, [fragment, tools]);
  return (
    <div className="flex flex-col gap-4">
      {link.problem && (
        <Alert variant="destructive" data-slot="recipe-link-problem">
          <AlertDescription>{link.problem}</AlertDescription>
        </Alert>
      )}
      <RecipeBuilder
        key={draft?.nonce ?? 0}
        tools={tools}
        initialRecipe={initial}
        incoming={link.incoming}
        onPendingResolved={onResolved}
      />
    </div>
  );
}
