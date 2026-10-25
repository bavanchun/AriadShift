"use client";

import { Button } from "@ariadshift/ui/components/button";
import { LogOutIcon } from "lucide-react";
import { type FormEvent, useState } from "react";
import { useAccount } from "./account-context";
import { AccountDialog } from "./account-dialog";
import { errorMessage, OutcomeMessage, Section, TextField } from "./account-ui";

export interface AccountActionsSectionProps {
  /** Navigates after signing out or deleting the account; defaults to a full page load. */
  navigate?: (path: string) => void;
}

export function AccountActionsSection({ navigate }: AccountActionsSectionProps) {
  const { auth, refreshSession, announce } = useAccount();
  const [deleting, setDeleting] = useState(false);
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const go = navigate ?? ((path: string) => window.location.assign(path));

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
    try {
      await auth.deleteAccount(password);
      setPassword("");
      setDeleting(false);
      await refreshSession();
      announce("Your account was deleted.");
      go("/");
    } catch (failure) {
      setError(errorMessage(failure, "Could not delete the account. Check your password."));
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
            setPassword("");
            setDeleting(true);
          }}
        >
          Delete account
        </Button>
      </div>
      <p className="text-sm text-muted-foreground">
        Before deleting your account, revoke your{" "}
        <a href="/api-keys" className="underline underline-offset-3">
          API keys
        </a>{" "}
        and delete the{" "}
        <a href="/jobs" className="underline underline-offset-3">
          cloud jobs
        </a>{" "}
        you still have. Files of jobs you leave are removed when their retention period ends.
      </p>

      {deleting ? (
        <AccountDialog
          role="alertdialog"
          title="Delete your account?"
          description="This cannot be undone. Confirm your password to delete the account."
          onClose={() => setDeleting(false)}
        >
          <form onSubmit={onDelete} className="flex flex-col gap-4">
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
            <div className="flex justify-end gap-2">
              <Button variant="ghost" onClick={() => setDeleting(false)}>
                Cancel
              </Button>
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
