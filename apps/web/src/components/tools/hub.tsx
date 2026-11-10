"use client";

import type { ToolDescriptor as WasmToolDescriptor } from "@ariadshift/ariad-wasm";
import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { KbdGroup } from "@ariadshift/ui/components/kbd";
import { ToggleGroup, ToggleGroupItem } from "@ariadshift/ui/components/toggle-group";
import { TooltipProvider } from "@ariadshift/ui/components/tooltip";
import { cn } from "@ariadshift/ui/lib/utils";
import {
  HistoryIcon,
  LockKeyholeIcon,
  SearchIcon,
  SearchXIcon,
  StarIcon,
  XIcon,
} from "lucide-react";
import { type ReactNode, useCallback, useId, useMemo, useState } from "react";
import { type ToolDescriptor, toolCatalog } from "../../converter/tool-catalog";
import {
  CATEGORY_LABELS,
  describeTools,
  groupByCategory,
  identityTranslate,
  type RecipeStepDraft,
  RUNTIME_LABELS,
  recordRecent,
  searchTools,
  TOOL_CATEGORIES,
  type ToolCategory,
  type ToolRuntimeBadge,
  type ToolsTranslate,
  useRecentTools,
  useToolFavorites,
} from "../../lib/tools";
import { CATEGORY_ICONS, CATEGORY_TINTS, ToolCard } from "./card";
import { ToolCommandPalette } from "./palette";

export type { RecipeStepDraft } from "../../lib/tools";

const RUNTIME_FILTERS = ["all", "browser", "desktop", "cloud"] as const;
type RuntimeFilter = (typeof RUNTIME_FILTERS)[number];

const GRID = "grid gap-3 sm:grid-cols-2 xl:grid-cols-3";

export interface ToolHubProps {
  /** Controlled selection. Omit it and the hub tracks the selected tool itself. */
  selectedToolId?: string | null;
  onSelectTool?: (toolId: string) => void;
  /** Receives the steps of a plain-sentence recipe chosen in the palette. */
  onRunRecipe?: (steps: RecipeStepDraft[]) => void;
  /** Tools the browser runtime reports; they refine the static registry. */
  runtimeTools?: WasmToolDescriptor[];
  /** Called when the palette opens, from the button or from ⌘K / Ctrl+K. */
  onOpenPalette?: () => void;
  /** Opens the palette on mount, for a host that caught ⌘K before the hub was loaded. */
  defaultPaletteOpen?: boolean;
  translate?: ToolsTranslate;
  className?: string;
}

/** Searchable grid of every registered tool, grouped by category. */
export function ToolHub({
  selectedToolId,
  onSelectTool,
  onRunRecipe,
  runtimeTools,
  onOpenPalette,
  defaultPaletteOpen = false,
  translate = identityTranslate,
  className,
}: ToolHubProps) {
  const t = translate;
  const searchId = useId();
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<ToolCategory | "all">("all");
  const [runtime, setRuntime] = useState<RuntimeFilter>("all");
  const [innerSelected, setInnerSelected] = useState<string | null>(null);
  const [paletteOpen, setPaletteOpen] = useState(defaultPaletteOpen);
  const [paletteSeed, setPaletteSeed] = useState("");
  const selected = selectedToolId === undefined ? innerSelected : selectedToolId;

  const catalog = useMemo(() => toolCatalog(runtimeTools), [runtimeTools]);
  const entries = useMemo(() => describeTools(catalog, t), [catalog, t]);
  const byId = useMemo(() => new Map(catalog.map((tool) => [tool.id, tool])), [catalog]);
  const { favorites, isFavorite, toggle } = useToolFavorites();
  const { recents } = useRecentTools();

  const results = useMemo(
    () => searchTools(entries, query, { category, runtime: runtime === "all" ? "all" : runtime }),
    [entries, query, category, runtime],
  );
  const groups = useMemo(() => groupByCategory(results), [results]);
  const filtering = query.trim() !== "" || category !== "all" || runtime !== "all";

  const selectTool = useCallback(
    (toolId: string) => {
      recordRecent(toolId);
      setInnerSelected(toolId);
      onSelectTool?.(toolId);
    },
    [onSelectTool],
  );

  const changePalette = useCallback(
    (open: boolean) => {
      setPaletteOpen(open);
      if (open) {
        onOpenPalette?.();
      }
    },
    [onOpenPalette],
  );

  const openPalette = (seed: string) => {
    setPaletteSeed(seed);
    changePalette(true);
  };

  const reset = () => {
    setQuery("");
    setCategory("all");
    setRuntime("all");
  };

  const renderCard = (tool: ToolDescriptor) => (
    <ToolCard
      key={tool.id}
      tool={tool}
      selected={selected === tool.id}
      favorite={isFavorite(tool.id)}
      onSelect={selectTool}
      onToggleFavorite={toggle}
      translate={t}
    />
  );
  const favoriteTools = favorites.flatMap((id) => byId.get(id) ?? []);
  const recentTools = recents.flatMap((id) => byId.get(id) ?? []);

  return (
    <TooltipProvider>
      <section
        data-testid="tool-hub"
        aria-label={t("tools.hub.label", "Tools")}
        className={cn("flex flex-col gap-6", className)}
      >
        <div className="flex flex-wrap items-center gap-3">
          <div className="relative min-w-56 flex-1">
            <SearchIcon
              aria-hidden="true"
              className="pointer-events-none absolute start-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
            />
            <input
              id={searchId}
              type="search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t("tools.hub.search", "Search tools…")}
              aria-label={t("tools.hub.search", "Search tools…")}
              autoComplete="off"
              spellCheck={false}
              className="h-10 w-full rounded-lg border border-input bg-[var(--color-bg-surface)] ps-9 pe-3 text-sm outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
            />
          </div>
          <Button
            type="button"
            variant="outline"
            size="lg"
            data-testid="palette-trigger"
            aria-haspopup="dialog"
            aria-label={t("tools.hub.openPalette", "Open command palette")}
            onClick={() => openPalette(query)}
          >
            <SearchIcon aria-hidden="true" />
            {t("tools.hub.command", "Command")}
            <KbdGroup keys={["mod", "K"]} />
          </Button>
          <Badge
            variant="outline"
            data-testid="zero-upload-badge"
            className="h-7 gap-1.5 border-success/40 bg-success-soft px-3 text-success"
          >
            <LockKeyholeIcon aria-hidden="true" />
            {t("tools.hub.zeroUpload", "On this device · 0 bytes uploaded")}
          </Badge>
        </div>

        <div className="flex flex-col gap-3">
          <ToggleGroup
            aria-label={t("tools.hub.categories", "Category")}
            value={[category]}
            onValueChange={(next) => setCategory((next[0] as ToolCategory | "all") ?? "all")}
            className="max-w-full flex-wrap"
          >
            <ToggleGroupItem value="all" size="sm" data-testid="category-all">
              {t("tools.hub.all", "All")}
            </ToggleGroupItem>
            {TOOL_CATEGORIES.map((id) => {
              const Icon = CATEGORY_ICONS[id];
              return (
                <ToggleGroupItem key={id} value={id} size="sm" data-testid={`category-${id}`}>
                  <Icon aria-hidden="true" />
                  {t(`tools.category.${id}`, CATEGORY_LABELS[id])}
                </ToggleGroupItem>
              );
            })}
          </ToggleGroup>
          <ToggleGroup
            aria-label={t("tools.hub.runtimes", "Where it runs")}
            value={[runtime]}
            onValueChange={(next) => setRuntime((next[0] as RuntimeFilter) ?? "all")}
            variant="outline"
            size="sm"
          >
            {RUNTIME_FILTERS.map((id) => (
              <ToggleGroupItem key={id} value={id} data-testid={`runtime-${id}`}>
                {id === "all"
                  ? t("tools.hub.all", "All")
                  : t(`tools.runtime.${id}`, RUNTIME_LABELS[id as ToolRuntimeBadge])}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </div>

        {!filtering && favoriteTools.length > 0 && (
          <Strip
            testId="favorites-strip"
            icon={<StarIcon aria-hidden="true" className="size-4 text-thread" />}
            title={t("tools.favorites", "Favourites")}
          >
            {favoriteTools.map(renderCard)}
          </Strip>
        )}
        {!filtering && recentTools.length > 0 && (
          <Strip
            testId="recents-strip"
            icon={<HistoryIcon aria-hidden="true" className="size-4 text-muted-foreground" />}
            title={t("tools.recents", "Recent tools")}
          >
            {recentTools.map(renderCard)}
          </Strip>
        )}

        <p role="status" className="text-sm text-muted-foreground" data-testid="result-count">
          {t("tools.hub.count", results.length === 1 ? "1 tool" : `${results.length} tools`, {
            count: results.length,
          })}
        </p>

        {results.length === 0 ? (
          <div
            data-testid="empty-state"
            className="flex flex-col items-center gap-3 rounded-xl border border-dashed border-[var(--color-border-subtle,var(--color-border))] px-6 py-12 text-center"
          >
            <SearchXIcon aria-hidden="true" className="size-8 text-muted-foreground" />
            <p className="text-sm font-medium">
              {t("tools.hub.noMatch", "No tool matches")}
              {query.trim() && <> “{query.trim()}”</>}
            </p>
            <p className="max-w-md text-sm text-muted-foreground">
              {t(
                "tools.hub.tryRecipe",
                "Try a plain sentence in the command palette, such as “rotate 90 degrees then compress”.",
              )}
            </p>
            <div className="flex flex-wrap justify-center gap-2">
              <Button type="button" variant="outline" onClick={reset}>
                <XIcon aria-hidden="true" />
                {t("tools.hub.reset", "Reset search and filters")}
              </Button>
              <Button type="button" onClick={() => openPalette(query)}>
                {t("tools.hub.openInPalette", "Try in command palette")}
              </Button>
            </div>
          </div>
        ) : (
          groups.map((group) => {
            const Icon = CATEGORY_ICONS[group.category];
            const label = t(`tools.category.${group.category}`, CATEGORY_LABELS[group.category]);
            return (
              <section
                key={group.category}
                data-testid={`group-${group.category}`}
                aria-label={label}
                className="flex flex-col gap-3"
              >
                <h2 className="flex items-center gap-2 text-sm font-semibold">
                  <span
                    aria-hidden="true"
                    className={cn(
                      "flex size-6 items-center justify-center rounded-md",
                      CATEGORY_TINTS[group.category],
                    )}
                  >
                    <Icon className="size-3.5" />
                  </span>
                  {label}
                  <span className="font-normal text-muted-foreground">{group.tools.length}</span>
                </h2>
                <div className={GRID}>{group.tools.map((entry) => renderCard(entry.tool))}</div>
              </section>
            );
          })
        )}

        <ToolCommandPalette
          catalog={catalog}
          open={paletteOpen}
          onOpenChange={changePalette}
          initialSearch={paletteSeed}
          onSelectTool={selectTool}
          onRunRecipe={onRunRecipe}
          onJumpToCategory={(next) => {
            setQuery("");
            setRuntime("all");
            setCategory(next);
          }}
          translate={t}
        />
      </section>
    </TooltipProvider>
  );
}

function Strip({
  testId,
  icon,
  title,
  children,
}: {
  testId: string;
  icon: ReactNode;
  title: string;
  children: ReactNode;
}) {
  return (
    <section data-testid={testId} aria-label={title} className="flex flex-col gap-3">
      <h2 className="flex items-center gap-2 text-sm font-semibold">
        {icon}
        {title}
      </h2>
      <div className={GRID}>{children}</div>
    </section>
  );
}
