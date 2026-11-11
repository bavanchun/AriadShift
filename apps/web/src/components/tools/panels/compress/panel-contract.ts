"use client";

import { useEffect, useRef } from "react";
import type { ToolDescriptor } from "../../../../converter/tool-catalog";
import type { WorkspaceFile } from "../../../workspace/types";
import type { ParamsValidation } from "../../params-form";

/** What the tool inspector hands to a purpose-built panel in place of the generic form. */
export interface CustomToolPanelProps {
  tool: ToolDescriptor;
  files: WorkspaceFile[];
  pageCount?: number;
  disabled?: boolean;
  locale?: string;
  onChange: (params: Record<string, unknown>, validation: ParamsValidation) => void;
}

/** Reports the panel's parameters whenever they, or their validation, actually change. */
export function useEmitParams(
  params: Record<string, unknown>,
  validation: ParamsValidation,
  onChange: CustomToolPanelProps["onChange"],
) {
  const onChangeRef = useRef(onChange);
  useEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);
  const last = useRef<string | null>(null);
  useEffect(() => {
    const emission = JSON.stringify({ params, validation });
    if (last.current === emission) return;
    last.current = emission;
    onChangeRef.current(params, validation);
  }, [params, validation]);
}

/** Human-readable decimal size, for example `2.4 MB`. */
export function formatBytes(bytes: number, locale?: string): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1;
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: digits, minimumFractionDigits: digits }).format(value)} ${units[unit]}`;
}

export const CONTROL_CLASS =
  "min-w-0 rounded-lg border border-input bg-background px-2.5 text-sm text-foreground outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20";

export const VALID: ParamsValidation = { isValid: true, errors: {} };
