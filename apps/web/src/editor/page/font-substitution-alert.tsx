"use client";

import { Alert, AlertDescription, AlertTitle } from "@ariadshift/ui/components/alert";
import { TriangleAlertIcon } from "lucide-react";
import { DEFAULT_PAGE_MODE_LABELS, formatLabel, type PageModeLabels } from "./page-mode-labels";
import type { FontResolution } from "./page-model";

export interface FontSubstitutionAlertProps {
  resolution: FontResolution;
  labels?: Partial<PageModeLabels>;
  className?: string;
}

/** The visible warning shown whenever edited text cannot use the PDF's embedded font. */
export function FontSubstitutionAlert({
  resolution,
  labels,
  className,
}: FontSubstitutionAlertProps) {
  const copy = { ...DEFAULT_PAGE_MODE_LABELS, ...labels };
  if (!resolution.substituted) return null;
  return (
    <Alert data-slot="font-substitution-alert" className={className}>
      <TriangleAlertIcon aria-hidden="true" />
      <AlertTitle>{copy.substitutionTitle}</AlertTitle>
      <AlertDescription className="flex flex-col gap-1">
        <p>
          {formatLabel(copy.substitutionBody, {
            from: resolution.originalLabel,
            to: resolution.resolvedLabel,
          })}
        </p>
        <p lang="vi" className="text-xs text-muted-foreground">
          {formatLabel(copy.substitutionChars, { chars: resolution.missingChars.join(" ") })}
        </p>
      </AlertDescription>
    </Alert>
  );
}
