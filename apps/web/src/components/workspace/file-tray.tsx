"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { DropOverlay } from "@ariadshift/ui/components/drop-overlay";
import { FileDropZone } from "@ariadshift/ui/components/file-drop-zone";
import { cn } from "@ariadshift/ui/lib/utils";
import {
  ArrowDownIcon,
  ArrowUpIcon,
  FileTextIcon,
  PlusIcon,
  ShieldCheckIcon,
  XIcon,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { StorageKind } from "@/converter/protocol";
import { formatBytes } from "@/lib/format-bytes";
import { useWorkspaceCopy } from "./copy";
import type { WorkspaceFile } from "./types";

export interface FileTrayProps {
  files: WorkspaceFile[];
  activeId: string | null;
  /** Where the converter keeps the files; shown so "on this device" is concrete. */
  storage?: StorageKind;
  busy?: boolean;
  locale?: string;
  accept?: string;
  onAddFiles: (files: File[]) => void;
  onRemove: (id: string) => void;
  /** Moves a file by one position; tool input order follows the tray order. */
  onMove: (id: string, offset: -1 | 1) => void;
  onSelect: (id: string) => void;
  className?: string;
}

/** True while files are being dragged over the window; drops anywhere add files. */
function useWindowFileDrag(onFiles: (files: File[]) => void, enabled: boolean) {
  const [active, setActive] = useState(false);
  const depth = useRef(0);
  const handler = useRef(onFiles);
  handler.current = onFiles;
  useEffect(() => {
    if (!enabled) return;
    const hasFiles = (event: DragEvent) =>
      Array.from(event.dataTransfer?.types ?? []).includes("Files");
    const enter = (event: DragEvent) => {
      if (!hasFiles(event)) return;
      depth.current += 1;
      setActive(true);
    };
    const leave = (event: DragEvent) => {
      if (!hasFiles(event)) return;
      depth.current = Math.max(0, depth.current - 1);
      if (depth.current === 0) setActive(false);
    };
    const over = (event: DragEvent) => {
      if (hasFiles(event)) event.preventDefault();
    };
    const drop = (event: DragEvent) => {
      if (!hasFiles(event)) return;
      event.preventDefault();
      depth.current = 0;
      setActive(false);
      const files = Array.from(event.dataTransfer?.files ?? []);
      if (files.length) handler.current(files);
    };
    window.addEventListener("dragenter", enter);
    window.addEventListener("dragleave", leave);
    window.addEventListener("dragover", over);
    window.addEventListener("drop", drop);
    return () => {
      window.removeEventListener("dragenter", enter);
      window.removeEventListener("dragleave", leave);
      window.removeEventListener("dragover", over);
      window.removeEventListener("drop", drop);
    };
  }, [enabled]);
  return active;
}

export function FileTray({
  files,
  activeId,
  storage,
  busy = false,
  locale = "en",
  accept,
  onAddFiles,
  onRemove,
  onMove,
  onSelect,
  className,
}: FileTrayProps) {
  const copy = useWorkspaceCopy().fileTray;
  const picker = useRef<HTMLInputElement>(null);
  const dragging = useWindowFileDrag(onAddFiles, !busy);

  return (
    <section
      data-slot="file-tray"
      aria-label={copy.title}
      className={cn("flex flex-col gap-3", className)}
    >
      <DropOverlay
        active={dragging}
        fullscreen
        title={copy.dropTitle}
        description={copy.dropDescription}
        privacyLabel={copy.privacy}
      />
      {files.length === 0 ? (
        <FileDropZone
          onFiles={onAddFiles}
          accept={accept}
          disabled={busy}
          buttonLabel={copy.choose}
          label={copy.title}
        >
          <p className="max-w-sm text-sm text-muted-foreground">{copy.empty}</p>
        </FileDropZone>
      ) : (
        <>
          <ul aria-label={copy.list} className="m-0 flex list-none flex-col gap-2 p-0">
            {files.map((file, index) => (
              <li
                key={file.id}
                data-active={file.id === activeId || undefined}
                className="flex items-center gap-2 rounded-lg border bg-card p-2 data-active:border-primary data-active:ring-1 data-active:ring-primary"
              >
                <FileTextIcon
                  aria-hidden="true"
                  className="size-5 shrink-0 text-muted-foreground"
                />
                <button
                  type="button"
                  aria-label={copy.use(file.name)}
                  aria-current={file.id === activeId ? "true" : undefined}
                  onClick={() => onSelect(file.id)}
                  className="flex min-w-0 flex-1 flex-col items-start text-start outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
                >
                  <span className="w-full truncate text-sm font-medium">{file.name}</span>
                  <span className="text-xs text-muted-foreground tabular-nums">
                    {formatBytes(file.size, locale)}
                    {file.pageCount !== null && ` · ${copy.pages(file.pageCount)}`}
                  </span>
                  {file.problem && (
                    <span role="alert" className="text-xs text-destructive">
                      {file.problem}
                    </span>
                  )}
                </button>
                {file.id === activeId && <Badge variant="secondary">{copy.active}</Badge>}
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={copy.moveUp(file.name)}
                  disabled={busy || index === 0}
                  onClick={() => onMove(file.id, -1)}
                >
                  <ArrowUpIcon aria-hidden="true" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={copy.moveDown(file.name)}
                  disabled={busy || index === files.length - 1}
                  onClick={() => onMove(file.id, 1)}
                >
                  <ArrowDownIcon aria-hidden="true" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={copy.remove(file.name)}
                  disabled={busy}
                  onClick={() => onRemove(file.id)}
                >
                  <XIcon aria-hidden="true" />
                </Button>
              </li>
            ))}
          </ul>
          <input
            ref={picker}
            type="file"
            multiple
            accept={accept}
            className="sr-only"
            tabIndex={-1}
            aria-hidden="true"
            onChange={(event) => {
              const chosen = Array.from(event.currentTarget.files ?? []);
              event.currentTarget.value = "";
              if (chosen.length) onAddFiles(chosen);
            }}
          />
          <Button
            variant="outline"
            size="sm"
            disabled={busy}
            onClick={() => picker.current?.click()}
          >
            <PlusIcon aria-hidden="true" />
            {copy.addMore}
          </Button>
        </>
      )}
      <p
        data-slot="file-tray-privacy"
        className="inline-flex items-center gap-1.5 text-xs font-medium text-success"
      >
        <ShieldCheckIcon aria-hidden="true" className="size-3.5" />
        {copy.privacy}
        {storage && (
          <span className="font-normal text-muted-foreground">· {copy.storage(storage)}</span>
        )}
      </p>
      {busy && (
        <p role="status" className="text-xs text-muted-foreground">
          {copy.reading}
        </p>
      )}
    </section>
  );
}
