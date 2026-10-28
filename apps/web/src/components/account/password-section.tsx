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
  useOutcome,
} from "./account-ui";

export function PasswordSection() {
  const { auth } = useAccount();
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
      setConfirmError("The new passwords do not match.");
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
      succeed(
        revokeOthers ? "Password changed. Other sessions were signed out." : "Password changed.",
      );
    } catch (error) {
      fail(errorMessage(error, "Could not change the password."));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section title="Password">
      <form onSubmit={onSubmit} className="flex flex-col gap-4">
        <TextField
          label="Current password"
          type="password"
          autoComplete="current-password"
          required
          value={current}
          onValueChange={setCurrent}
        />
        <TextField
          label="New password"
          type="password"
          autoComplete="new-password"
          required
          minLength={MIN_PASSWORD_LENGTH}
          hint={`At least ${MIN_PASSWORD_LENGTH} characters.`}
          value={next}
          onValueChange={setNext}
        />
        <TextField
          label="Confirm new password"
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
          Sign out of other sessions
        </label>
        <OutcomeMessage outcome={outcome} />
        <Button type="submit" className="self-start" disabled={busy}>
          Change password
        </Button>
      </form>
    </Section>
  );
}
