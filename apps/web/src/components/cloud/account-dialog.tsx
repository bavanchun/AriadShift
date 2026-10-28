"use client";

import { Button } from "@ariadshift/ui/components/button";
import { LoaderCircleIcon } from "lucide-react";
import { type FormEvent, useEffect, useId, useRef, useState } from "react";
import { type AccountUser, AuthError, type CloudAuth } from "@/lib/cloud/auth";

type Mode = "sign-in" | "sign-up" | "two-factor";

export interface AccountDialogProps {
  auth: CloudAuth;
  open: boolean;
  onClose: () => void;
  onSignedIn: (user: AccountUser) => void;
}

const FIELD =
  "h-11 w-full rounded-lg border border-input bg-background px-3 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50";

/** A minimal sign-in and sign-up dialog for cloud mode; account pages live elsewhere. */
export function AccountDialog({ auth, open, onClose, onSignedIn }: AccountDialogProps) {
  const dialog = useRef<HTMLDialogElement>(null);
  const attemptRef = useRef(0);
  const [mode, setMode] = useState<Mode>("sign-in");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [backup, setBackup] = useState(false);
  const titleId = useId();
  const ids = { name: useId(), email: useId(), password: useId(), code: useId() };

  // Focus goes back to whatever opened the dialog, when it still exists.
  useEffect(() => {
    const opener = document.activeElement;
    return () => {
      attemptRef.current += 1;
      if (opener instanceof HTMLElement && opener.isConnected) {
        opener.focus();
      }
    };
  }, []);

  useEffect(() => {
    const element = dialog.current;
    if (!element) {
      return;
    }
    if (open && !element.open) {
      setMode("sign-in");
      setProblem(null);
      // jsdom has no showModal; the open attribute keeps the form usable there.
      if (typeof element.showModal === "function") {
        element.showModal();
      } else {
        element.setAttribute("open", "");
      }
    } else if (!open && element.open) {
      element.close();
    }
  }, [open]);

  const cancel = () => {
    attemptRef.current += 1;
    auth.cancelPendingAuth();
    onClose();
  };

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const value = (key: string) => String(form.get(key) ?? "").trim();
    const attempt = ++attemptRef.current;
    setBusy(true);
    setProblem(null);
    try {
      if (mode === "two-factor") {
        const user = await auth.verifyCode(value("code"), backup ? "backup" : "totp");
        if (attemptRef.current !== attempt || auth.user?.id !== user.id) {
          return;
        }
        onSignedIn(user);
      } else if (mode === "sign-up") {
        const user = await auth.signUp(value("name"), value("email"), String(form.get("password")));
        if (attemptRef.current !== attempt || auth.user?.id !== user.id) {
          return;
        }
        onSignedIn(user);
      } else {
        const result = await auth.signIn(value("email"), String(form.get("password")));
        if (attemptRef.current !== attempt) {
          return;
        }
        if (result.status === "two-factor") {
          setMode("two-factor");
          return;
        }
        if (auth.user?.id !== result.user.id) {
          return;
        }
        onSignedIn(result.user);
      }
    } catch (error) {
      if (attemptRef.current !== attempt) {
        return;
      }
      setProblem(
        error instanceof AuthError
          ? error.message
          : "The account service cannot be reached. Check your connection and try again.",
      );
    } finally {
      if (attemptRef.current === attempt) {
        setBusy(false);
      }
    }
  };

  const title =
    mode === "sign-up"
      ? "Create an account"
      : mode === "two-factor"
        ? "Enter your code"
        : "Sign in";

  return (
    <dialog
      ref={dialog}
      aria-labelledby={titleId}
      className="m-auto w-[min(28rem,calc(100%-2rem))] rounded-xl border border-border bg-card p-6 text-card-foreground shadow-lg backdrop:bg-black/40"
      onClose={cancel}
      data-testid="account-dialog"
    >
      <form className="flex flex-col gap-4" onSubmit={submit}>
        <h2 id={titleId} className="text-lg font-semibold">
          {title}
        </h2>
        <p className="text-sm text-muted-foreground">
          {mode === "two-factor"
            ? backup
              ? "Enter one of your backup codes."
              : "Enter the six-digit code from your authenticator app."
            : "An account raises the cloud limits. Converting on this device never needs one."}
        </p>
        {mode === "sign-up" && (
          <div className="flex flex-col gap-1.5">
            <label htmlFor={ids.name} className="text-sm font-medium">
              Name
            </label>
            <input id={ids.name} name="name" autoComplete="name" required className={FIELD} />
          </div>
        )}
        {mode !== "two-factor" ? (
          <>
            <div className="flex flex-col gap-1.5">
              <label htmlFor={ids.email} className="text-sm font-medium">
                Email
              </label>
              <input
                id={ids.email}
                name="email"
                type="email"
                autoComplete="email"
                required
                className={FIELD}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <label htmlFor={ids.password} className="text-sm font-medium">
                Password
              </label>
              <input
                id={ids.password}
                name="password"
                type="password"
                autoComplete={mode === "sign-up" ? "new-password" : "current-password"}
                minLength={mode === "sign-up" ? 8 : undefined}
                required
                className={FIELD}
              />
            </div>
          </>
        ) : (
          <div className="flex flex-col gap-1.5">
            <label htmlFor={ids.code} className="text-sm font-medium">
              {backup ? "Backup code" : "Authentication code"}
            </label>
            <input
              id={ids.code}
              name="code"
              autoComplete="one-time-code"
              inputMode={backup ? "text" : "numeric"}
              required
              className={FIELD}
            />
          </div>
        )}
        {problem && (
          <p className="text-sm text-destructive" role="alert">
            {problem}
          </p>
        )}
        <div className="flex flex-wrap items-center gap-3">
          <Button type="submit" size="lg" disabled={busy}>
            {busy && <LoaderCircleIcon aria-hidden="true" className="animate-spin" />}
            {mode === "sign-up" ? "Create account" : mode === "two-factor" ? "Verify" : "Sign in"}
          </Button>
          <Button type="button" size="lg" variant="outline" onClick={cancel}>
            Cancel
          </Button>
        </div>
        {mode === "two-factor" ? (
          <button
            type="button"
            className="self-start text-sm underline"
            onClick={() => setBackup((value) => !value)}
          >
            {backup ? "Use the authenticator app instead" : "Use a backup code instead"}
          </button>
        ) : (
          <button
            type="button"
            className="self-start text-sm underline"
            onClick={() => {
              setProblem(null);
              setMode(mode === "sign-up" ? "sign-in" : "sign-up");
            }}
          >
            {mode === "sign-up" ? "I already have an account" : "Create an account instead"}
          </button>
        )}
      </form>
    </dialog>
  );
}
