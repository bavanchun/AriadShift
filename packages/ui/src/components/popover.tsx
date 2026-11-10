"use client";

import { Popover as PopoverPrimitive } from "@base-ui/react/popover";
import { cn } from "#lib/utils";

function Popover(props: PopoverPrimitive.Root.Props) {
  return <PopoverPrimitive.Root data-slot="popover" {...props} />;
}

function PopoverTrigger(props: PopoverPrimitive.Trigger.Props) {
  return <PopoverPrimitive.Trigger data-slot="popover-trigger" {...props} />;
}

function PopoverClose(props: PopoverPrimitive.Close.Props) {
  return <PopoverPrimitive.Close data-slot="popover-close" {...props} />;
}

export interface PopoverContentProps extends PopoverPrimitive.Popup.Props {
  /** Placement relative to the trigger. `inline-start` and `inline-end` follow the writing direction. */
  side?: PopoverPrimitive.Positioner.Props["side"];
  align?: PopoverPrimitive.Positioner.Props["align"];
  sideOffset?: number;
  /** Element the popover is portalled into. Defaults to `document.body`. */
  container?: PopoverPrimitive.Portal.Props["container"];
}

/** Anchored panel that opens from its trigger and closes on Escape or an outside press. */
function PopoverContent({
  className,
  side = "bottom",
  align = "center",
  sideOffset = 8,
  container,
  ...props
}: PopoverContentProps) {
  return (
    <PopoverPrimitive.Portal container={container}>
      <PopoverPrimitive.Positioner
        side={side}
        align={align}
        sideOffset={sideOffset}
        className="z-50"
      >
        <PopoverPrimitive.Popup
          data-slot="popover-content"
          className={cn(
            "w-72 origin-(--transform-origin) rounded-lg border bg-popover p-3 text-sm text-popover-foreground shadow-elevation-2 outline-none transition-[opacity,transform] duration-150",
            "data-ending-style:scale-95 data-ending-style:opacity-0 data-starting-style:scale-95 data-starting-style:opacity-0 motion-reduce:transition-none motion-reduce:data-ending-style:scale-100 motion-reduce:data-starting-style:scale-100",
            className,
          )}
          {...props}
        />
      </PopoverPrimitive.Positioner>
    </PopoverPrimitive.Portal>
  );
}

export { Popover, PopoverClose, PopoverContent, PopoverTrigger };
