"use client";

import { Toggle as TogglePrimitive } from "@base-ui/react/toggle";
import { ToggleGroup as ToggleGroupPrimitive } from "@base-ui/react/toggle-group";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "#lib/utils";

const toggleGroupItemVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-1.5 text-sm font-medium whitespace-nowrap transition-colors outline-none select-none focus-visible:z-10 focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50 motion-reduce:transition-none [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
  {
    variants: {
      variant: {
        default:
          "bg-transparent text-muted-foreground hover:bg-muted hover:text-foreground data-pressed:bg-secondary data-pressed:text-secondary-foreground",
        outline:
          "border border-input bg-transparent hover:bg-muted data-pressed:bg-secondary data-pressed:text-secondary-foreground",
      },
      size: {
        default: "h-8 min-w-8 px-2.5",
        sm: "h-7 min-w-7 px-2 text-[0.8rem]",
        lg: "h-9 min-w-9 px-3",
      },
    },
    defaultVariants: { variant: "default", size: "default" },
  },
);

type ToggleGroupVariantProps = VariantProps<typeof toggleGroupItemVariants>;

/** Per-group styling shared with the items through data attributes, so items stay prop-free. */
function ToggleGroup({
  className,
  variant = "default",
  size = "default",
  ...props
}: ToggleGroupPrimitive.Props & ToggleGroupVariantProps) {
  return (
    <ToggleGroupPrimitive
      data-slot="toggle-group"
      data-variant={variant}
      data-size={size}
      className={cn(
        "group/toggle-group inline-flex w-fit items-center gap-1 rounded-lg data-[variant=outline]:gap-0 data-[variant=outline]:shadow-elevation-1",
        className,
      )}
      {...props}
    />
  );
}

function ToggleGroupItem({
  className,
  variant,
  size,
  ...props
}: TogglePrimitive.Props & ToggleGroupVariantProps) {
  return (
    <TogglePrimitive
      data-slot="toggle-group-item"
      className={cn(
        toggleGroupItemVariants({ variant, size }),
        "rounded-lg group-data-[variant=outline]/toggle-group:rounded-none group-data-[variant=outline]/toggle-group:first:rounded-s-lg group-data-[variant=outline]/toggle-group:last:rounded-e-lg group-data-[variant=outline]/toggle-group:not-first:border-s-0",
        className,
      )}
      {...props}
    />
  );
}

export { ToggleGroup, ToggleGroupItem, toggleGroupItemVariants };
