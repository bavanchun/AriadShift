"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@ariadshift/ui/components/card";
import { signCopy } from "./copy";

export type SignatureStatus = "VALID" | "INVALID" | "SELF_SIGNED";
export type SignatureCoverage = "entire_file" | "entire_revision" | "partial" | "unknown";

export interface SignatureEntry {
  field_name: string;
  signer: {
    subject?: string;
    issuer?: string;
    serial_number?: string;
    not_valid_before?: string;
    not_valid_after?: string;
  };
  integrity: boolean;
  valid: boolean;
  coverage: SignatureCoverage;
  certificate_status?: string;
  timestamp?: { timestamp?: string; integrity?: boolean; valid?: boolean } | null;
  signing_time?: string | null;
  revocation_status?: "not_revoked" | "revoked" | "unknown";
  /** Not in every engine report; shown as "Not reported" when absent. */
  digest_algorithm?: string;
  modifications?: {
    docmdp_ok?: boolean | null;
    bottom_line?: boolean | null;
    modification_level?: string | null;
    diff_summary?: unknown;
  };
  error?: string;
}

/** The `pdf.sign_verify` report, as the sign engine writes it. */
export interface SignValidationReport {
  file?: string;
  valid: boolean;
  integrity: boolean;
  signature_count?: number;
  signatures_count?: number;
  signatures: SignatureEntry[];
}

/** An intact signature whose certificate vouches for itself is self-signed, not merely invalid. */
export function signatureStatus(entry: SignatureEntry): SignatureStatus {
  if (entry.valid) return "VALID";
  const { subject, issuer } = entry.signer;
  if (entry.integrity && subject && issuer && subject === issuer) return "SELF_SIGNED";
  return "INVALID";
}

const BADGE: Record<SignatureStatus, "default" | "secondary" | "destructive"> = {
  VALID: "default",
  SELF_SIGNED: "secondary",
  INVALID: "destructive",
};

function formatTime(value: string | null | undefined, locale?: string): string | null {
  if (!value) return null;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  try {
    return `${new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short", timeZone: "UTC" }).format(date)} UTC`;
  } catch {
    return date.toISOString();
  }
}

/** The common name of a distinguished name such as `Common Name: Ada, Organisation: Ariad`. */
export function commonName(subject: string | undefined): string | null {
  if (!subject) return null;
  const match = subject.match(/(?:^|,\s*)(?:Common Name|CN)\s*[:=]\s*([^,]+)/i);
  return (match?.[1] ?? subject).trim();
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid gap-0.5">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="text-sm break-words">{children}</dd>
    </div>
  );
}

function SignatureRow({ entry, locale }: { entry: SignatureEntry; locale?: string }) {
  const copy = signCopy.card;
  const status = signatureStatus(entry);
  const signed =
    formatTime(entry.signing_time, locale) ?? formatTime(entry.timestamp?.timestamp, locale);
  const modification = entry.modifications;
  return (
    <li
      data-slot="signature-entry"
      data-status={status}
      className="grid gap-3 rounded-lg border border-input p-3"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="text-sm font-medium">
          {copy.field}: {entry.field_name}
        </span>
        <Badge variant={BADGE[status]} data-slot="signature-status">
          {copy.status[status]}
        </Badge>
      </div>
      <p className="text-sm text-muted-foreground">{copy.statusBody[status]}</p>
      <dl className="grid gap-3 sm:grid-cols-2">
        <Row label={copy.signer}>{commonName(entry.signer.subject) ?? copy.unknown}</Row>
        <Row label={copy.issuer}>{commonName(entry.signer.issuer) ?? copy.unknown}</Row>
        <Row label={copy.signedAt}>{signed ?? copy.unknown}</Row>
        <Row label={copy.timestamp}>
          {formatTime(entry.timestamp?.timestamp, locale) ?? copy.unknown}
        </Row>
        <Row label={copy.digest}>{entry.digest_algorithm ?? copy.unknown}</Row>
        <Row label={copy.integrity}>{entry.integrity ? copy.intact : copy.broken}</Row>
        <Row label={copy.coverage}>{copy.coverageText[entry.coverage] ?? copy.unknown}</Row>
        <Row label={copy.revocation}>
          {copy.revocationText[entry.revocation_status ?? "unknown"]}
        </Row>
      </dl>
      {modification?.docmdp_ok === null || modification?.docmdp_ok === undefined ? null : (
        <p className="text-xs text-muted-foreground">
          {copy.modifications}: {modification.docmdp_ok ? copy.docmdpOk : copy.docmdpBad}
          {modification.modification_level ? ` (${modification.modification_level})` : ""}
        </p>
      )}
      {entry.error ? (
        <p role="alert" className="text-sm text-destructive">
          {copy.error}: {entry.error}
        </p>
      ) : null}
    </li>
  );
}

/** Structured display of a `pdf.sign_verify` result: one block per signature in the document. */
export function SignatureValidationCard({
  report,
  locale,
}: {
  report?: SignValidationReport;
  locale?: string;
}) {
  const copy = signCopy.card;
  const count = report
    ? (report.signature_count ?? report.signatures_count ?? report.signatures.length)
    : 0;
  return (
    <Card data-slot="signature-validation-card" aria-label={copy.title} className="text-start">
      <CardHeader className="flex-row items-center justify-between gap-2">
        <CardTitle>{copy.title}</CardTitle>
        {report && count > 0 ? (
          <Badge variant={report.valid ? "default" : "destructive"}>
            {report.valid ? copy.overall.valid : copy.overall.invalid}
          </Badge>
        ) : null}
      </CardHeader>
      <CardContent className="grid gap-3">
        {!report ? (
          <p className="text-sm text-muted-foreground">{copy.pending}</p>
        ) : count === 0 || report.signatures.length === 0 ? (
          <p className="text-sm text-muted-foreground">{copy.none}</p>
        ) : (
          <>
            <p className="text-xs text-muted-foreground">{copy.count(count)}</p>
            <ul className="grid gap-3" aria-label={copy.title}>
              {report.signatures.map((entry, index) => (
                <SignatureRow
                  // A document may repeat a field name across revisions.
                  key={`${entry.field_name}-${index}`}
                  entry={entry}
                  locale={locale}
                />
              ))}
            </ul>
          </>
        )}
      </CardContent>
    </Card>
  );
}
