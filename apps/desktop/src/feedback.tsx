import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import {
  BanIcon,
  CheckIcon,
  ClockIcon,
  LoaderCircleIcon,
  TriangleAlertIcon,
  XIcon,
} from "lucide-react";
import { useEffect } from "react";
import type { JobStatus, JobView } from "./bindings/types";
import { fileName } from "./formats";

/** A visual shortcut hint. Assistive technology reads `aria-keyshortcuts` on the control. */
export function Kbd({ children }: { children: string }) {
  return (
    <span className="kbd" aria-hidden="true">
      {children}
    </span>
  );
}

const statuses: Record<
  JobStatus,
  {
    label: string;
    icon: typeof CheckIcon;
    variant: "default" | "secondary" | "destructive" | "outline";
  }
> = {
  queued: { label: "Waiting", icon: ClockIcon, variant: "outline" },
  running: { label: "Converting", icon: LoaderCircleIcon, variant: "secondary" },
  succeeded: { label: "Converted", icon: CheckIcon, variant: "default" },
  failed: { label: "Failed", icon: TriangleAlertIcon, variant: "destructive" },
  cancelled: { label: "Cancelled", icon: BanIcon, variant: "outline" },
};

/** A job status as icon plus text, so it never relies on colour alone. */
export function StatusBadge({ status }: { status: JobStatus }) {
  const { label, icon: Icon, variant } = statuses[status];
  return (
    <Badge variant={variant} data-status={status}>
      <Icon aria-hidden="true" className={status === "running" ? "spin" : undefined} />
      {label}
    </Badge>
  );
}

export type ToastAction = { label: string; run: () => void };
export type Toast = {
  id: string;
  tone: "success" | "error";
  text: string;
  actions: ToastAction[];
};

/** Success toasts leave on their own; the Queue keeps the result. Failures stay until dismissed. */
export const toastTimeout = 8000;

function ToastItem({ toast, onDismiss }: { toast: Toast; onDismiss: (id: string) => void }) {
  useEffect(() => {
    if (toast.tone !== "success") return;
    const timer = setTimeout(() => onDismiss(toast.id), toastTimeout);
    return () => clearTimeout(timer);
  }, [toast, onDismiss]);
  return (
    <div className="toast" data-tone={toast.tone}>
      {toast.tone === "success" ? (
        <CheckIcon aria-hidden="true" />
      ) : (
        <TriangleAlertIcon aria-hidden="true" />
      )}
      <p>{toast.text}</p>
      <div className="actions toast-actions">
        {toast.actions.map((action) => (
          <Button
            key={action.label}
            variant="outline"
            size="sm"
            onClick={() => {
              action.run();
              onDismiss(toast.id);
            }}
          >
            {action.label}
          </Button>
        ))}
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Dismiss notification"
          onClick={() => onDismiss(toast.id)}
        >
          <XIcon aria-hidden="true" />
        </Button>
      </div>
    </div>
  );
}

/** A polite live region, mounted before any toast so screen readers announce additions. */
export function Toasts({
  toasts,
  onDismiss,
}: {
  toasts: Toast[];
  onDismiss: (id: string) => void;
}) {
  return (
    <section className="toasts" aria-label="Conversion notifications" aria-live="polite">
      {toasts.map((toast) => (
        <ToastItem key={toast.id} toast={toast} onDismiss={onDismiss} />
      ))}
    </section>
  );
}

const plural = (count: number, word: string) => `${count} ${word}${count === 1 ? "" : "s"}`;

/** Handlers the completion toasts call; they must stay valid after the toast is created. */
export type FinishedHandlers = {
  reveal: (job: JobView) => void;
  retry: (job: JobView) => void;
  viewQueue: () => void;
};

/**
 * Toasts for jobs that just succeeded or failed. More than three at once collapse into one
 * summary, so a large batch does not bury the window.
 */
export function finishedToasts(finished: JobView[], handlers: FinishedHandlers): Toast[] {
  const viewQueue = { label: "View queue", run: handlers.viewQueue };
  const first = finished[0];
  if (!first) return [];
  if (finished.length > 3) {
    const failed = finished.filter((job) => job.status === "failed").length;
    return [
      {
        id: `batch-${first.id}`,
        tone: failed ? "error" : "success",
        text: `${plural(finished.length, "conversion")} finished: ${finished.length - failed} converted${failed ? `, ${failed} failed` : ""}.`,
        actions: [viewQueue],
      },
    ];
  }
  return finished.map(
    (job): Toast =>
      job.status === "succeeded"
        ? {
            id: job.id,
            tone: "success",
            text: `Converted ${fileName(job.input)} to ${job.target.toUpperCase()}.`,
            actions: [{ label: "Show output", run: () => handlers.reveal(job) }],
          }
        : {
            id: job.id,
            tone: "error",
            text: `Could not convert ${fileName(job.input)}${job.error ? `: ${job.error}` : "."}`,
            actions: [{ label: "Retry", run: () => handlers.retry(job) }, viewQueue],
          },
  );
}
