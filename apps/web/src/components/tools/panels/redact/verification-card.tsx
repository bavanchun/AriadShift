"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@ariadshift/ui/components/card";
import { CircleCheckIcon, CircleDashedIcon, CircleXIcon, TriangleAlertIcon } from "lucide-react";
import { redactCopy } from "./copy";

export type RedactionCheckStatus = "pass" | "fail" | "unavailable";
export type RedactionVerificationStatus = "verified" | "partial" | "unverifiable" | "failed";

export const REDACTION_CHECKS = [
  "pdfium_text",
  "core_text",
  "region_render",
  "image_pixels",
  "object_graph",
  "rewrite_history",
] as const;
export type RedactionCheckName = (typeof REDACTION_CHECKS)[number];

/** The `pdf.redact` verification report, as the engine returns it. It never holds matched text. */
export interface RedactionVerificationReport {
  status: RedactionVerificationStatus;
  checks: Record<RedactionCheckName, RedactionCheckStatus>;
  target_count: number;
  match_count: number;
  warnings?: string[];
  carriers?: Partial<Record<CarrierKey, number>>;
}

export const CARRIER_KEYS = [
  "text_removed",
  "images_modified",
  "vectors_inspected",
  "annotations_removed",
  "metadata_scrubbed",
  "attachments_removed",
] as const;
export type CarrierKey = (typeof CARRIER_KEYS)[number];

const STATUS_STYLE: Record<RedactionVerificationStatus, "default" | "secondary" | "destructive"> = {
  verified: "default",
  partial: "secondary",
  unverifiable: "secondary",
  failed: "destructive",
};

function CheckIcon({ status }: { status: RedactionCheckStatus | "pending" }) {
  const props = { "aria-hidden": true, className: "mt-0.5 size-4 shrink-0" } as const;
  if (status === "pass")
    return <CircleCheckIcon {...props} className="mt-0.5 size-4 shrink-0 text-primary" />;
  if (status === "fail")
    return <CircleXIcon {...props} className="mt-0.5 size-4 shrink-0 text-destructive" />;
  if (status === "unavailable")
    return (
      <TriangleAlertIcon {...props} className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
    );
  return <CircleDashedIcon {...props} className="mt-0.5 size-4 shrink-0 text-muted-foreground" />;
}

/**
 * The checklist that proves a redaction removed content rather than hiding it. Without a
 * `report` it lists what will be checked after the tool runs; with one it shows each gate's result.
 */
export function RedactionVerificationCard({
  report,
  standalone = false,
}: {
  report?: RedactionVerificationReport;
  /** The report comes from `pdf.redact_verify`, which cannot see the original file. */
  standalone?: boolean;
}) {
  const copy = redactCopy.verification;
  const status = report?.status ?? "planned";
  const carriers = CARRIER_KEYS.filter((key) => report?.carriers?.[key] !== undefined);
  return (
    <Card
      data-slot="redaction-verification-card"
      data-status={status}
      aria-label={copy.title}
      className="text-start"
    >
      <CardHeader className="flex-row items-center justify-between gap-2">
        <CardTitle>{report ? copy.title : copy.plannedTitle}</CardTitle>
        <Badge variant={report ? STATUS_STYLE[report.status] : "outline"}>
          {copy.status[status]}
        </Badge>
      </CardHeader>
      <CardContent className="grid gap-3">
        <p className="text-sm text-muted-foreground">
          {report ? copy.statusBody[report.status] : copy.plannedBody}
        </p>
        <ul className="grid gap-2" aria-label={copy.title}>
          {REDACTION_CHECKS.map((name) => {
            const result = report?.checks[name] ?? "pending";
            return (
              <li
                key={name}
                data-check={name}
                data-result={result}
                className="flex items-start gap-2 text-sm"
              >
                <CheckIcon status={result} />
                <span className="grid gap-0.5">
                  <span className="font-medium">
                    {copy.checks[name].title}
                    <span className="ms-2 text-xs font-normal text-muted-foreground">
                      {copy.check[result]}
                    </span>
                  </span>
                  <span className="text-xs text-muted-foreground">{copy.checks[name].body}</span>
                </span>
              </li>
            );
          })}
        </ul>
        {report ? (
          <dl className="grid grid-cols-2 gap-2 text-sm">
            <div>
              <dt className="text-xs text-muted-foreground">{copy.summary.targets}</dt>
              <dd className="font-medium tabular-nums">{report.target_count}</dd>
            </div>
            <div>
              <dt className="text-xs text-muted-foreground">{copy.summary.matches}</dt>
              <dd className="font-medium tabular-nums">{report.match_count}</dd>
            </div>
          </dl>
        ) : null}
        {carriers.length ? (
          <div className="grid gap-1">
            <p className="text-xs font-medium text-muted-foreground">{copy.carriers.title}</p>
            <ul className="grid gap-0.5 text-sm">
              {carriers.map((key) => (
                <li key={key} className="flex justify-between gap-2">
                  <span>{copy.carriers[key]}</span>
                  <span className="tabular-nums">{report?.carriers?.[key]}</span>
                </li>
              ))}
            </ul>
          </div>
        ) : null}
        {report?.warnings?.length ? (
          <div className="grid gap-1" data-slot="redaction-warnings">
            <p className="text-xs font-medium text-destructive">{copy.warnings}</p>
            <ul className="grid list-disc gap-0.5 ps-4 text-sm">
              {report.warnings.map((warning) => (
                <li key={warning}>{warning}</li>
              ))}
            </ul>
          </div>
        ) : null}
        {standalone ? <p className="text-xs text-muted-foreground">{copy.standalone}</p> : null}
      </CardContent>
    </Card>
  );
}
