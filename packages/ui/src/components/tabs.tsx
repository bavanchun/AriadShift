"use client";

import { Tabs as TabsPrimitive } from "@base-ui/react/tabs";
import { DirectionProvider } from "#lib/direction";
import { cn } from "#lib/utils";

export interface TabsProps extends TabsPrimitive.Root.Props {
  /**
   * Writing direction used for arrow-key navigation. Leave unset to follow the nearest
   * `DirectionProvider`; set it when the tabs sit in a different direction than the page.
   */
  dir?: "ltr" | "rtl";
}

/** Tabs with roving focus. Arrow keys follow the writing direction; Home and End jump to the ends. */
function Tabs({ className, orientation = "horizontal", dir, ...props }: TabsProps) {
  const tabs = (
    <TabsPrimitive.Root
      data-slot="tabs"
      orientation={orientation}
      dir={dir}
      className={cn("flex gap-3 data-[orientation=horizontal]:flex-col", className)}
      {...props}
    />
  );
  return dir ? <DirectionProvider direction={dir}>{tabs}</DirectionProvider> : tabs;
}

function TabsList({ className, ...props }: TabsPrimitive.List.Props) {
  return (
    <TabsPrimitive.List
      data-slot="tabs-list"
      className={cn(
        "relative inline-flex w-fit items-center gap-1 rounded-lg bg-muted p-1 text-muted-foreground data-[orientation=vertical]:flex-col data-[orientation=vertical]:items-stretch",
        className,
      )}
      {...props}
    />
  );
}

function TabsTrigger({ className, ...props }: TabsPrimitive.Tab.Props) {
  return (
    <TabsPrimitive.Tab
      data-slot="tabs-trigger"
      className={cn(
        "inline-flex h-7 items-center justify-center gap-1.5 rounded-md px-2.5 text-sm font-medium whitespace-nowrap transition-colors outline-none select-none hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50 motion-reduce:transition-none",
        "data-active:bg-card data-active:text-foreground data-active:shadow-elevation-1 dark:data-active:bg-input/50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
        className,
      )}
      {...props}
    />
  );
}

function TabsContent({ className, ...props }: TabsPrimitive.Panel.Props) {
  return (
    <TabsPrimitive.Panel
      data-slot="tabs-content"
      className={cn(
        "flex-1 rounded-md text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
        className,
      )}
      {...props}
    />
  );
}

export { Tabs, TabsContent, TabsList, TabsTrigger };
