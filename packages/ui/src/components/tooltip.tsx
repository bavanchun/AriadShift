"use client";

import { Tooltip as TooltipPrimitive } from "@base-ui/react/tooltip";
import * as React from "react";
import { cn } from "#lib/utils";

/** Id shared by a tooltip's trigger and popup so the trigger can be described by it. */
const TooltipIdContext = React.createContext<string | undefined>(undefined);

/** Shares the open delay between tooltips so moving along a toolbar feels instant. */
function TooltipProvider({ delay = 300, ...props }: TooltipPrimitive.Provider.Props) {
  return <TooltipPrimitive.Provider delay={delay} {...props} />;
}

function Tooltip(props: TooltipPrimitive.Root.Props) {
  const id = React.useId();
  return (
    <TooltipIdContext.Provider value={id}>
      <TooltipPrimitive.Root data-slot="tooltip" {...props} />
    </TooltipIdContext.Provider>
  );
}

function TooltipTrigger(props: TooltipPrimitive.Trigger.Props) {
  const id = React.useContext(TooltipIdContext);
  return <TooltipPrimitive.Trigger data-slot="tooltip-trigger" aria-describedby={id} {...props} />;
}

export interface TooltipContentProps extends TooltipPrimitive.Popup.Props {
  side?: TooltipPrimitive.Positioner.Props["side"];
  align?: TooltipPrimitive.Positioner.Props["align"];
  sideOffset?: number;
  /** Element the tooltip is portalled into. Defaults to `document.body`. */
  container?: TooltipPrimitive.Portal.Props["container"];
}

/** Short label shown on hover and keyboard focus. Keep it to plain text. */
function TooltipContent({
  className,
  side = "top",
  align = "center",
  sideOffset = 6,
  container,
  ...props
}: TooltipContentProps) {
  const id = React.useContext(TooltipIdContext);
  return (
    <TooltipPrimitive.Portal container={container}>
      <TooltipPrimitive.Positioner
        side={side}
        align={align}
        sideOffset={sideOffset}
        className="z-50"
      >
        <TooltipPrimitive.Popup
          id={id}
          role="tooltip"
          data-slot="tooltip-content"
          className={cn(
            "max-w-xs origin-(--transform-origin) rounded-md bg-foreground px-2 py-1 text-xs text-background shadow-elevation-2 transition-opacity duration-150 data-ending-style:opacity-0 data-starting-style:opacity-0 motion-reduce:transition-none",
            className,
          )}
          {...props}
        />
      </TooltipPrimitive.Positioner>
    </TooltipPrimitive.Portal>
  );
}

export { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger };
