"use client";
// The /app page: converting one file, browsing every tool, and the PDF workspace. The converter
// is the landing view and stays mounted; the hub and the workspace load on first use so the
// route's first load does not carry the tool registry, the viewer or the PDFium module.
import { Button } from "@ariadshift/ui/components/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@ariadshift/ui/components/tabs";
import { useSearchParams } from "next/navigation";
import { useTranslations } from "next-intl";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { FRAGMENT_KEY } from "@/lib/recipe";
import { ConverterApp } from "../converter/converter-app";
import type { RecipeDraft, RecipeDraftStep } from "./recipes-host";
import { useLazyModule } from "./use-lazy-module";

type View = "convert" | "tools" | "workspace" | "recipes";

const loadWorkspace = () => import("./workspace-host");
const loadTools = () => import("./tools-host");
const loadRecipes = () => import("./recipes-host");

const isView = (value: string | null): value is View =>
  value === "convert" || value === "tools" || value === "workspace" || value === "recipes";

function isPaletteShortcut(event: KeyboardEvent) {
  return (
    (event.metaKey || event.ctrlKey) &&
    !event.altKey &&
    !event.shiftKey &&
    event.key.toLowerCase() === "k"
  );
}

/** Keeps `?view=` and `?tool=` in the address bar, so a tool can be linked and reloaded. */
function writeLocation(view: View, toolId: string | null) {
  const url = new URL(window.location.href);
  if (view === "convert") url.searchParams.delete("view");
  else url.searchParams.set("view", view);
  if (view === "workspace" && toolId) url.searchParams.set("tool", toolId);
  else url.searchParams.delete("tool");
  window.history.replaceState(window.history.state, "", url);
}

export function AppShell() {
  const t = useTranslations("web.app");
  const params = useSearchParams();
  const linkedTool = params.get("tool");
  const linkedView = params.get("view");
  const [view, setView] = useState<View>(
    linkedTool ? "workspace" : isView(linkedView) ? linkedView : "convert",
  );
  const [requested, setRequested] = useState<{ id: string; nonce: number } | null>(
    linkedTool ? { id: linkedTool, nonce: 0 } : null,
  );
  const nonce = useRef(0);
  // Each of these turns true on first use and stays true, so state in the view survives a tab change.
  const [toolsWanted, setToolsWanted] = useState(view === "tools");
  const [paletteOnLoad, setPaletteOnLoad] = useState(false);
  const [workspaceWanted, setWorkspaceWanted] = useState(view === "workspace");
  const [recipesWanted, setRecipesWanted] = useState(view === "recipes");
  const tools = useLazyModule(loadTools, toolsWanted);
  const workspace = useLazyModule(loadWorkspace, workspaceWanted);
  const recipes = useLazyModule(loadRecipes, recipesWanted);
  // Steps handed to the builder, and a `#recipe=` link waiting for its confirmation (AD-18).
  const [recipeDraft, setRecipeDraft] = useState<RecipeDraft | null>(null);
  const [recipeFragment, setRecipeFragment] = useState<string | null>(null);
  const draftNonce = useRef(0);
  // A recipe drafted by the AI panel: untrusted, so it waits behind the same gate as a link.
  const [recipeImport, setRecipeImport] = useState<string | null>(null);

  const show = useCallback((next: View, toolId: string | null = null) => {
    setView(next);
    if (next === "tools") setToolsWanted(true);
    if (next === "workspace") setWorkspaceWanted(true);
    if (next === "recipes") setRecipesWanted(true);
    writeLocation(next, toolId);
  }, []);

  const openRecipe = useCallback(
    (steps: RecipeDraftStep[]) => {
      draftNonce.current += 1;
      setRecipeDraft({ steps, nonce: draftNonce.current });
      show("recipes");
    },
    [show],
  );

  const openImportedRecipe = useCallback(
    (recipe: { steps: Array<{ tool: string; params: Record<string, unknown> }> }) => {
      // `name` is a display field the recipe format does not carry, so only the steps travel.
      setRecipeImport(JSON.stringify({ version: "ariad-recipe/1", steps: recipe.steps }));
      show("recipes");
    },
    [show],
  );

  // A shared recipe link opens the builder; the recipe loads only after the user confirms it.
  useEffect(() => {
    const read = () => {
      if (!window.location.hash.startsWith(`#${FRAGMENT_KEY}=`)) return;
      setRecipeFragment(window.location.hash);
      show("recipes");
    };
    read();
    window.addEventListener("hashchange", read);
    return () => window.removeEventListener("hashchange", read);
  }, [show]);

  const recipeResolved = useCallback(() => {
    setRecipeFragment(null);
    setRecipeImport(null);
    const url = new URL(window.location.href);
    if (!url.hash.startsWith(`#${FRAGMENT_KEY}=`)) return;
    url.hash = "";
    window.history.replaceState(window.history.state, "", url);
  }, []);

  const openTool = useCallback(
    (toolId: string) => {
      nonce.current += 1;
      setRequested({ id: toolId, nonce: nonce.current });
      show("workspace", toolId);
    },
    [show],
  );

  // ⌘K / Ctrl+K works from the converter too. Until the hub exists nothing else listens for it, so
  // this loads the hub with its palette open; afterwards the hub's own listener takes over.
  useEffect(() => {
    if (toolsWanted) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isPaletteShortcut(event)) return;
      event.preventDefault();
      setPaletteOnLoad(true);
      setToolsWanted(true);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [toolsWanted]);

  return (
    <Tabs value={view} onValueChange={(next) => isView(next) && show(next, requested?.id ?? null)}>
      <TabsList aria-label={t("sections_label")}>
        <TabsTrigger value="convert" data-testid="tab-convert">
          {t("tab.convert")}
        </TabsTrigger>
        <TabsTrigger value="tools" data-testid="tab-tools">
          {t("tab.tools")}
        </TabsTrigger>
        <TabsTrigger value="workspace" data-testid="tab-workspace">
          {t("tab.workspace")}
        </TabsTrigger>
        <TabsTrigger value="recipes" data-testid="tab-recipes">
          {t("tab.recipes")}
        </TabsTrigger>
      </TabsList>
      <TabsContent value="convert" keepMounted className="text-base">
        <ConverterApp />
      </TabsContent>
      <TabsContent value="tools" keepMounted className="text-base">
        {toolsWanted && (
          <div className="flex flex-col gap-4">
            <p className="text-muted-foreground">{t("tools_lede")}</p>
            <Lazy
              kind="tools"
              state={tools.state}
              retry={tools.retry}
              render={(module) => (
                <module.default
                  onSelectTool={openTool}
                  onRunRecipe={(steps) =>
                    openRecipe(steps.map(({ toolId, params }) => ({ toolId, params })))
                  }
                  defaultPaletteOpen={paletteOnLoad}
                  onOpenPalette={() => setPaletteOnLoad(false)}
                />
              )}
            />
          </div>
        )}
      </TabsContent>
      <TabsContent value="workspace" keepMounted className="text-base">
        {workspaceWanted && (
          <Lazy
            kind="workspace"
            state={workspace.state}
            retry={workspace.retry}
            render={(module) => (
              <module.default
                requestedTool={requested}
                onSaveRecipe={openRecipe}
                onOpenRecipeDraft={openImportedRecipe}
              />
            )}
          />
        )}
      </TabsContent>
      <TabsContent value="recipes" keepMounted className="text-base">
        {recipesWanted && (
          <div className="flex flex-col gap-4">
            <p className="text-muted-foreground">{t("recipes_lede")}</p>
            <Lazy
              kind="recipes"
              state={recipes.state}
              retry={recipes.retry}
              render={(module) => (
                <module.default
                  draft={recipeDraft}
                  fragment={recipeFragment}
                  imported={recipeImport}
                  onResolved={recipeResolved}
                />
              )}
            />
          </div>
        )}
      </TabsContent>
    </Tabs>
  );
}

function Lazy<T>({
  kind,
  state,
  retry,
  render,
}: {
  kind: "tools" | "workspace" | "recipes";
  state: { status: "idle" | "loading" | "failed" } | { status: "ready"; module: T };
  retry: () => void;
  render: (module: T) => ReactNode;
}) {
  const t = useTranslations("web.app");
  if (state.status === "ready") return render(state.module);
  if (state.status === "failed") {
    return (
      <div role="alert" className="flex flex-wrap items-center gap-3 text-sm">
        <span>{t(`${kind}_failed`)}</span>
        <Button type="button" variant="outline" size="sm" onClick={retry}>
          {t("retry")}
        </Button>
      </div>
    );
  }
  return (
    <p role="status" className="text-sm text-muted-foreground">
      {t(`${kind}_loading`)}
    </p>
  );
}
