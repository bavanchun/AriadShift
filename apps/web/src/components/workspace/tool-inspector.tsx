"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@ariadshift/ui/components/card";
import { Progress } from "@ariadshift/ui/components/progress";
import { CloudIcon, MonitorIcon, PlayIcon, XIcon } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { ParamsForm, type ParamsValidation } from "@/components/tools/params-form";
import { toolHandoff } from "@/converter/handoff";
import type { ToolDescriptor } from "@/converter/tool-catalog";
import { useWorkspaceCopy } from "./copy";
import { HandoffPanel } from "./handoff-panel";
import { useToolLabel } from "./tool-label";
import type { WorkspaceFile } from "./types";

export interface ToolInspectorProps {
  tool: ToolDescriptor | null;
  files: WorkspaceFile[];
  /** Page count of the file the tool will read, for page-range validation. */
  pageCount?: number;
  /** The configured cloud API; "Run in Cloud" appears only when this is set. */
  cloudApiUrl?: string;
  locale?: string;
  /** Runs the tool in the browser worker. */
  onRun: (params: Record<string, unknown>) => void | Promise<void>;
  onCancel: () => void;
  /** Runs the tool in the cloud. Without it the cloud path is never offered. */
  onRunCloud?: (params: Record<string, unknown>) => void | Promise<void>;
  copyText?: (text: string) => Promise<void>;
  busy?: boolean;
  /** Latest progress message from the worker. */
  progress?: string | null;
  error?: string | null;
  className?: string;
}

/** Title, runtime and tier, the parameter form, and the one honest way to run the tool. */
export function ToolInspector({
  tool,
  files,
  pageCount,
  cloudApiUrl,
  locale,
  onRun,
  onCancel,
  onRunCloud,
  copyText,
  busy = false,
  progress,
  error,
  className,
}: ToolInspectorProps) {
  const copy = useWorkspaceCopy().inspector;
  const toolLabel = useToolLabel();
  const [params, setParams] = useState<Record<string, unknown>>({});
  const [validation, setValidation] = useState<ParamsValidation>({ isValid: true, errors: {} });
  const [forceDesktop, setForceDesktop] = useState(false);
  const toolId = tool?.id;
  // A different tool starts from the browser path again.
  useEffect(() => setForceDesktop(false), [toolId]);
  const onChange = useCallback((next: Record<string, unknown>, checked: ParamsValidation) => {
    setParams(next);
    setValidation(checked);
  }, []);

  if (!tool)
    return (
      <Card data-slot="tool-inspector" className={className}>
        <CardContent className="text-sm text-muted-foreground">{copy.empty}</CardContent>
      </Card>
    );

  const inputNames = files.map((file) => file.name);
  const handoff = toolHandoff(tool, params, inputNames, { cloudApiUrl, locale });
  // Cloud is a real path only when something can start it; otherwise the answer is the CLI.
  const mode = handoff.executionMode === "cloud" && !onRunCloud ? "desktop" : handoff.executionMode;
  const shown = forceDesktop ? "desktop" : mode;
  const ready = files.length > 0 && validation.isValid && !busy;

  return (
    <Card data-slot="tool-inspector" className={className}>
      <CardHeader>
        <CardTitle className="flex flex-wrap items-center gap-2">
          {toolLabel(tool)}
          <Badge variant="secondary" data-slot="runtime-badge">
            {copy.runtime[shown]}
          </Badge>
          <Badge variant={tool.tier === "pro" ? "default" : "outline"} data-slot="tier-badge">
            {copy.tier[tool.tier]}
          </Badge>
        </CardTitle>
        <CardDescription>{tool.summary}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <ParamsForm
          tool={tool}
          pageCount={pageCount}
          inputs={files.map((file) => ({ id: file.id, name: file.name }))}
          disabled={busy}
          onChange={onChange}
        />

        {shown === "desktop" ? (
          <>
            <HandoffPanel
              tool={tool}
              params={params}
              inputNames={inputNames}
              locale={locale}
              copyText={copyText}
            />
            {forceDesktop && (
              <Button
                variant="ghost"
                size="sm"
                className="self-start"
                onClick={() => setForceDesktop(false)}
              >
                {copy.switchBrowser}
              </Button>
            )}
          </>
        ) : (
          <div className="flex flex-col gap-2">
            <div className="flex flex-wrap items-center gap-2">
              {shown === "browser" ? (
                <Button disabled={!ready} onClick={() => onRun(params)}>
                  <PlayIcon aria-hidden="true" />
                  {busy ? copy.running : copy.runHere}
                </Button>
              ) : (
                <Button disabled={!ready} onClick={() => onRunCloud?.(params)}>
                  <CloudIcon aria-hidden="true" />
                  {busy ? copy.running : copy.runCloud}
                </Button>
              )}
              {busy && (
                <Button variant="outline" onClick={onCancel}>
                  <XIcon aria-hidden="true" />
                  {copy.cancel}
                </Button>
              )}
              {shown === "browser" && (
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={busy}
                  onClick={() => setForceDesktop(true)}
                >
                  <MonitorIcon aria-hidden="true" />
                  {copy.switchDesktop}
                </Button>
              )}
            </div>
            {shown === "cloud" && <p className="text-xs text-muted-foreground">{copy.cloudNote}</p>}
            {files.length === 0 && (
              <p className="text-xs text-muted-foreground">{copy.needsFile}</p>
            )}
            {!validation.isValid && files.length > 0 && (
              <p className="text-xs text-muted-foreground">{copy.invalid}</p>
            )}
            {busy && (
              <div role="status" className="flex flex-col gap-1">
                <Progress value={null} aria-label={copy.progress} />
                {progress && <span className="text-xs text-muted-foreground">{progress}</span>}
              </div>
            )}
          </div>
        )}
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {copy.failed(error)}
          </p>
        )}
      </CardContent>
    </Card>
  );
}
