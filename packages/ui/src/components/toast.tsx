"use client";

import { cva, type VariantProps } from "class-variance-authority";
import { CircleAlertIcon, CircleCheckIcon, InfoIcon, TriangleAlertIcon, XIcon } from "lucide-react";
import * as React from "react";
import { Button } from "#components/button";
import { cn } from "#lib/utils";

export type ToastVariant = "default" | "success" | "warning" | "destructive";

const DEFAULT_DURATION = 5000;
const MAX_TOASTS = 5;

const toastVariants = cva(
  "group/toast pointer-events-auto relative grid w-full grid-cols-[auto_1fr_auto] items-start gap-x-3 gap-y-1 overflow-hidden rounded-lg border bg-popover p-3 text-sm text-popover-foreground shadow-elevation-2 animate-in fade-in-0 slide-in-from-bottom-2 motion-reduce:animate-none",
  {
    variants: {
      variant: {
        default: "[&>[data-slot=toast-icon]]:text-info",
        success: "border-success/40 [&>[data-slot=toast-icon]]:text-success",
        warning: "border-warning/40 [&>[data-slot=toast-icon]]:text-warning",
        destructive: "border-destructive/50 [&>[data-slot=toast-icon]]:text-destructive",
      },
    },
    defaultVariants: { variant: "default" },
  },
);

const ICONS = {
  default: InfoIcon,
  success: CircleCheckIcon,
  warning: TriangleAlertIcon,
  destructive: CircleAlertIcon,
} satisfies Record<ToastVariant, React.ComponentType<{ className?: string }>>;

interface ToastContextValue {
  dismiss: () => void;
}

const ToastContext = React.createContext<ToastContextValue | null>(null);

export interface ToastProps
  extends Omit<React.ComponentProps<"div">, "title">,
    VariantProps<typeof toastVariants> {
  /** Milliseconds before the toast dismisses itself; `Infinity` keeps it until closed. */
  duration?: number;
  /** Shows the Golden Thread draining over the toast's lifetime. */
  thread?: boolean;
  /** Called when the toast times out or its close or action button is used. */
  onDismiss?: () => void;
}

/** One notification. Auto-dismisses after `duration`, pausing while hovered or focused. */
function Toast({
  className,
  variant = "default",
  duration = DEFAULT_DURATION,
  thread = false,
  onDismiss,
  children,
  onPointerEnter,
  onPointerLeave,
  onFocus,
  onBlur,
  ...props
}: ToastProps) {
  const [paused, setPaused] = React.useState(false);
  const remaining = React.useRef(duration);
  const dismissRef = React.useRef(onDismiss);
  dismissRef.current = onDismiss;
  const timed = Number.isFinite(duration) && duration > 0;

  React.useEffect(() => {
    remaining.current = duration;
  }, [duration]);

  React.useEffect(() => {
    if (!timed || paused) {
      return;
    }
    const startedAt = Date.now();
    const timer = window.setTimeout(() => dismissRef.current?.(), remaining.current);
    return () => {
      window.clearTimeout(timer);
      remaining.current = Math.max(0, remaining.current - (Date.now() - startedAt));
    };
  }, [timed, paused]);

  const context = React.useMemo(() => ({ dismiss: () => dismissRef.current?.() }), []);
  const Icon = ICONS[variant ?? "default"];

  return (
    <ToastContext.Provider value={context}>
      {/* biome-ignore lint/a11y/noStaticElementInteractions: hover and focus only pause the timer; the content stays operable without them */}
      <div
        role={variant === "destructive" || variant === "warning" ? "alert" : "status"}
        data-slot="toast"
        data-variant={variant}
        className={cn(toastVariants({ variant }), className)}
        onPointerEnter={(event) => {
          setPaused(true);
          onPointerEnter?.(event);
        }}
        onPointerLeave={(event) => {
          setPaused(false);
          onPointerLeave?.(event);
        }}
        onFocus={(event) => {
          setPaused(true);
          onFocus?.(event);
        }}
        onBlur={(event) => {
          if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
            setPaused(false);
          }
          onBlur?.(event);
        }}
        {...props}
      >
        <Icon aria-hidden="true" data-slot="toast-icon" className="mt-0.5 size-4" />
        {children}
        {thread && (
          <span
            aria-hidden="true"
            data-slot="toast-thread"
            data-paused={paused || undefined}
            className="absolute inset-x-0 bottom-0 h-0.5 origin-[0%_50%] bg-thread rtl:origin-[100%_50%] motion-reduce:animate-none group-hover/toast:[animation-play-state:paused] data-paused:[animation-play-state:paused] animate-thread-drain"
            style={timed ? { animationDuration: `${duration}ms` } : { animation: "none" }}
          />
        )}
      </div>
    </ToastContext.Provider>
  );
}

function ToastTitle({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div data-slot="toast-title" className={cn("col-start-2 font-medium", className)} {...props} />
  );
}

function ToastDescription({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="toast-description"
      className={cn("col-start-2 text-muted-foreground", className)}
      {...props}
    />
  );
}

/** Button inside a toast, such as Undo. Dismisses the toast after its own handler runs. */
function ToastAction({ className, onClick, ...props }: React.ComponentProps<typeof Button>) {
  const context = React.useContext(ToastContext);
  return (
    <Button
      data-slot="toast-action"
      variant="outline"
      size="sm"
      className={cn("col-start-2 mt-1 w-fit", className)}
      onClick={(event) => {
        onClick?.(event);
        context?.dismiss();
      }}
      {...props}
    />
  );
}

function ToastClose({
  className,
  "aria-label": ariaLabel = "Dismiss notification",
  ...props
}: React.ComponentProps<typeof Button>) {
  const context = React.useContext(ToastContext);
  return (
    <Button
      data-slot="toast-close"
      variant="ghost"
      size="icon-xs"
      aria-label={ariaLabel}
      className={cn("col-start-3 row-start-1", className)}
      onClick={() => context?.dismiss()}
      {...props}
    >
      <XIcon aria-hidden="true" />
    </Button>
  );
}

export interface ToastOptions {
  title?: React.ReactNode;
  description?: React.ReactNode;
  variant?: ToastVariant;
  /** Action button content, usually a `ToastAction`. */
  action?: React.ReactNode;
  duration?: number;
  /** Shows the Golden Thread countdown. */
  thread?: boolean;
}

interface ToastRecord extends ToastOptions {
  id: string;
}

interface ToastStore {
  toasts: ToastRecord[];
  toast: (options: ToastOptions) => string;
  dismiss: (id?: string) => void;
}

const StoreContext = React.createContext<ToastStore | null>(null);

export interface ToastProviderProps {
  children?: React.ReactNode;
  /** Default `duration` for toasts that do not set one. */
  duration?: number;
}

/** Holds the toast queue. Render one `ToastViewport` below it. */
function ToastProvider({ children, duration = DEFAULT_DURATION }: ToastProviderProps) {
  const [toasts, setToasts] = React.useState<ToastRecord[]>([]);
  const counter = React.useRef(0);

  const toast = React.useCallback(
    (options: ToastOptions) => {
      counter.current += 1;
      const id = `toast-${counter.current}`;
      setToasts((current) => [...current, { duration, ...options, id }].slice(-MAX_TOASTS));
      return id;
    },
    [duration],
  );

  const dismiss = React.useCallback((id?: string) => {
    setToasts((current) => (id === undefined ? [] : current.filter((item) => item.id !== id)));
  }, []);

  const store = React.useMemo(() => ({ toasts, toast, dismiss }), [toasts, toast, dismiss]);
  return <StoreContext.Provider value={store}>{children}</StoreContext.Provider>;
}

/** Queue access: `toast(options)` returns an id; `dismiss(id)` or `dismiss()` removes toasts. */
function useToast(): ToastStore {
  const store = React.useContext(StoreContext);
  if (!store) {
    throw new Error("useToast must be used inside <ToastProvider>.");
  }
  return store;
}

export interface ToastViewportProps extends Omit<React.ComponentProps<"section">, "children"> {
  /** Accessible name of the notification region. */
  label?: string;
}

/** Fixed region at the bottom-end corner that renders the queued toasts. */
function ToastViewport({ className, label = "Notifications", ...props }: ToastViewportProps) {
  const { toasts, dismiss } = useToast();
  return (
    <section
      aria-label={label}
      aria-live="polite"
      data-slot="toast-viewport"
      className={cn(
        "pointer-events-none fixed bottom-4 end-4 z-[60] flex w-[min(24rem,calc(100%-2rem))] flex-col gap-2",
        className,
      )}
      {...props}
    >
      {toasts.map(({ id, title, description, action, ...rest }) => (
        <Toast key={id} onDismiss={() => dismiss(id)} {...rest}>
          <div className="col-start-2 grid gap-0.5">
            {title && <ToastTitle>{title}</ToastTitle>}
            {description && <ToastDescription>{description}</ToastDescription>}
          </div>
          <ToastClose />
          {action}
        </Toast>
      ))}
    </section>
  );
}

export {
  Toast,
  ToastAction,
  ToastClose,
  ToastDescription,
  ToastProvider,
  ToastTitle,
  ToastViewport,
  toastVariants,
  useToast,
};
