"use client";

import { Button, buttonVariants } from "@ariadshift/ui/components/button";
import { LogOutIcon } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { useAccount } from "./account-context";
import { AccountDialog } from "./account-dialog";
import { errorMessage, OutcomeMessage, Section, TextField } from "./account-ui";
import { AuthError } from "./auth-api";
import { signInHref } from "./session-gate";

export interface AccountActionsSectionProps {
  /** Navigates after signing out or deleting the account; defaults to a full page load. */
  navigate?: (path: string) => void;
}

export function AccountActionsSection({ navigate }: AccountActionsSectionProps) {
  const { auth, refreshSession, announce } = useAccount();
  const [hasPassword, setHasPassword] = useState(true);
  const [deleting, setDeleting] = useState(false);
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reauthRequired, setReauthRequired] = useState(false);
  const go = navigate ?? ((path: string) => window.location.assign(path));

  useEffect(() => {
    let active = true;
    void auth
      .hasPassword()
      .then((value) => {
        if (active) setHasPassword(value);
      })
      .catch(() => {
        // Default to requiring a password if account inspection fails.
      });
    return () => {
      active = false;
    };
  }, [auth]);

  const onSignOut = async () => {
    setBusy(true);
    setError(null);
    try {
      await auth.signOut();
      await refreshSession();
      announce("Signed out.");
      go("/sign-in");
    } catch (failure) {
      setError(errorMessage(failure, "Could not sign out. Try again."));
    } finally {
      setBusy(false);
    }
  };

  const onDelete = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setReauthRequired(false);
    try {
      await auth.deleteAccount(hasPassword ? password : undefined);
      setPassword("");
      setDeleting(false);
      await refreshSession();
      announce("Your account was deleted.");
      go("/");
    } catch (failure) {
      const needsReauth =
        failure instanceof AuthError &&
        (failure.code === "reauth_required" || failure.code === "SESSION_EXPIRED");
      setReauthRequired(needsReauth);
      setError(
        needsReauth
          ? errorMessage(failure, "Sign in again before deleting your account.")
          : errorMessage(
              failure,
              hasPassword
                ? "Could not delete the account. Check your password."
                : "Could not delete the account. Sign in again and retry.",
            ),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section title="Session and account">
      {!deleting ? (
        <OutcomeMessage outcome={error ? { kind: "error", message: error } : null} />
      ) : null}
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" onClick={() => void onSignOut()} disabled={busy}>
          <LogOutIcon aria-hidden="true" />
          Sign out
        </Button>
        <Button
          variant="destructive"
          onClick={() => {
            setError(null);
            setReauthRequired(false);
            setPassword("");
            setDeleting(true);
          }}
        >
          Delete account
        </Button>
      </div>
      <p className="text-sm text-muted-foreground">
        Deleting your account revokes your{" "}
        <a href="/api-keys" className="underline underline-offset-3">
          API keys
        </a>{" "}
        automatically. Before deleting it, delete the{" "}
        <a href="/jobs" className="underline underline-offset-3">
          cloud jobs
        </a>{" "}
        you still have. Files of jobs you leave are removed when their retention period ends.
      </p>

      {deleting ? (
        <AccountDialog
          role="alertdialog"
          title="Delete your account?"
          description={
            hasPassword
              ? "This cannot be undone. Confirm your password to delete the account."
              : "This cannot be undone. Confirm below to delete the account using your current sign-in session."
          }
          onClose={() => setDeleting(false)}
        >
          <form onSubmit={onDelete} className="flex flex-col gap-4">
            {hasPassword ? (
              <TextField
                label="Password"
                type="password"
                autoComplete="current-password"
                data-autofocus
                required
                value={password}
                onValueChange={setPassword}
                error={error}
              />
            ) : error ? (
              <OutcomeMessage outcome={{ kind: "error", message: error }} />
            ) : null}
            <div className="flex justify-end gap-2">
              <Button
                type="button"
                variant="ghost"
                {...(!hasPassword ? { "data-autofocus": true } : {})}
                onClick={() => setDeleting(false)}
              >
                Cancel
              </Button>
              {reauthRequired ? (
                <a href={signInHref("/account")} className={buttonVariants({ variant: "outline" })}>
                  Sign in again
                </a>
              ) : null}
              <Button type="submit" variant="destructive" disabled={busy}>
                Delete account
              </Button>
            </div>
          </form>
        </AccountDialog>
      ) : null}
    </Section>
  );
}
