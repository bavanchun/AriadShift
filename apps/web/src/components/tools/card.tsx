"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { Kbd } from "@ariadshift/ui/components/kbd";
import { Tooltip, TooltipContent, TooltipTrigger } from "@ariadshift/ui/components/tooltip";
import { cn } from "@ariadshift/ui/lib/utils";
import {
  CloudIcon,
  FileSearchIcon,
  GlobeIcon,
  LaptopIcon,
  LayersIcon,
  type LucideIcon,
  Minimize2Icon,
  PencilLineIcon,
  RefreshCwIcon,
  ScanTextIcon,
  ShieldCheckIcon,
  SparklesIcon,
  StarIcon,
  TextCursorInputIcon,
} from "lucide-react";
import { memo, useMemo } from "react";
import type { ToolDescriptor } from "../../converter/tool-catalog";
import {
  describeTools,
  identityTranslate,
  RUNTIME_LABELS,
  TIER_LABELS,
  type ToolCategory,
  type ToolRuntimeBadge,
  type ToolsTranslate,
} from "../../lib/tools";

export const CATEGORY_ICONS: Record<ToolCategory, LucideIcon> = {
  organize: LayersIcon,
  convert: RefreshCwIcon,
  compress: Minimize2Icon,
  edit: PencilLineIcon,
  forms_annotations: TextCursorInputIcon,
  security: ShieldCheckIcon,
  ocr_scan: ScanTextIcon,
  metadata_inspect: FileSearchIcon,
  automation_ai: SparklesIcon,
};

/** Category tint for the icon tile: soft token background, strong token foreground. */
export const CATEGORY_TINTS: Record<ToolCategory, string> = {
  organize: "bg-info-soft text-info",
  convert: "bg-success-soft text-success",
  compress: "bg-warning-soft text-warning",
  edit: "bg-thread-soft text-gold",
  forms_annotations: "bg-sage/15 text-sage",
  security: "bg-destructive-soft text-destructive",
  ocr_scan: "bg-gold-soft/40 text-gold",
  metadata_inspect: "bg-muted text-muted-foreground",
  automation_ai: "bg-thread-soft text-thread",
};

export const RUNTIME_ICONS: Record<ToolRuntimeBadge, LucideIcon> = {
  browser: GlobeIcon,
  desktop: LaptopIcon,
  cloud: CloudIcon,
};

/** Browser is in-device (emerald), Desktop is amber, Cloud is sky: distinct beyond colour by icon. */
export const RUNTIME_TINTS: Record<ToolRuntimeBadge, string> = {
  browser: "bg-success-soft text-success",
  desktop: "bg-warning-soft text-warning",
  cloud: "bg-info-soft text-info",
};

export interface ToolCardProps {
  tool: ToolDescriptor;
  selected?: boolean;
  favorite?: boolean;
  /** Runs when the card is activated. */
  onSelect: (toolId: string) => void;
  /** Shows the favourite star when set; clicking it never selects the card. */
  onToggleFavorite?: (toolId: string) => void;
  translate?: ToolsTranslate;
  className?: string;
}

/**
 * One tool in the hub. The whole card is the primary action (a stretched button); the star sits
 * above it so toggling a favourite never opens the tool.
 */
export const ToolCard = memo(function ToolCard({
  tool,
  selected = false,
  favorite = false,
  onSelect,
  onToggleFavorite,
  translate = identityTranslate,
  className,
}: ToolCardProps) {
  const entry = useMemo(() => describeTools([tool])[0], [tool]);
  if (!entry) {
    return null;
  }
  const t = translate;
  const CategoryIcon = CATEGORY_ICONS[entry.category];
  const RuntimeIcon = RUNTIME_ICONS[entry.runtime];
  const runtimeLabel = t(`tools.runtime.${entry.runtime}`, RUNTIME_LABELS[entry.runtime]);
  const runtimeList = entry.runtimes
    .map((runtime) => t(`tools.runtime.${runtime}`, RUNTIME_LABELS[runtime]))
    .join(" · ");
  const tierLabel = t(`tools.tier.${entry.tier}`, TIER_LABELS[entry.tier]);
  const title = t(`tools.title.${entry.id}`, entry.title);
  const summary = t(`tools.summary.${entry.id}`, entry.summary);
  const descriptionId = `tool-${entry.id.replace(/\W/g, "-")}-summary`;
  const favoriteLabel = favorite
    ? t("tools.favorite.remove", "Remove from favourites")
    : t("tools.favorite.add", "Add to favourites");

  return (
    <article
      data-slot="tool-card"
      data-tool-id={entry.id}
      data-category={entry.category}
      data-runtime={entry.runtime}
      data-tier={entry.tier}
      data-selected={selected || undefined}
      className={cn(
        "group/tool relative flex flex-col gap-3 rounded-xl border border-[var(--color-border-subtle,var(--color-border))] bg-[var(--color-bg-surface)] p-4 text-start shadow-elevation-1 transition-[border-color,box-shadow] motion-reduce:transition-none",
        "hover:border-thread hover:shadow-elevation-2 focus-within:border-thread focus-within:shadow-elevation-2",
        "data-selected:border-thread data-selected:ring-2 data-selected:ring-thread/40",
        className,
      )}
    >
      <div className="flex items-start gap-3">
        <span
          aria-hidden="true"
          className={cn(
            "flex size-10 shrink-0 items-center justify-center rounded-lg",
            CATEGORY_TINTS[entry.category],
          )}
        >
          <CategoryIcon className="size-5" />
        </span>
        <div className="min-w-0 flex-1 pe-8">
          <h3 className="text-sm font-semibold leading-5 text-[var(--color-fg-strong,var(--color-foreground))]">
            <button
              type="button"
              onClick={() => onSelect(entry.id)}
              aria-describedby={descriptionId}
              aria-current={selected ? "true" : undefined}
              className="rounded-sm text-start outline-none after:absolute after:inset-0 after:rounded-xl focus-visible:after:ring-3 focus-visible:after:ring-ring/50"
            >
              {title}
            </button>
          </h3>
          <code className="mt-1 inline-block max-w-full truncate rounded-sm bg-muted px-1.5 py-0.5 font-mono text-[0.7rem] text-muted-foreground">
            {entry.id}
          </code>
        </div>
      </div>

      <p id={descriptionId} className="line-clamp-3 text-sm text-muted-foreground">
        {summary}
      </p>

      <div className="mt-auto flex flex-wrap items-center gap-1.5">
        <Tooltip>
          <TooltipTrigger render={<span className="relative z-10" />}>
            <Badge
              data-testid="runtime-badge"
              data-runtime={entry.runtime}
              variant="secondary"
              className={RUNTIME_TINTS[entry.runtime]}
            >
              <RuntimeIcon aria-hidden="true" />
              {runtimeLabel}
              <span className="sr-only">
                {t("tools.runtime.list", "Runs in")}: {runtimeList}
              </span>
            </Badge>
          </TooltipTrigger>
          <TooltipContent>
            {t("tools.runtime.list", "Runs in")}: {runtimeList}
          </TooltipContent>
        </Tooltip>
        <Badge
          data-testid="tier-badge"
          data-tier={entry.tier}
          variant={entry.tier === "pro" ? "default" : "outline"}
        >
          {tierLabel}
        </Badge>
        <span
          aria-hidden="true"
          className="ms-auto flex items-center gap-1 text-xs text-muted-foreground opacity-0 transition-opacity group-focus-within/tool:opacity-100 group-hover/tool:opacity-100 motion-reduce:transition-none"
        >
          {t("tools.card.open", "Open")} <Kbd>↵</Kbd>
        </span>
      </div>

      {onToggleFavorite && (
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                aria-pressed={favorite}
                aria-label={`${favoriteLabel}: ${title}`}
                data-testid="favorite-toggle"
                className="absolute end-2 top-2 z-10"
                onClick={(event) => {
                  event.stopPropagation();
                  onToggleFavorite(entry.id);
                }}
              />
            }
          >
            <StarIcon aria-hidden="true" className={cn(favorite && "fill-thread text-thread")} />
          </TooltipTrigger>
          <TooltipContent>{favoriteLabel}</TooltipContent>
        </Tooltip>
      )}
    </article>
  );
});
