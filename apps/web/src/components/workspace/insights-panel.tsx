"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { Card, CardContent, CardHeader, CardTitle } from "@ariadshift/ui/components/card";
import { Skeleton } from "@ariadshift/ui/components/skeleton";
import { LockIcon, TriangleAlertIcon } from "lucide-react";
import type { ReactNode } from "react";
import type { PdfFileInsights } from "@/converter/insights";
import type { ToolDescriptor } from "@/converter/tool-catalog";
import { useWorkspaceCopy } from "./copy";
import { toolLabel } from "./tool-label";
import type { InsightsState } from "./types";

export interface FileInsightsPanelProps {
  state: InsightsState;
  /** Tools the suggestions may name; ids missing here are not offered. */
  tools: ToolDescriptor[];
  onOpenTool: (toolId: string) => void;
  className?: string;
}

const LOW_DPI = 150;

function Fact({ label, children, warn }: { label: string; children: ReactNode; warn?: boolean }) {
  return (
    <li
      data-warn={warn || undefined}
      className="flex items-center gap-1.5 rounded-lg border bg-muted/40 px-2.5 py-1.5 text-sm data-warn:border-warning/50 data-warn:bg-warning-soft"
    >
      {warn && <TriangleAlertIcon aria-hidden="true" className="size-3.5 text-warning" />}
      <span className="sr-only">{label}: </span>
      {children}
    </li>
  );
}

/** The intent-first screen: what the dropped PDF contains, and one click to act on it. */
export function FileInsightsPanel({ state, tools, onOpenTool, className }: FileInsightsPanelProps) {
  const copy = useWorkspaceCopy().insights;
  const heading = (
    <CardHeader>
      <CardTitle>{copy.title}</CardTitle>
    </CardHeader>
  );
  if (state.status === "loading")
    return (
      <Card data-slot="file-insights" className={className}>
        {heading}
        <CardContent role="status" className="flex flex-col gap-2">
          <p className="text-sm text-muted-foreground">{copy.loading}</p>
          <Skeleton className="h-8 w-full" />
        </CardContent>
      </Card>
    );
  if (state.status === "failed")
    return (
      <Card data-slot="file-insights" className={className}>
        {heading}
        <CardContent role="alert" className="text-sm text-destructive">
          {copy.failed} {state.message}
        </CardContent>
      </Card>
    );

  const insights: PdfFileInsights = state.insights;
  const dpi = insights.estimatedImageDpi;
  const lowDpi =
    insights.estimatedImageDpi.lowDpiPages.length > 0 || (dpi.min !== null && dpi.min < LOW_DPI);
  const suggestions = insights.suggestedTools.flatMap((id) => {
    const tool = tools.find((candidate) => candidate.id === id);
    return tool ? [tool] : [];
  });
  return (
    <Card data-slot="file-insights" className={className}>
      {heading}
      <CardContent className="flex flex-col gap-4">
        <ul className="m-0 flex list-none flex-wrap gap-2 p-0">
          <Fact label={copy.title}>{copy.pages(insights.pageCount)}</Fact>
          <Fact label={copy.scanned(0)} warn={insights.scannedPageCount > 0}>
            {copy.scanned(insights.scannedPageCount)}
          </Fact>
          <Fact label={copy.blank(0)} warn={insights.blankPageCount > 0}>
            {copy.blank(insights.blankPageCount)}
          </Fact>
          <Fact label="DPI" warn={lowDpi}>
            {copy.dpi(dpi.min, dpi.max)}
          </Fact>
          <Fact label={copy.formFields(0)} warn={insights.emptyRequiredFormFields.count > 0}>
            {copy.formFields(insights.emptyRequiredFormFields.count)}
          </Fact>
          <Fact label={copy.signatures(0)}>{copy.signatures(insights.signatureFieldCount)}</Fact>
          {insights.hasMetadata && <Fact label={copy.metadata}>{copy.metadata}</Fact>}
          {insights.hasEditHistory && <Fact label={copy.editHistory}>{copy.editHistory}</Fact>}
          {insights.encrypted && (
            <Badge variant="outline">
              <LockIcon aria-hidden="true" />
              {copy.encrypted}
            </Badge>
          )}
          {insights.linearized && <Badge variant="outline">{copy.linearized}</Badge>}
        </ul>
        {lowDpi && dpi.lowDpiPages.length > 0 && (
          <p className="text-sm text-warning">{copy.lowDpi(dpi.lowDpiPages)}</p>
        )}
        {insights.warnings.map((warning) => (
          <p key={warning} className="text-sm text-muted-foreground">
            {warning}
          </p>
        ))}
        <div className="flex flex-col gap-2">
          <h3 className="text-sm font-semibold">{copy.suggested}</h3>
          {suggestions.length === 0 ? (
            <p className="text-sm text-muted-foreground">{copy.noSuggestions}</p>
          ) : (
            <ul className="m-0 flex list-none flex-wrap gap-2 p-0">
              {suggestions.map((tool) => (
                <li key={tool.id}>
                  <Button
                    variant="secondary"
                    aria-label={copy.open(toolLabel(tool))}
                    onClick={() => onOpenTool(tool.id)}
                  >
                    {toolLabel(tool)}
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
