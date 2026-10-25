"use client";

import { Button } from "@ariadshift/ui/components/button";
import { KeyRoundIcon } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useId, useState } from "react";
import { useAccount } from "./account-context";
import { AccountDialog } from "./account-dialog";
import {
  errorMessage,
  formatDate,
  OutcomeMessage,
  Section,
  TextField,
  useOutcome,
} from "./account-ui";
import type { PasskeyInfo } from "./auth-api";

export function PasskeysSection() {
  const { auth } = useAccount();
  const headingId = useId();
  const [passkeys, setPasskeys] = useState<PasskeyInfo[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");
  const [removing, setRemoving] = useState<PasskeyInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const { outcome, succeed } = useOutcome();
  const [dialogError, setDialogError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setPasskeys(await auth.listPasskeys());
      setLoadError(null);
    } catch (error) {
      setLoadError(errorMessage(error, "Could not load your passkeys."));
    }
  }, [auth]);

  useEffect(() => {
    void load();
  }, [load]);

  const onAdd = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setDialogError(null);
    try {
      await auth.addPasskey(name.trim());
      await load();
      setAdding(false);
      setName("");
      succeed("Passkey added.");
    } catch (error) {
      // A canceled browser prompt lands here too; the dialog stays open to try again.
      setDialogError(errorMessage(error, "The passkey was not added."));
    } finally {
      setBusy(false);
    }
  };

  const onRemove = async () => {
    if (!removing) {
      return;
    }
    setBusy(true);
    setDialogError(null);
    try {
      await auth.deletePasskey(removing.id);
      await load();
      const label = removing.name ?? "Passkey";
      setRemoving(null);
      succeed(`${label} removed.`);
    } catch (error) {
      setDialogError(errorMessage(error, "Could not remove the passkey."));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section
      title="Passkeys"
      headingId={headingId}
      description="Sign in with your device's screen lock or a security key instead of a password."
    >
      {loadError ? (
        <OutcomeMessage outcome={{ kind: "error", message: loadError }} />
      ) : passkeys === null ? (
        <p className="text-sm text-muted-foreground">Loading passkeys…</p>
      ) : passkeys.length === 0 ? (
        <p className="text-sm text-muted-foreground">No passkeys yet.</p>
      ) : (
        <ul className="flex flex-col divide-y divide-border" aria-labelledby={headingId}>
          {passkeys.map((passkey) => {
            const label = passkey.name ?? "Unnamed passkey";
            return (
              <li key={passkey.id} className="flex items-center justify-between gap-3 py-2">
                <span className="flex flex-col">
                  <span className="font-medium">{label}</span>
                  <span className="text-xs text-muted-foreground">
                    Added {formatDate(passkey.createdAt)}
                  </span>
                </span>
                <Button
                  variant="destructive"
                  size="sm"
                  onClick={() => {
                    setDialogError(null);
                    setRemoving(passkey);
                  }}
                >
                  Remove<span className="sr-only"> passkey {label}</span>
                </Button>
              </li>
            );
          })}
        </ul>
      )}
      <OutcomeMessage outcome={outcome} />
      <Button
        variant="outline"
        className="self-start"
        onClick={() => {
          setDialogError(null);
          setAdding(true);
        }}
      >
        <KeyRoundIcon aria-hidden="true" />
        Add a passkey
      </Button>

      {adding ? (
        <AccountDialog
          title="Add a passkey"
          description="Name it after the device or key, then follow your browser's prompt."
          onClose={() => setAdding(false)}
        >
          <form onSubmit={onAdd} className="flex flex-col gap-4">
            <TextField
              label="Passkey name"
              data-autofocus
              required
              maxLength={64}
              placeholder="For example: Work laptop"
              value={name}
              onValueChange={setName}
              error={dialogError}
            />
            <div className="flex justify-end gap-2">
              <Button variant="ghost" onClick={() => setAdding(false)}>
                Cancel
              </Button>
              <Button type="submit" disabled={busy}>
                Continue
              </Button>
            </div>
          </form>
        </AccountDialog>
      ) : null}

      {removing ? (
        <AccountDialog
          role="alertdialog"
          title={`Remove ${removing.name ?? "this passkey"}?`}
          description="You can no longer sign in with it. Your other sign-in methods keep working."
          onClose={() => setRemoving(null)}
          returnFocus={() => document.getElementById(headingId)}
        >
          {dialogError ? (
            <OutcomeMessage outcome={{ kind: "error", message: dialogError }} />
          ) : null}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" data-autofocus onClick={() => setRemoving(null)}>
              Cancel
            </Button>
            <Button variant="destructive" disabled={busy} onClick={() => void onRemove()}>
              Remove passkey
            </Button>
          </div>
        </AccountDialog>
      ) : null}
    </Section>
  );
}
