"use client";
// The structured reports `pdf.redact`, `pdf.redact_verify` and `pdf.sign_verify` return, shown
// as the verification and validation cards once the tool has run.
import { useEffect, useState } from "react";
import {
  RedactionVerificationCard,
  type RedactionVerificationReport,
  SignatureValidationCard,
  type SignValidationReport,
} from "@/components/tools/panels";
import { openOutput } from "@/converter/client";
import type { ToolExecutionResult } from "@/converter/protocol";

const REDACT_STATUSES = ["verified", "partial", "unverifiable", "failed"];

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

export function isRedactionReport(value: unknown): value is RedactionVerificationReport {
  return (
    isObject(value) &&
    typeof value.status === "string" &&
    REDACT_STATUSES.includes(value.status) &&
    isObject(value.checks)
  );
}

export function isSignReport(value: unknown): value is SignValidationReport {
  return (
    isObject(value) &&
    typeof value.valid === "boolean" &&
    typeof value.integrity === "boolean" &&
    Array.isArray(value.signatures)
  );
}

/** The report inside inline result data, either the data itself or one nested key down. */
function findReport<T>(data: unknown, guard: (value: unknown) => value is T): T | null {
  if (guard(data)) return data;
  if (!isObject(data)) return null;
  for (const key of ["report", "verification", "redaction_report", "validation"]) {
    if (guard(data[key])) return data[key] as T;
  }
  return null;
}

type Kind = "redact" | "sign";

const kindOf = (toolId: string): Kind | null =>
  toolId === "pdf.redact" || toolId === "pdf.redact_verify"
    ? "redact"
    : toolId === "pdf.sign_verify"
      ? "sign"
      : null;

/** Inline data first; otherwise the JSON artifact the tool published next to its output. */
function useReport<T>(
  result: ToolExecutionResult,
  active: boolean,
  guard: (value: unknown) => value is T,
): T | null {
  const inline = active ? findReport(result.data, guard) : null;
  const [fetched, setFetched] = useState<T | null>(null);
  useEffect(() => {
    setFetched(null);
    if (!active || inline) return;
    const artifact = result.artifacts.find(
      (entry) => entry.mediaType === "application/json" || /\.json$/i.test(entry.name),
    );
    if (!artifact) return;
    let current = true;
    openOutput(artifact.location)
      .then(async (blob) => findReport(JSON.parse(await blob.text()), guard))
      .then(
        (report) => current && setFetched(report),
        () => undefined,
      );
    return () => {
      current = false;
    };
  }, [active, inline, result, guard]);
  return inline ?? fetched;
}

/** The verification or validation card for a finished run, or nothing for other tools. */
export function ToolReportCards({
  toolId,
  result,
  locale,
}: {
  toolId: string;
  result: ToolExecutionResult;
  locale?: string;
}) {
  const kind = kindOf(toolId);
  const redaction = useReport(result, kind === "redact", isRedactionReport);
  const signatures = useReport(result, kind === "sign", isSignReport);
  if (kind === "redact" && redaction)
    return (
      <RedactionVerificationCard report={redaction} standalone={toolId === "pdf.redact_verify"} />
    );
  if (kind === "sign" && signatures)
    return <SignatureValidationCard report={signatures} locale={locale} />;
  return null;
}
