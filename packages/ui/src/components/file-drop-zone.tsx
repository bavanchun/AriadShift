"use client";

import { UploadIcon } from "lucide-react";
import { type DragEvent, type ReactNode, useId, useRef, useState } from "react";
import { Button } from "#components/button";
import { cn } from "#lib/utils";

export interface FileDropZoneProps {
  /** Called with the dropped or picked files; the files are passed on, never read here. */
  onFiles: (files: File[]) => void;
  /** Value for the file input's `accept` attribute. */
  accept?: string;
  disabled?: boolean;
  /** Label of the picker button. */
  buttonLabel?: string;
  /** Text shown above the button. */
  children?: ReactNode;
  /** Accessible name of the drop area. */
  label?: string;
  className?: string;
}

/** A drop target with a keyboard-operable file picker. */
export function FileDropZone({
  onFiles,
  accept,
  disabled = false,
  buttonLabel = "Choose a file",
  children,
  label = "File drop area",
  className,
}: FileDropZoneProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const inputId = useId();
  const [dragging, setDragging] = useState(false);

  const accepts = (event: DragEvent) =>
    !disabled && Array.from(event.dataTransfer.types).includes("Files");

  return (
    <fieldset
      data-slot="file-drop-zone"
      data-dragging={dragging || undefined}
      className={cn(
        "m-0 flex min-w-0 flex-col items-center justify-center gap-4 rounded-xl border-2 border-dashed border-input bg-card p-8 text-center transition-colors",
        "data-dragging:border-primary data-dragging:bg-secondary",
        disabled && "opacity-60",
        className,
      )}
      onDragEnter={(event) => {
        if (accepts(event)) {
          event.preventDefault();
          setDragging(true);
        }
      }}
      onDragOver={(event) => {
        if (accepts(event)) {
          event.preventDefault();
          event.dataTransfer.dropEffect = "copy";
        }
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
          setDragging(false);
        }
      }}
      onDrop={(event) => {
        event.preventDefault();
        setDragging(false);
        if (!disabled && event.dataTransfer.files.length > 0) {
          onFiles(Array.from(event.dataTransfer.files));
        }
      }}
    >
      <legend className="sr-only">{label}</legend>
      <UploadIcon aria-hidden="true" className="size-8 text-muted-foreground" />
      {children}
      <input
        ref={inputRef}
        id={inputId}
        type="file"
        accept={accept}
        disabled={disabled}
        className="sr-only"
        tabIndex={-1}
        aria-hidden="true"
        onChange={(event) => {
          const files = Array.from(event.currentTarget.files ?? []);
          event.currentTarget.value = "";
          if (files.length > 0) {
            onFiles(files);
          }
        }}
      />
      <Button size="lg" disabled={disabled} onClick={() => inputRef.current?.click()}>
        {buttonLabel}
      </Button>
    </fieldset>
  );
}
