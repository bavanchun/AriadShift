"use client";

// Small building blocks shared by the account pages: a labelled text field,
// a section card with a real heading, and inline result messages.

import { Alert, AlertDescription } from "@ariadshift/ui/components/alert";
import { Card, CardContent, CardDescription, CardHeader } from "@ariadshift/ui/components/card";
import { cn } from "@ariadshift/ui/lib/utils";
import type { ComponentProps, ReactNode } from "react";
import { useCallback, useId, useState } from "react";
import { useAccount } from "./account-context";

/** Better Auth's default minimum password length. */
export const MIN_PASSWORD_LENGTH = 8;

export const INPUT_CLASS =
  "h-9 w-full rounded-lg border border-input bg-background px-3 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 aria-invalid:border-destructive disabled:opacity-50";

export interface TextFieldProps extends Omit<ComponentProps<"input">, "id" | "onChange"> {
  label: string;
  value: string;
  onValueChange: (value: string) => void;
  /** Help text read with the field. */
  hint?: string;
  /** Validation message; marks the field invalid. */
  error?: string | null;
}

export function TextField({
  label,
  value,
  onValueChange,
  hint,
  error,
  className,
  ...props
}: TextFieldProps) {
  const id = useId();
  const hintId = `${id}-hint`;
  const errorId = `${id}-error`;
  const describedBy = [hint ? hintId : null, error ? errorId : null].filter(Boolean).join(" ");
  return (
    <div className={cn("flex flex-col gap-1.5", className)}>
      <label htmlFor={id} className="text-sm font-medium">
        {label}
      </label>
      <input
        {...props}
        id={id}
        value={value}
        onChange={(event) => onValueChange(event.target.value)}
        aria-invalid={error ? true : undefined}
        aria-describedby={describedBy || undefined}
        className={INPUT_CLASS}
      />
      {hint ? (
        <p id={hintId} className="text-xs text-muted-foreground">
          {hint}
        </p>
      ) : null}
      {error ? (
        <p id={errorId} className="text-xs text-destructive">
          {error}
        </p>
      ) : null}
    </div>
  );
}

export function Section({
  title,
  description,
  children,
  headingId,
}: {
  title: string;
  description?: ReactNode;
  children: ReactNode;
  headingId?: string;
}) {
  const generated = useId();
  const id = headingId ?? generated;
  return (
    <section aria-labelledby={id}>
      <Card>
        <CardHeader>
          <h2 id={id} className="text-base font-semibold" tabIndex={-1}>
            {title}
          </h2>
          {description ? <CardDescription>{description}</CardDescription> : null}
        </CardHeader>
        <CardContent className="flex flex-col gap-4">{children}</CardContent>
      </Card>
    </section>
  );
}

/** Result of the last action. */
export type Outcome = { kind: "success" | "error"; message: string } | null;

/**
 * Shows a result next to the control that caused it. Failures are alerts;
 * successes are announced through the page's polite live region by `useOutcome`.
 */
export function OutcomeMessage({ outcome }: { outcome: Outcome }) {
  if (!outcome) {
    return null;
  }
  if (outcome.kind === "error") {
    return (
      <Alert variant="destructive">
        <AlertDescription>{outcome.message}</AlertDescription>
      </Alert>
    );
  }
  return <p className="text-sm text-muted-foreground">{outcome.message}</p>;
}

/** The last action's result, announcing successes once. */
export function useOutcome() {
  const { announce } = useAccount();
  const [outcome, setOutcome] = useState<Outcome>(null);
  const succeed = useCallback(
    (message: string) => {
      setOutcome({ kind: "success", message });
      announce(message);
    },
    [announce],
  );
  const fail = useCallback((message: string) => setOutcome({ kind: "error", message }), []);
  const clear = useCallback(() => setOutcome(null), []);
  return { outcome, succeed, fail, clear };
}

/** A medium-length local date, for example "25 Oct 2026". */
export function formatDate(iso: string): string {
  const date = new Date(iso);
  return Number.isNaN(date.getTime())
    ? "an unknown date"
    : date.toLocaleDateString(undefined, { dateStyle: "medium" });
}

/** The message of a thrown error, or a fallback for anything else. */
export function errorMessage(error: unknown, fallback: string): string {
  return error instanceof Error && error.message ? error.message : fallback;
}
