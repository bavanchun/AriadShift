"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@ariadshift/ui/components/dialog";
import { useMemo, useState } from "react";
import { type DocumentModeLabels, fillLabel } from "./document-mode-labels";
import type { DocumentEditSession, ExportBundle } from "./document-session";

export type ExportFormat = "pdf" | "docx" | "md" | "html";

export const EXPORT_FORMATS: readonly ExportFormat[] = ["pdf", "docx", "md", "html"];

/** What the host receives on confirm: the edited IR, ready to convert to `format`. */
export interface ExportRequest extends ExportBundle {
  format: ExportFormat;
}

export interface ExportFidelityDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  session: DocumentEditSession;
  /** Session revision; recomputes the summary when the document changed. */
  revision: number;
  labels: DocumentModeLabels;
  defaultFormat?: ExportFormat;
  onExport: (request: ExportRequest) => void | Promise<void>;
}

const MAX_LISTED_CHANGES = 50;

const formatLabel = (labels: DocumentModeLabels, format: ExportFormat) =>
  ({
    pdf: labels.exportPdf,
    docx: labels.exportDocx,
    md: labels.exportMarkdown,
    html: labels.exportHtml,
  })[format];

type BundleResult = { bundle: ExportBundle } | { error: string };

function ExportBody({
  session,
  revision,
  labels,
  defaultFormat,
  onExport,
  onClose,
}: Omit<ExportFidelityDialogProps, "open" | "onOpenChange"> & { onClose: () => void }) {
  const [format, setFormat] = useState<ExportFormat>(defaultFormat ?? "pdf");
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const result = useMemo<BundleResult>(() => {
    try {
      return { bundle: session.buildExport() };
    } catch (reason) {
      return { error: reason instanceof Error ? reason.message : String(reason) };
    }
  }, [session, revision]);

  const confirm = async () => {
    if ("error" in result) return;
    setBusy(true);
    setFailure(null);
    try {
      await onExport({ format, ...result.bundle });
      onClose();
    } catch (reason) {
      setFailure(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const summary = "bundle" in result ? result.bundle.summary : null;
  const percent = summary ? Math.round(summary.estimatedFidelityScore * 100) : 0;
  const counts: [string, number][] = summary
    ? [
        [labels.addedBlocks, summary.addedBlocks.length],
        [labels.modifiedBlocks, summary.modifiedBlocks.length],
        [labels.deletedBlocks, summary.deletedBlocks.length],
        [labels.styleChanges, summary.styleChanges.length],
      ]
    : [];
  const kindLabel = {
    added: labels.addedBlocks,
    modified: labels.modifiedBlocks,
    deleted: labels.deletedBlocks,
    style: labels.styleChanges,
    document: "",
  };
  const items = summary?.changeItems ?? [];

  return (
    <>
      <DialogHeader>
        <DialogTitle>{labels.exportTitle}</DialogTitle>
        <DialogDescription>{labels.exportDescription}</DialogDescription>
      </DialogHeader>

      {"error" in result ? (
        <p role="alert" className="text-sm text-destructive">
          {fillLabel(labels.exportFailed, { reason: result.error })}
        </p>
      ) : (
        <>
          <div className="flex items-center gap-3">
            <span className="text-sm font-medium">{labels.fidelityScore}</span>
            <Badge
              data-testid="fidelity-score"
              aria-label={`${labels.fidelityScore}: ${percent}%`}
              className="text-sm"
            >
              {percent}%
            </Badge>
          </div>
          <p className="text-xs text-muted-foreground">{labels.fidelityNote}</p>

          <section aria-label={labels.whatChanged} className="flex flex-col gap-2">
            <h3 className="text-sm font-semibold">{labels.whatChanged}</h3>
            <dl className="grid grid-cols-2 gap-x-4 gap-y-1 text-sm">
              {counts.map(([label, count]) => (
                <div key={label} className="flex justify-between gap-2">
                  <dt className="text-muted-foreground">{label}</dt>
                  <dd className="font-medium" data-testid={`count-${label}`}>
                    {count}
                  </dd>
                </div>
              ))}
            </dl>
            {items.length === 0 ? (
              <p className="text-sm text-muted-foreground">{labels.noChanges}</p>
            ) : (
              <ul className="max-h-40 overflow-y-auto text-xs text-muted-foreground">
                {items.slice(0, MAX_LISTED_CHANGES).map((item) => (
                  <li key={`${item.kind}:${item.blockId ?? item.message}`}>
                    {item.blockId ? `${kindLabel[item.kind]} · ${item.blockId}` : item.message}
                  </li>
                ))}
                {items.length > MAX_LISTED_CHANGES && (
                  <li>
                    {fillLabel(labels.moreChanges, { count: items.length - MAX_LISTED_CHANGES })}
                  </li>
                )}
              </ul>
            )}
          </section>

          <fieldset className="flex flex-wrap gap-3">
            <legend className="mb-1 text-sm font-medium">{labels.exportFormat}</legend>
            {EXPORT_FORMATS.map((value) => (
              <label key={value} className="flex items-center gap-1.5 text-sm">
                <input
                  type="radio"
                  name="export-format"
                  value={value}
                  checked={format === value}
                  onChange={() => setFormat(value)}
                />
                {formatLabel(labels, value)}
              </label>
            ))}
          </fieldset>
        </>
      )}

      {failure && (
        <p role="alert" className="text-sm text-destructive">
          {fillLabel(labels.exportFailed, { reason: failure })}
        </p>
      )}

      <DialogFooter>
        <Button type="button" variant="outline" onClick={onClose}>
          {labels.cancel}
        </Button>
        <Button type="button" disabled={"error" in result || busy} onClick={() => void confirm()}>
          {labels.confirmExport}
        </Button>
      </DialogFooter>
    </>
  );
}

/** Shows the fidelity estimate and the change list before the edited IR is handed to export. */
export function ExportFidelityDialog(props: ExportFidelityDialogProps) {
  const { open, onOpenChange, labels } = props;
  return (
    <Dialog open={open} onOpenChange={(next) => onOpenChange(next)}>
      <DialogContent closeLabel={labels.close}>
        {open && <ExportBody {...props} onClose={() => onOpenChange(false)} />}
      </DialogContent>
    </Dialog>
  );
}
