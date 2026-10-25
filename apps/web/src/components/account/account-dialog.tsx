"use client";

// A modal dialog on the native <dialog> element: showModal() makes the rest of
// the page inert and closes on Escape. Render it only while it is open; on
// mount it focuses the element marked `data-autofocus` (or the first control),
// and on unmount it returns focus to the control that opened it.

import type { ReactNode } from "react";
import { useEffect, useId, useRef } from "react";

const FOCUSABLE =
  'input:not([disabled]), button:not([disabled]), textarea:not([disabled]), select:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])';

export interface AccountDialogProps {
  title: string;
  description?: ReactNode;
  /** Called on Escape and by the dialog's own close controls. */
  onClose: () => void;
  /** `alertdialog` for confirmations of destructive actions. */
  role?: "dialog" | "alertdialog";
  /** Where focus goes after closing when the opener no longer exists. */
  returnFocus?: () => HTMLElement | null;
  children: ReactNode;
}

export function AccountDialog({
  title,
  description,
  onClose,
  role = "dialog",
  returnFocus,
  children,
}: AccountDialogProps) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const descriptionId = useId();
  // Read the latest callback in the unmount cleanup without re-running the effect.
  const returnFocusRef = useRef(returnFocus);
  returnFocusRef.current = returnFocus;

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) {
      return;
    }
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    if (typeof dialog.showModal === "function") {
      dialog.showModal();
    } else {
      // Environments without the modal API (jsdom) still get an open dialog.
      dialog.setAttribute("open", "");
    }
    const initial =
      dialog.querySelector<HTMLElement>("[data-autofocus]") ??
      dialog.querySelector<HTMLElement>(FOCUSABLE);
    initial?.focus();
    return () => {
      if (dialog.open && typeof dialog.close === "function") {
        dialog.close();
      }
      const target = opener?.isConnected ? opener : returnFocusRef.current?.();
      target?.focus();
    };
  }, []);

  return (
    <dialog
      ref={ref}
      role={role === "alertdialog" ? "alertdialog" : undefined}
      aria-modal="true"
      aria-labelledby={titleId}
      aria-describedby={description ? descriptionId : undefined}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      className="m-auto w-[min(32rem,calc(100vw-2rem))] rounded-xl bg-card p-0 text-card-foreground shadow-lg ring-1 ring-foreground/10 backdrop:bg-black/50"
    >
      <div className="flex flex-col gap-4 p-5">
        <div className="flex flex-col gap-1">
          <h2 id={titleId} className="text-lg font-semibold">
            {title}
          </h2>
          {description ? (
            <div id={descriptionId} className="text-sm text-muted-foreground">
              {description}
            </div>
          ) : null}
        </div>
        {children}
      </div>
    </dialog>
  );
}
