"use client";

import { Slider as SliderPrimitive } from "@base-ui/react/slider";
import { cn } from "#lib/utils";

export interface SliderProps extends SliderPrimitive.Root.Props {
  /** Shows the current value next to the track. */
  showValue?: boolean;
  /** Accessible name for each thumb. A range slider passes one name per thumb. */
  thumbLabel?: string | string[];
}

/** Range slider. A single value renders one thumb; an array renders one thumb per value. */
function Slider({
  className,
  value,
  defaultValue,
  min = 0,
  max = 100,
  showValue = false,
  thumbLabel,
  ...props
}: SliderProps) {
  const current = value ?? defaultValue ?? min;
  const count = Array.isArray(current) ? current.length : 1;
  const labelAt = (index: number) =>
    Array.isArray(thumbLabel) ? thumbLabel[index] : count === 1 ? thumbLabel : undefined;

  return (
    <SliderPrimitive.Root
      data-slot="slider"
      value={value}
      defaultValue={defaultValue}
      min={min}
      max={max}
      thumbAlignment="edge"
      className={cn("flex w-full min-w-40 items-center gap-3 data-disabled:opacity-50", className)}
      {...props}
    >
      <SliderPrimitive.Control
        data-slot="slider-control"
        className="flex h-6 w-full touch-none items-center select-none"
      >
        <SliderPrimitive.Track
          data-slot="slider-track"
          className="relative h-1.5 w-full rounded-full bg-muted select-none"
        >
          <SliderPrimitive.Indicator
            data-slot="slider-range"
            className="rounded-full bg-primary select-none"
          />
          {Array.from({ length: count }, (_, index) => (
            <SliderPrimitive.Thumb
              // The thumb count is fixed for the lifetime of the value shape.
              // biome-ignore lint/suspicious/noArrayIndexKey: positional identity is the thumb's identity
              key={index}
              index={count === 1 ? undefined : index}
              data-slot="slider-thumb"
              aria-label={labelAt(index)}
              className="size-4 rounded-full border border-primary bg-background shadow-elevation-1 transition-shadow outline-none select-none hover:ring-3 hover:ring-ring/30 has-focus-visible:ring-3 has-focus-visible:ring-ring/50 data-dragging:ring-3 data-dragging:ring-ring/50 motion-reduce:transition-none"
            />
          ))}
        </SliderPrimitive.Track>
      </SliderPrimitive.Control>
      {showValue && (
        <SliderPrimitive.Value
          data-slot="slider-value"
          className="min-w-10 text-end text-sm text-muted-foreground tabular-nums"
        />
      )}
    </SliderPrimitive.Root>
  );
}

export { Slider };
