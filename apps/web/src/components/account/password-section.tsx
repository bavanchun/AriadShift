"use client";

import { Button } from "@ariadshift/ui/components/button";
import { type FormEvent, useState } from "react";
import { useAccount } from "./account-context";
import {
  errorMessage,
  MIN_PASSWORD_LENGTH,
  OutcomeMessage,
  Section,
  TextField,
  useAccountText,
  useOutcome,
} from "./account-ui";

export function PasswordSection() {
  const { auth } = useAccount();
  const { t } = useAccountText();
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [revokeOthers, setRevokeOthers] = useState(true);
  const [confirmError, setConfirmError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const { outcome, succeed, fail, clear } = useOutcome();

  const onSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (next !== confirm) {
      setConfirmError(t("password.mismatch"));
      return;
    }
    setConfirmError(null);
    setBusy(true);
    clear();
    try {
      await auth.changePassword(current, next, revokeOthers);
      setCurrent("");
      setNext("");
      setConfirm("");
      succeed(t(revokeOthers ? "password.changed_revoked" : "password.changed"));
    } catch (error) {
      fail(errorMessage(t, error, "password.failed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section title={t("password.title")}>
      <form onSubmit={onSubmit} className="flex flex-col gap-4">
        <TextField
          label={t("password.current")}
          type="password"
          autoComplete="current-password"
          required
          value={current}
          onValueChange={setCurrent}
        />
        <TextField
          label={t("password.new")}
          type="password"
          autoComplete="new-password"
          required
          minLength={MIN_PASSWORD_LENGTH}
          hint={t("fields.password_hint", { min: MIN_PASSWORD_LENGTH })}
          value={next}
          onValueChange={setNext}
        />
        <TextField
          label={t("password.confirm")}
          type="password"
          autoComplete="new-password"
          required
          value={confirm}
          onValueChange={setConfirm}
          error={confirmError}
        />
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={revokeOthers}
            onChange={(event) => setRevokeOthers(event.target.checked)}
          />
          {t("password.revoke_others")}
        </label>
        <OutcomeMessage outcome={outcome} />
        <Button type="submit" className="self-start" disabled={busy}>
          {t("password.submit")}
        </Button>
      </form>
    </Section>
  );
}
