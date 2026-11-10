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
  useAccountText,
  useOutcome,
} from "./account-ui";
import type { PasskeyInfo } from "./auth-api";

export function PasskeysSection() {
  const { auth } = useAccount();
  const { t, locale } = useAccountText();
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
      setLoadError(errorMessage(t, error, "passkeys.load_failed"));
    }
  }, [auth, t]);

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
      succeed(t("passkeys.added"));
    } catch (error) {
      // A canceled browser prompt lands here too; the dialog stays open to try again.
      setDialogError(errorMessage(t, error, "passkeys.add_failed"));
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
      const label = removing.name ?? t("passkeys.default_name");
      setRemoving(null);
      succeed(t("passkeys.removed", { name: label }));
    } catch (error) {
      setDialogError(errorMessage(t, error, "passkeys.remove_failed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section
      title={t("passkeys.title")}
      headingId={headingId}
      description={t("passkeys.description")}
    >
      {loadError ? (
        <OutcomeMessage outcome={{ kind: "error", message: loadError }} />
      ) : passkeys === null ? (
        <p className="text-sm text-muted-foreground">{t("passkeys.loading")}</p>
      ) : passkeys.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("passkeys.empty")}</p>
      ) : (
        <ul className="flex flex-col divide-y divide-border" aria-labelledby={headingId}>
          {passkeys.map((passkey) => {
            const label = passkey.name ?? t("passkeys.unnamed");
            return (
              <li key={passkey.id} className="flex items-center justify-between gap-3 py-2">
                <span className="flex flex-col">
                  <span className="font-medium">{label}</span>
                  <span className="text-xs text-muted-foreground">
                    {t("passkeys.added_on", { date: formatDate(passkey.createdAt, t, locale) })}
                  </span>
                </span>
                <Button
                  variant="destructive"
                  size="sm"
                  aria-label={t("passkeys.remove_label", { name: label })}
                  onClick={() => {
                    setDialogError(null);
                    setRemoving(passkey);
                  }}
                >
                  {t.rich("passkeys.remove_button", {
                    name: label,
                    hidden: (chunks) => <span className="sr-only">{chunks}</span>,
                  })}
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
        {t("passkeys.add_button")}
      </Button>

      {adding ? (
        <AccountDialog
          title={t("passkeys.add_title")}
          description={t("passkeys.add_description")}
          onClose={() => setAdding(false)}
        >
          <form onSubmit={onAdd} className="flex flex-col gap-4">
            <TextField
              label={t("passkeys.name_label")}
              data-autofocus
              required
              maxLength={64}
              placeholder={t("passkeys.name_placeholder")}
              value={name}
              onValueChange={setName}
              error={dialogError}
            />
            <div className="flex justify-end gap-2">
              <Button type="button" variant="ghost" onClick={() => setAdding(false)}>
                {t("actions.cancel")}
              </Button>
              <Button type="submit" disabled={busy}>
                {t("actions.continue")}
              </Button>
            </div>
          </form>
        </AccountDialog>
      ) : null}

      {removing ? (
        <AccountDialog
          role="alertdialog"
          title={
            removing.name
              ? t("passkeys.remove_title", { name: removing.name })
              : t("passkeys.remove_title_unnamed")
          }
          description={t("passkeys.remove_description")}
          onClose={() => setRemoving(null)}
          returnFocus={() => document.getElementById(headingId)}
        >
          {dialogError ? (
            <OutcomeMessage outcome={{ kind: "error", message: dialogError }} />
          ) : null}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" data-autofocus onClick={() => setRemoving(null)}>
              {t("actions.cancel")}
            </Button>
            <Button variant="destructive" disabled={busy} onClick={() => void onRemove()}>
              {t("passkeys.remove_confirm")}
            </Button>
          </div>
        </AccountDialog>
      ) : null}
    </Section>
  );
}
