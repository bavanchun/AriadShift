"use client";

import { cn } from "@ariadshift/ui/lib/utils";
import { CheckIcon, CircleAlertIcon, LoaderCircleIcon, MinusIcon } from "lucide-react";
import { useWorkspaceCopy } from "./copy";
import type { ThreadStatus, ThreadStep } from "./types";

export interface GoldenThreadBarProps {
  steps: ThreadStep[];
  className?: string;
}

const DONE: ThreadStatus[] = ["completed", "skipped"];

/**
 * The pipeline as a golden thread: a bead per step, joined by a thread that is drawn up to
 * the last finished step. Colour is never the only signal; each step also names its state.
 */
export function GoldenThreadBar({ steps, className }: GoldenThreadBarProps) {
  const copy = useWorkspaceCopy().thread;
  return (
    <ol
      data-slot="golden-thread"
      aria-label={copy.label}
      className={cn("m-0 flex list-none items-start gap-0 p-0", className)}
    >
      {steps.map((step, index) => {
        const joined = index > 0 && DONE.includes(steps[index - 1]?.status as ThreadStatus);
        return (
          <li
            key={step.id}
            data-status={step.status}
            className="relative flex min-w-0 flex-1 flex-col items-center gap-1 text-center"
          >
            {index > 0 && (
              <span
                aria-hidden="true"
                data-slot="golden-thread-link"
                data-drawn={joined || undefined}
                className="absolute top-3 right-1/2 h-0.5 w-full -translate-y-1/2 rounded-full bg-border data-drawn:bg-thread"
                style={
                  joined
                    ? {
                        backgroundImage:
                          "linear-gradient(90deg, var(--thread-soft), var(--color-thread))",
                      }
                    : undefined
                }
              />
            )}
            <span
              className={cn(
                "relative z-10 flex size-6 items-center justify-center rounded-full border bg-card text-muted-foreground",
                step.status === "completed" && "border-thread bg-thread text-background",
                step.status === "running" && "border-thread text-thread",
                step.status === "skipped" && "border-thread/60 text-thread",
                step.status === "failed" && "border-destructive text-destructive",
              )}
            >
              {step.status === "completed" && <CheckIcon aria-hidden="true" className="size-3.5" />}
              {step.status === "running" && (
                <LoaderCircleIcon
                  aria-hidden="true"
                  className="size-3.5 animate-spin motion-reduce:animate-none"
                />
              )}
              {step.status === "skipped" && <MinusIcon aria-hidden="true" className="size-3.5" />}
              {step.status === "failed" && (
                <CircleAlertIcon aria-hidden="true" className="size-3.5" />
              )}
            </span>
            <span className="max-w-full truncate text-xs font-medium">{step.label}</span>
            <span className="text-[0.65rem] text-muted-foreground">{copy.status[step.status]}</span>
          </li>
        );
      })}
    </ol>
  );
}
