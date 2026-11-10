"use client";

import { GripVerticalIcon, RotateCwIcon, Trash2Icon } from "lucide-react";
import * as React from "react";
import { Button } from "#components/button";
import { cn } from "#lib/utils";

export type PageRotation = 0 | 90 | 180 | 270;

export interface PageThumbnailLabels {
  page: (number: number) => string;
  select: (number: number) => string;
  rotate: (number: number) => string;
  remove: (number: number) => string;
  drag: (number: number) => string;
  blank: string;
  scanned: string;
}

const DEFAULT_LABELS: PageThumbnailLabels = {
  page: (number) => `Page ${number}`,
  select: (number) => `Select page ${number}`,
  rotate: (number) => `Rotate page ${number}`,
  remove: (number) => `Delete page ${number}`,
  drag: (number) => `Reorder page ${number}`,
  blank: "Blank",
  scanned: "Scanned",
};

export interface PageThumbnailProps
  extends Omit<React.ComponentProps<"fieldset">, "children" | "onSelect" | "onChange"> {
  /** One-based page number shown in the badge. */
  page: number;
  /** Preview image URL. Use `children` for a custom preview such as a canvas. */
  src?: string;
  children?: React.ReactNode;
  selected?: boolean;
  defaultSelected?: boolean;
  onSelectedChange?: (selected: boolean) => void;
  rotation?: PageRotation;
  onRotate?: () => void;
  onDelete?: () => void;
  /** True while this page is being dragged. */
  dragging?: boolean;
  /** Props for the drag handle, for example from a sortable library. Omit to hide the handle. */
  dragHandleProps?: React.ComponentProps<"button">;
  blank?: boolean;
  scanned?: boolean;
  /** Replaces the English strings, for localisation. */
  labels?: Partial<PageThumbnailLabels>;
}

/** Page card for the organise grid: preview, page badge, selection, rotate, delete and drag. */
function PageThumbnail({
  className,
  page,
  src,
  children,
  selected: selectedProp,
  defaultSelected = false,
  onSelectedChange,
  rotation = 0,
  onRotate,
  onDelete,
  dragging = false,
  dragHandleProps,
  blank = false,
  scanned = false,
  labels: labelOverrides,
  ...props
}: PageThumbnailProps) {
  const labels = { ...DEFAULT_LABELS, ...labelOverrides };
  const [innerSelected, setInnerSelected] = React.useState(defaultSelected);
  const selected = selectedProp ?? innerSelected;

  const actionClass =
    "bg-background/90 opacity-0 shadow-elevation-1 transition-opacity group-focus-within/page:opacity-100 group-hover/page:opacity-100 pointer-coarse:opacity-100 focus-visible:opacity-100 motion-reduce:transition-none";

  return (
    <fieldset
      data-slot="page-thumbnail"
      data-selected={selected || undefined}
      data-dragging={dragging || undefined}
      className={cn(
        "group/page relative m-0 flex min-w-0 flex-col gap-2 rounded-lg border bg-card p-2 text-card-foreground transition-[box-shadow,opacity] motion-reduce:transition-none",
        "data-selected:border-primary data-selected:ring-2 data-selected:ring-primary data-dragging:opacity-60 data-dragging:shadow-elevation-3",
        className,
      )}
      {...props}
    >
      <legend className="sr-only">{labels.page(page)}</legend>
      {/* A page keeps its own direction: previews are never mirrored in RTL. */}
      <div
        dir="ltr"
        data-slot="page-thumbnail-preview"
        className="relative flex aspect-3/4 items-center justify-center overflow-hidden rounded-md bg-muted"
      >
        <div
          data-slot="page-thumbnail-page"
          data-rotation={rotation}
          className="flex aspect-3/4 h-full items-center justify-center bg-white shadow-elevation-1 transition-transform duration-200 motion-reduce:transition-none"
          style={{
            transform: `rotate(${rotation}deg)${rotation % 180 === 0 ? "" : " scale(0.75)"}`,
          }}
        >
          {children ??
            (src && (
              <img src={src} alt="" className="size-full object-contain" draggable={false} />
            ))}
        </div>
      </div>

      <span className="absolute start-3 top-3">
        <input
          type="checkbox"
          aria-label={labels.select(page)}
          checked={selected}
          onChange={(event) => {
            setInnerSelected(event.target.checked);
            onSelectedChange?.(event.target.checked);
          }}
          className="size-4 cursor-pointer rounded-sm border border-input bg-background accent-primary outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
        />
      </span>

      <div className="absolute end-3 top-3 flex gap-1">
        {onRotate && (
          <Button
            variant="outline"
            size="icon-xs"
            aria-label={labels.rotate(page)}
            className={actionClass}
            onClick={onRotate}
          >
            <RotateCwIcon aria-hidden="true" />
          </Button>
        )}
        {onDelete && (
          <Button
            variant="outline"
            size="icon-xs"
            aria-label={labels.remove(page)}
            className={cn(actionClass, "text-destructive")}
            onClick={onDelete}
          >
            <Trash2Icon aria-hidden="true" />
          </Button>
        )}
      </div>

      <div className="flex items-center justify-between gap-1">
        {dragHandleProps ? (
          <button
            type="button"
            aria-label={labels.drag(page)}
            {...dragHandleProps}
            className={cn(
              "inline-flex size-6 cursor-grab touch-none items-center justify-center rounded-md text-muted-foreground outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50 active:cursor-grabbing",
              dragHandleProps.className,
            )}
          >
            <GripVerticalIcon aria-hidden="true" className="size-4" />
          </button>
        ) : (
          <span className="size-6" aria-hidden="true" />
        )}
        <span
          data-slot="page-thumbnail-badge"
          className="rounded-full bg-secondary px-2 py-0.5 text-xs font-medium text-secondary-foreground tabular-nums"
        >
          {page}
        </span>
        <span className="flex min-w-6 justify-end gap-1">
          {blank && (
            <span
              data-slot="page-thumbnail-blank"
              className="rounded-full bg-warning-soft px-1.5 py-0.5 text-[0.65rem] font-medium text-warning"
            >
              {labels.blank}
            </span>
          )}
          {scanned && (
            <span
              data-slot="page-thumbnail-scanned"
              className="rounded-full bg-info-soft px-1.5 py-0.5 text-[0.65rem] font-medium text-info"
            >
              {labels.scanned}
            </span>
          )}
        </span>
      </div>
    </fieldset>
  );
}

export { PageThumbnail };
