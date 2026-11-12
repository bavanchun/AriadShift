"use client";

import { Button } from "@ariadshift/ui/components/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@ariadshift/ui/components/dialog";
import { CloudIcon, ShieldCheckIcon } from "lucide-react";
import { useId, useState } from "react";
import { type AiPanelLabels, resolveAiLabels } from "./ai-labels";

export interface CloudConsent {
  /** Always `true`: the dialog only reports an explicit, checked confirmation. */
  consent: true;
  /** Optional bring-your-own key. Held in memory by the caller, never persisted. */
  apiKey?: string;
}

export interface CloudConsentDialogProps {
  open: boolean;
  /** Name of the operation that would run, shown under "what leaves your device". */
  operation: string;
  onConfirm: (consent: CloudConsent) => void;
  /** Cancel button, Escape, close button and backdrop all land here. */
  onCancel: () => void;
  labels?: Partial<AiPanelLabels["consent"]>;
}

function ConsentForm({
  operation,
  onConfirm,
  onCancel,
  labels,
}: Omit<CloudConsentDialogProps, "open" | "labels"> & { labels: AiPanelLabels["consent"] }) {
  const [agreed, setAgreed] = useState(false);
  const [apiKey, setApiKey] = useState("");
  const checkId = useId();
  const keyId = useId();
  const hintId = useId();

  return (
    <form
      className="flex flex-col gap-4"
      onSubmit={(event) => {
        event.preventDefault();
        if (!agreed) return;
        const key = apiKey.trim();
        onConfirm({ consent: true, ...(key && { apiKey: key }) });
      }}
    >
      <DialogHeader>
        <DialogTitle className="flex items-center gap-2">
          <CloudIcon aria-hidden="true" className="size-4" />
          {labels.title}
        </DialogTitle>
        <DialogDescription>{labels.description}</DialogDescription>
      </DialogHeader>

      <section aria-label={labels.leaves} className="rounded-lg border bg-muted/40 p-3 text-sm">
        <p className="m-0 mb-1 font-medium">{labels.leaves}</p>
        <ul className="m-0 flex list-disc flex-col gap-0.5 ps-5 text-muted-foreground">
          <li>{labels.leavesDocument}</li>
          <li>
            {labels.leavesOperation}: <strong className="text-foreground">{operation}</strong>
          </li>
          <li>{labels.leavesProvider}</li>
        </ul>
        <p className="m-0 mt-2 flex items-start gap-1.5 text-xs text-muted-foreground">
          <ShieldCheckIcon aria-hidden="true" className="mt-0.5 size-3.5 shrink-0" />
          {labels.stays}
        </p>
      </section>

      <div className="flex items-start gap-2">
        <input
          id={checkId}
          type="checkbox"
          checked={agreed}
          onChange={(event) => setAgreed(event.target.checked)}
          className="mt-0.5 size-4 shrink-0 accent-primary"
        />
        <label htmlFor={checkId} className="text-sm">
          {labels.checkbox}
        </label>
      </div>

      <div className="flex flex-col gap-1">
        <label htmlFor={keyId} className="text-sm font-medium">
          {labels.apiKey}
        </label>
        <input
          id={keyId}
          type="password"
          autoComplete="off"
          spellCheck={false}
          autoCapitalize="off"
          value={apiKey}
          onChange={(event) => setApiKey(event.target.value)}
          aria-describedby={hintId}
          className="h-8 rounded-lg border bg-background px-2.5 text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
        />
        <span id={hintId} className="text-xs text-muted-foreground">
          {labels.apiKeyHint}
        </span>
      </div>

      <DialogFooter>
        <Button type="button" variant="outline" onClick={onCancel}>
          {labels.cancel}
        </Button>
        <Button type="submit" disabled={!agreed}>
          {labels.confirm}
        </Button>
      </DialogFooter>
    </form>
  );
}

/**
 * The gate in front of every Cloud AI request. Without an explicit, checked confirmation nothing
 * is reported through `onConfirm`, so no caller can obtain `cloud_consent: true` from it.
 */
export function CloudConsentDialog({
  open,
  operation,
  onConfirm,
  onCancel,
  labels,
}: CloudConsentDialogProps) {
  const text = resolveAiLabels({ consent: labels as AiPanelLabels["consent"] }).consent;
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onCancel()}>
      <DialogContent showClose closeLabel={text.close} data-slot="cloud-consent-dialog">
        {/* Mounted only while open, so the typed key is discarded on every dismissal. */}
        <ConsentForm
          operation={operation}
          onConfirm={onConfirm}
          onCancel={onCancel}
          labels={text}
        />
      </DialogContent>
    </Dialog>
  );
}
