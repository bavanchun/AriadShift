"use client";

import { Progress as ProgressPrimitive } from "@base-ui/react/progress";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "#lib/utils";

const indicatorVariants = cva(
  "h-full rounded-full transition-[width] duration-300 motion-reduce:transition-none",
  {
    variants: {
      variant: {
        default: "bg-primary",
        /** The Golden Thread: multi-step pipelines, recipes and batches. */
        golden: "bg-linear-to-r from-gold-soft via-thread to-gold rtl:bg-linear-to-l",
      },
    },
    defaultVariants: { variant: "default" },
  },
);

export interface ProgressProps
  extends Omit<ProgressPrimitive.Root.Props, "value">,
    VariantProps<typeof indicatorVariants> {
  /** A number from `min` to `max`, or `null` for an indeterminate bar. */
  value: number | null;
  /** Accessible name. Required when no visible label is wired up. */
  "aria-label"?: string;
}

/** Determinate or indeterminate progress bar. */
function Progress({ className, value, variant = "default", ...props }: ProgressProps) {
  return (
    <ProgressPrimitive.Root
      data-slot="progress"
      data-variant={variant}
      value={value}
      className={cn("w-full", className)}
      {...props}
    >
      <ProgressPrimitive.Track
        data-slot="progress-track"
        className="relative h-2 w-full overflow-hidden rounded-full bg-muted"
      >
        <ProgressPrimitive.Indicator
          data-slot="progress-indicator"
          className={cn(
            indicatorVariants({ variant }),
            "data-indeterminate:w-1/3 data-indeterminate:animate-indeterminate rtl:data-indeterminate:animate-indeterminate-rtl motion-reduce:data-indeterminate:animate-none motion-reduce:data-indeterminate:opacity-60",
          )}
        />
      </ProgressPrimitive.Track>
    </ProgressPrimitive.Root>
  );
}

export { indicatorVariants as progressIndicatorVariants, Progress };
