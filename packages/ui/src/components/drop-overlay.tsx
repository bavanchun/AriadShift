import { ShieldCheckIcon, UploadCloudIcon } from "lucide-react";
import type * as React from "react";
import { Badge } from "#components/badge";
import { cn } from "#lib/utils";

export interface DropOverlayProps extends Omit<React.ComponentProps<"div">, "title"> {
  /** Shows the overlay. When false nothing renders. */
  active: boolean;
  /** Covers the whole viewport instead of the nearest positioned ancestor. */
  fullscreen?: boolean;
  title?: React.ReactNode;
  description?: React.ReactNode;
  /** Accepted formats or tools to offer, shown as badges. */
  hints?: string[];
  /** Local-privacy statement. Keep it true: nothing leaves the device. */
  privacyLabel?: React.ReactNode;
}

/**
 * Overlay for a file drag. It ignores pointer events, so the drag events still reach the
 * element underneath and the drop target does not flicker.
 */
function DropOverlay({
  className,
  active,
  fullscreen = false,
  title = "Drop files to start",
  description,
  hints,
  privacyLabel = "On this device · 0 bytes uploaded",
  ...props
}: DropOverlayProps) {
  if (!active) {
    return null;
  }
  return (
    <div
      role="status"
      data-slot="drop-overlay"
      data-fullscreen={fullscreen || undefined}
      className={cn(
        "pointer-events-none z-40 flex items-center justify-center bg-scrim p-4 backdrop-blur-[2px] animate-in fade-in-0 motion-reduce:animate-none",
        fullscreen ? "fixed inset-0" : "absolute inset-0 rounded-[inherit]",
        className,
      )}
      {...props}
    >
      <div className="relative flex w-full max-w-md flex-col items-center gap-3 rounded-xl bg-popover p-8 text-center text-popover-foreground shadow-elevation-3">
        <svg
          aria-hidden="true"
          data-slot="drop-overlay-border"
          className="absolute inset-0 size-full overflow-visible text-thread"
        >
          <rect
            x="1"
            y="1"
            style={{ width: "calc(100% - 2px)", height: "calc(100% - 2px)" }}
            rx="12"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeDasharray="8 4"
            className="animate-dash-march motion-reduce:animate-none"
          />
        </svg>
        <UploadCloudIcon aria-hidden="true" className="size-10 text-primary" />
        <p className="text-base font-semibold">{title}</p>
        {description && <p className="text-sm text-muted-foreground">{description}</p>}
        {hints && hints.length > 0 && (
          <ul
            data-slot="drop-overlay-hints"
            className="m-0 flex list-none flex-wrap justify-center gap-1.5 p-0"
          >
            {hints.map((hint) => (
              <li key={hint}>
                <Badge variant="secondary">{hint}</Badge>
              </li>
            ))}
          </ul>
        )}
        <p
          data-slot="drop-overlay-privacy"
          className="inline-flex items-center gap-1.5 text-xs font-medium text-success"
        >
          <ShieldCheckIcon aria-hidden="true" className="size-3.5" />
          {privacyLabel}
        </p>
      </div>
    </div>
  );
}

export { DropOverlay };
