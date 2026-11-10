"use client";

import { Alert, AlertDescription, AlertTitle } from "@ariadshift/ui/components/alert";
import { Button } from "@ariadshift/ui/components/button";
import { useToast } from "@ariadshift/ui/components/toast";
import { CopyIcon, DownloadIcon, EyeIcon, EyeOffIcon, MonitorIcon } from "lucide-react";
import { useState } from "react";
import { type ToolHandoff, toolHandoff } from "@/converter/handoff";
import type { ToolDescriptor } from "@/converter/tool-catalog";
import { useWorkspaceCopy } from "./copy";

export interface HandoffPanelProps {
  tool: ToolDescriptor;
  params: Record<string, unknown>;
  inputNames: string[];
  locale?: string;
  /** Overrides how the command is copied; defaults to the async clipboard. */
  copyText?: (text: string) => Promise<void>;
  className?: string;
}

/** The honest answer for a tool the browser cannot run: the exact command and where to get it. */
export function HandoffPanel({
  tool,
  params,
  inputNames,
  locale,
  copyText = (text) => navigator.clipboard.writeText(text),
  className,
}: HandoffPanelProps) {
  const copy = useWorkspaceCopy().handoff;
  const { toast } = useToast();
  const [reveal, setReveal] = useState(false);
  const hasSecrets = tool.secret_params.some((key) => key in params);
  const handoff: ToolHandoff = toolHandoff(tool, params, inputNames, {
    locale,
    revealSecrets: reveal,
  });

  async function copyCommand() {
    try {
      await copyText(handoff.cliCommand);
      toast({ variant: "success", title: copy.copied });
    } catch {
      toast({ variant: "warning", title: copy.copyFailed });
    }
  }

  return (
    <Alert data-slot="handoff-panel" className={className}>
      <MonitorIcon aria-hidden="true" />
      <AlertTitle>{copy.title}</AlertTitle>
      <AlertDescription className="flex flex-col gap-3">
        <p>{copy.why}</p>
        <div className="flex flex-col gap-1">
          <span className="text-xs font-medium">{copy.command}</span>
          <pre className="m-0 overflow-x-auto rounded-lg border bg-muted p-2 text-xs whitespace-pre-wrap break-all">
            <code>{handoff.cliCommand}</code>
          </pre>
        </div>
        {hasSecrets && <p className="text-xs text-muted-foreground">{copy.secretNote}</p>}
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" onClick={copyCommand}>
            <CopyIcon aria-hidden="true" />
            {copy.copy}
          </Button>
          {hasSecrets && (
            <Button
              variant="outline"
              size="sm"
              aria-pressed={reveal}
              onClick={() => setReveal(!reveal)}
            >
              {reveal ? <EyeOffIcon aria-hidden="true" /> : <EyeIcon aria-hidden="true" />}
              {reveal ? copy.maskSecrets : copy.revealSecrets}
            </Button>
          )}
          <a
            href={handoff.installUrl}
            className="inline-flex h-7 items-center gap-1 rounded-lg border bg-background px-2.5 text-[0.8rem] font-medium hover:bg-muted"
          >
            <DownloadIcon aria-hidden="true" className="size-3.5" />
            {copy.install}
          </a>
          <a href={handoff.deepLinkUrl} className="text-sm underline underline-offset-4">
            {copy.openApp}
          </a>
        </div>
      </AlertDescription>
    </Alert>
  );
}
