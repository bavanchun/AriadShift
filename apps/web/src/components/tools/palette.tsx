"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
} from "@ariadshift/ui/components/command";
import { KbdGroup } from "@ariadshift/ui/components/kbd";
import { toggleThemePreference } from "@ariadshift/ui/hooks/use-theme";
import { cn } from "@ariadshift/ui/lib/utils";
import {
  ArrowRightIcon,
  HistoryIcon,
  type LucideIcon,
  RefreshCwIcon,
  StarIcon,
  SunMoonIcon,
  Trash2Icon,
  WandSparklesIcon,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { type ToolDescriptor, toolCatalog } from "../../converter/tool-catalog";
import {
  CATEGORY_LABELS,
  describeTools,
  groupByCategory,
  type HubTool,
  identityTranslate,
  parseNaturalLanguageRecipe,
  type RecipeStepDraft,
  RUNTIME_LABELS,
  searchTools,
  TIER_LABELS,
  type ToolCategory,
  type ToolsTranslate,
  useRecentTools,
  useToolFavorites,
} from "../../lib/tools";
import { CATEGORY_ICONS, CATEGORY_TINTS, RUNTIME_ICONS, RUNTIME_TINTS } from "./card";

/** A search shows at most this many tools; refine the query to narrow further. */
const MAX_RESULTS = 40;

export interface ToolCommandPaletteProps {
  /** Tools to search. Defaults to the full registry. */
  catalog?: ToolDescriptor[];
  open?: boolean;
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
  /** Turns off the global ⌘K / Ctrl+K listener, for a host that binds the key itself. */
  disableShortcut?: boolean;
  /** Text placed in the search box each time the palette opens. */
  initialSearch?: string;
  onSelectTool: (toolId: string) => void;
  /** Loads a recognised plain-sentence recipe. Without it the recipe preview is hidden. */
  onRunRecipe?: (steps: RecipeStepDraft[]) => void;
  /** Adds the "Jump to Convert" quick action. */
  onJumpToCategory?: (category: ToolCategory) => void;
  /** Replaces the built-in light/dark switch, so the host can use its shared theme state. */
  onSwitchTheme?: () => void;
  translate?: ToolsTranslate;
}

interface QuickAction {
  id: string;
  label: string;
  keywords: string[];
  icon: LucideIcon;
  run: () => void;
}

/**
 * ⌘K palette: tools, favourites, recents and quick actions, plus a recipe preview when the text
 * reads as a plain sentence ("rotate 90 degrees then compress"). Search runs on this device.
 */
export function ToolCommandPalette({
  catalog,
  open: openProp,
  defaultOpen = false,
  onOpenChange,
  disableShortcut = false,
  initialSearch = "",
  onSelectTool,
  onRunRecipe,
  onJumpToCategory,
  onSwitchTheme,
  translate = identityTranslate,
}: ToolCommandPaletteProps) {
  const t = translate;
  const [innerOpen, setInnerOpen] = useState(defaultOpen);
  const open = openProp ?? innerOpen;
  const openRef = useRef(open);
  openRef.current = open;
  const [search, setSearch] = useState(defaultOpen ? initialSearch : "");
  const seedRef = useRef(initialSearch);
  seedRef.current = initialSearch;

  const setOpen = useCallback(
    (next: boolean) => {
      setInnerOpen(next);
      onOpenChange?.(next);
    },
    [onOpenChange],
  );

  // Each opening starts from the seed text; closing clears it.
  useEffect(() => {
    setSearch(open ? seedRef.current : "");
  }, [open]);

  useEffect(() => {
    if (disableShortcut) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (
        (event.metaKey || event.ctrlKey) &&
        !event.altKey &&
        !event.shiftKey &&
        event.key.toLowerCase() === "k"
      ) {
        event.preventDefault();
        setOpen(!openRef.current);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [disableShortcut, setOpen]);

  const tools = useMemo(() => catalog ?? toolCatalog(), [catalog]);
  const entries = useMemo(() => describeTools(tools, t), [tools, t]);
  const byId = useMemo(() => new Map(entries.map((entry) => [entry.id, entry])), [entries]);
  const { favorites } = useToolFavorites();
  const { recents, clear: clearRecentTools } = useRecentTools();

  const query = search.trim();
  const recipe = useMemo(
    () => (onRunRecipe ? parseNaturalLanguageRecipe(query, tools) : null),
    [onRunRecipe, query, tools],
  );
  const results = useMemo(
    () => (query ? searchTools(entries, query).slice(0, MAX_RESULTS) : []),
    [entries, query],
  );

  const choose = (toolId: string) => {
    setOpen(false);
    onSelectTool(toolId);
  };

  const actions: QuickAction[] = [
    ...(onJumpToCategory
      ? [
          {
            id: "jump-convert",
            label: t("tools.palette.jumpConvert", "Jump to Convert"),
            keywords: ["category", "convert", "format"],
            icon: RefreshCwIcon,
            run: () => onJumpToCategory("convert"),
          },
        ]
      : []),
    {
      id: "switch-theme",
      label: t("tools.palette.switchTheme", "Switch theme"),
      keywords: ["dark", "light", "appearance", "mode"],
      icon: SunMoonIcon,
      run: onSwitchTheme ?? toggleThemePreference,
    },
    ...(recents.length > 0
      ? [
          {
            id: "clear-recents",
            label: t("tools.palette.clearRecents", "Clear recent tools"),
            keywords: ["history", "forget", "reset"],
            icon: Trash2Icon,
            run: clearRecentTools,
          },
        ]
      : []),
  ];
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const visibleActions = actions.filter((action) => {
    const haystack = `${action.label} ${action.keywords.join(" ")}`.toLowerCase();
    return words.every((word) => haystack.includes(word));
  });

  const row = (entry: HubTool, scope: string) => (
    <ToolRow key={`${scope}:${entry.id}`} entry={entry} onChoose={choose} t={t} />
  );
  const favoriteEntries = favorites.flatMap((id) => byId.get(id) ?? []);
  const recentEntries = recents.flatMap((id) => byId.get(id) ?? []);

  return (
    <CommandDialog
      open={open}
      onOpenChange={setOpen}
      title={t("tools.palette.title", "Tool command palette")}
      description={t(
        "tools.palette.description",
        "Search tools, or describe what to do in a sentence, then press Enter.",
      )}
      commandProps={{ search, onSearchChange: setSearch, filter: () => 1, loop: true }}
    >
      <CommandInput
        placeholder={t("tools.palette.placeholder", "Search tools or describe a recipe…")}
        aria-label={t("tools.palette.search", "Search tools")}
      />
      <CommandList className="max-h-[min(28rem,60dvh)]">
        <CommandEmpty>
          {t(
            "tools.palette.empty",
            "No tool matches. Try a sentence like “rotate 90 then compress”.",
          )}
        </CommandEmpty>

        {recipe && onRunRecipe && (
          <>
            <CommandGroup heading={t("tools.palette.recipe", "Recipe")}>
              <CommandItem
                value={`recipe ${query}`}
                data-testid="recipe-preview"
                className="items-start py-2"
                onSelect={() => {
                  setOpen(false);
                  onRunRecipe(recipe.steps);
                }}
              >
                <WandSparklesIcon aria-hidden="true" className="mt-0.5 text-thread" />
                <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                  <ol
                    aria-label={t("tools.palette.recipeSteps", "Recipe steps")}
                    className="flex flex-wrap items-center gap-1.5"
                  >
                    {recipe.steps.map((step, index) => (
                      <li key={`${index}:${step.toolId}`} className="flex items-center gap-1.5">
                        {index > 0 && (
                          <ArrowRightIcon aria-hidden="true" className="text-muted-foreground" />
                        )}
                        <span
                          data-testid="recipe-step"
                          data-tool-id={step.toolId}
                          className="inline-flex items-center gap-1 rounded-full bg-thread-soft px-2 py-0.5 text-xs font-medium text-thread"
                        >
                          <span className="font-semibold">
                            {t("tools.palette.step", "Step")} {index + 1}
                          </span>
                          <span aria-hidden="true">·</span>
                          {step.label}
                        </span>
                      </li>
                    ))}
                  </ol>
                  <span className="text-xs text-muted-foreground">
                    {recipe.requiresConfirmation
                      ? t(
                          "tools.palette.recipeReview",
                          "Review before running: a step needs the desktop, cloud, AI or changes the file for good.",
                        )
                      : t("tools.palette.recipeReady", "Press Enter to load this recipe.")}
                  </span>
                </div>
                {recipe.requiresConfirmation && (
                  <Badge variant="outline" className="shrink-0 border-thread text-thread">
                    {t("tools.palette.confirm", "Review first")}
                  </Badge>
                )}
              </CommandItem>
            </CommandGroup>
            <CommandSeparator />
          </>
        )}

        {query ? (
          <>
            {results.length > 0 && (
              <CommandGroup heading={t("tools.palette.results", "Tools")}>
                {results.map((entry) => row(entry, "result"))}
              </CommandGroup>
            )}
            {visibleActions.length > 0 && (
              <CommandGroup heading={t("tools.palette.actions", "Actions")}>
                {visibleActions.map((action) => (
                  <ActionRow key={action.id} action={action} onDone={() => setOpen(false)} />
                ))}
              </CommandGroup>
            )}
          </>
        ) : (
          <>
            <CommandGroup heading={t("tools.palette.actions", "Actions")}>
              {actions.map((action) => (
                <ActionRow key={action.id} action={action} onDone={() => setOpen(false)} />
              ))}
            </CommandGroup>
            {favoriteEntries.length > 0 && (
              <CommandGroup
                heading={
                  <span className="flex items-center gap-1.5">
                    <StarIcon aria-hidden="true" className="size-3" />
                    {t("tools.favorites", "Favourites")}
                  </span>
                }
              >
                {favoriteEntries.map((entry) => row(entry, "favorite"))}
              </CommandGroup>
            )}
            {recentEntries.length > 0 && (
              <CommandGroup
                heading={
                  <span className="flex items-center gap-1.5">
                    <HistoryIcon aria-hidden="true" className="size-3" />
                    {t("tools.recents", "Recent tools")}
                  </span>
                }
              >
                {recentEntries.map((entry) => row(entry, "recent"))}
              </CommandGroup>
            )}
            {groupByCategory(entries).map((group) => (
              <CommandGroup
                key={group.category}
                heading={t(`tools.category.${group.category}`, CATEGORY_LABELS[group.category])}
              >
                {group.tools.map((entry) => row(entry, "all"))}
              </CommandGroup>
            ))}
          </>
        )}
      </CommandList>
      <div className="flex items-center justify-between gap-3 border-t px-3 py-2 text-xs text-muted-foreground">
        <span>{t("tools.palette.hint", "Search runs on this device.")}</span>
        <span className="flex items-center gap-3">
          <span className="flex items-center gap-1">
            <KbdGroup keys={["enter"]} /> {t("tools.palette.open", "Open")}
          </span>
          <span className="flex items-center gap-1">
            <KbdGroup keys={["esc"]} /> {t("tools.palette.close", "Close")}
          </span>
        </span>
      </div>
    </CommandDialog>
  );
}

function ToolRow({
  entry,
  onChoose,
  t,
}: {
  entry: HubTool;
  onChoose: (toolId: string) => void;
  t: ToolsTranslate;
}) {
  const CategoryIcon = CATEGORY_ICONS[entry.category];
  const RuntimeIcon = RUNTIME_ICONS[entry.runtime];
  return (
    <CommandItem
      value={`${entry.title} ${entry.id}`}
      keywords={entry.keywords}
      data-tool-id={entry.id}
      onSelect={() => onChoose(entry.id)}
    >
      <span
        aria-hidden="true"
        className={cn(
          "flex size-6 shrink-0 items-center justify-center rounded-md",
          CATEGORY_TINTS[entry.category],
        )}
      >
        <CategoryIcon className="size-3.5" />
      </span>
      <span className="min-w-0 flex-1 truncate">
        {t(`tools.title.${entry.id}`, entry.title)}
        <span className="ms-2 font-mono text-xs text-muted-foreground">{entry.id}</span>
      </span>
      <Badge
        variant="secondary"
        data-runtime={entry.runtime}
        className={RUNTIME_TINTS[entry.runtime]}
      >
        <RuntimeIcon aria-hidden="true" />
        {t(`tools.runtime.${entry.runtime}`, RUNTIME_LABELS[entry.runtime])}
      </Badge>
      <Badge variant={entry.tier === "pro" ? "default" : "outline"} data-tier={entry.tier}>
        {t(`tools.tier.${entry.tier}`, TIER_LABELS[entry.tier])}
      </Badge>
    </CommandItem>
  );
}

function ActionRow({ action, onDone }: { action: QuickAction; onDone: () => void }) {
  const Icon = action.icon;
  return (
    <CommandItem
      value={action.label}
      keywords={action.keywords}
      data-action-id={action.id}
      onSelect={() => {
        onDone();
        action.run();
      }}
    >
      <Icon aria-hidden="true" />
      {action.label}
    </CommandItem>
  );
}
