"use client";

import { Button, buttonVariants } from "@ariadshift/ui/components/button";
import { LogOutIcon } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { useLocalePath } from "@/i18n/use-locale-path";
import { useAccount } from "./account-context";
import { AccountDialog } from "./account-dialog";
import { errorMessage, OutcomeMessage, Section, TextField, useAccountText } from "./account-ui";
import { AuthError } from "./auth-api";
import { useSignInHref } from "./session-gate";

export interface AccountActionsSectionProps {
  /** Navigates after signing out or deleting the account; defaults to a full page load. */
  navigate?: (path: string) => void;
}

export function AccountActionsSection({ navigate }: AccountActionsSectionProps) {
  const { auth, refreshSession, announce } = useAccount();
  const { t } = useAccountText();
  const toLocale = useLocalePath();
  const toSignIn = useSignInHref();
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
      announce(t("account_actions.signed_out"));
      go(toLocale("/sign-in"));
    } catch (failure) {
      setError(errorMessage(t, failure, "account_actions.sign_out_failed"));
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
      announce(t("account_actions.deleted"));
      go(toLocale("/"));
    } catch (failure) {
      const needsReauth =
        failure instanceof AuthError &&
        (failure.code === "reauth_required" || failure.code === "SESSION_EXPIRED");
      setReauthRequired(needsReauth);
      setError(
        errorMessage(
          t,
          failure,
          needsReauth
            ? "errors.reauth_required"
            : hasPassword
              ? "account_actions.delete_failed_password"
              : "account_actions.delete_failed_session",
        ),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section title={t("account_actions.title")}>
      {!deleting ? (
        <OutcomeMessage outcome={error ? { kind: "error", message: error } : null} />
      ) : null}
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" onClick={() => void onSignOut()} disabled={busy}>
          <LogOutIcon aria-hidden="true" />
          {t("account_actions.sign_out")}
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
          {t("account_actions.delete")}
        </Button>
      </div>
      <p className="text-sm text-muted-foreground">
        {t.rich("account_actions.delete_note", {
          keys: (chunks) => (
            <a href={toLocale("/api-keys")} className="underline underline-offset-3">
              {chunks}
            </a>
          ),
          jobs: (chunks) => (
            <a href={toLocale("/jobs")} className="underline underline-offset-3">
              {chunks}
            </a>
          ),
        })}
      </p>

      {deleting ? (
        <AccountDialog
          role="alertdialog"
          title={t("account_actions.dialog_title")}
          description={t(
            hasPassword
              ? "account_actions.dialog_description_password"
              : "account_actions.dialog_description_session",
          )}
          onClose={() => setDeleting(false)}
        >
          <form onSubmit={onDelete} className="flex flex-col gap-4">
            {hasPassword ? (
              <TextField
                label={t("fields.password")}
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
                {t("actions.cancel")}
              </Button>
              {reauthRequired ? (
                <a href={toSignIn("/account")} className={buttonVariants({ variant: "outline" })}>
                  {t("account_actions.sign_in_again")}
                </a>
              ) : null}
              <Button type="submit" variant="destructive" disabled={busy}>
                {t("account_actions.delete")}
              </Button>
            </div>
          </form>
        </AccountDialog>
      ) : null}
    </Section>
  );
}
