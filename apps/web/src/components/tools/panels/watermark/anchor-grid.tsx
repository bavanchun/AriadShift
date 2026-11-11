"use client";

import { cn } from "@ariadshift/ui/lib/utils";
import { ANCHOR_LABELS, ANCHORS, type Anchor } from "./watermark-model";

/** 3×3 position picker. It is physical (left/right), like the page it positions on, in any locale. */
export function AnchorGrid({
  value,
  onChange,
  disabled,
}: {
  value: Anchor;
  onChange: (anchor: Anchor) => void;
  disabled?: boolean;
}) {
  return (
    <fieldset className="grid gap-1.5" disabled={disabled}>
      <legend className="mb-1 text-sm font-medium">Position</legend>
      <div className="grid w-28 grid-cols-3 gap-1" dir="ltr">
        {ANCHORS.map((anchor) => (
          <button
            key={anchor}
            type="button"
            aria-label={ANCHOR_LABELS[anchor]}
            aria-pressed={value === anchor}
            onClick={() => onChange(anchor)}
            className={cn(
              "flex aspect-square cursor-pointer items-center justify-center rounded-md border outline-none transition-colors focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50",
              value === anchor
                ? "border-primary bg-primary/10"
                : "border-input bg-background hover:bg-muted",
            )}
          >
            <span
              aria-hidden="true"
              className={cn(
                "size-2 rounded-full",
                value === anchor ? "bg-primary" : "bg-muted-foreground/40",
              )}
            />
          </button>
        ))}
      </div>
    </fieldset>
  );
}
