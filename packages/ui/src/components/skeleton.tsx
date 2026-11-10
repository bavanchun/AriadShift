import { cva, type VariantProps } from "class-variance-authority";
import type * as React from "react";
import { cn } from "#lib/utils";

const skeletonVariants = cva("rounded-md bg-muted", {
  variants: {
    variant: {
      shimmer:
        "animate-shimmer bg-[linear-gradient(90deg,transparent_0%,color-mix(in_oklch,var(--foreground)_8%,transparent)_50%,transparent_100%)] bg-size-[200%_100%] motion-reduce:animate-none motion-reduce:bg-none",
      pulse: "animate-pulse motion-reduce:animate-none",
    },
  },
  defaultVariants: { variant: "shimmer" },
});

/** Placeholder block shown while content loads. Decorative: mark the loading region with `aria-busy`. */
function Skeleton({
  className,
  variant,
  ...props
}: React.ComponentProps<"div"> & VariantProps<typeof skeletonVariants>) {
  return (
    <div
      data-slot="skeleton"
      aria-hidden="true"
      className={cn(skeletonVariants({ variant }), className)}
      {...props}
    />
  );
}

export { Skeleton, skeletonVariants };
