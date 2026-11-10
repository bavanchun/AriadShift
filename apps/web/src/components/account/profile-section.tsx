"use client";

import { Button } from "@ariadshift/ui/components/button";
import { type FormEvent, useState } from "react";
import { useAccount } from "./account-context";
import {
  errorMessage,
  OutcomeMessage,
  Section,
  TextField,
  useAccountText,
  useOutcome,
} from "./account-ui";
import type { AccountUser } from "./auth-api";

export function ProfileSection({ user }: { user: AccountUser }) {
  const { auth, refreshSession } = useAccount();
  const { t } = useAccountText();
  const [name, setName] = useState(user.name);
  const [busy, setBusy] = useState(false);
  const { outcome, succeed, fail, clear } = useOutcome();

  const onSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    clear();
    try {
      await auth.updateName(name.trim());
      await refreshSession();
      succeed(t("profile.saved"));
    } catch (error) {
      fail(errorMessage(t, error, "profile.save_failed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section title={t("profile.title")}>
      <form onSubmit={onSubmit} className="flex flex-col gap-4">
        <TextField
          label={t("fields.name")}
          autoComplete="name"
          required
          value={name}
          onValueChange={setName}
        />
        <div className="flex flex-col gap-1.5">
          <span className="text-sm font-medium">{t("fields.email")}</span>
          <span className="text-sm">{user.email}</span>
        </div>
        <OutcomeMessage outcome={outcome} />
        <Button type="submit" className="self-start" disabled={busy}>
          {t("profile.save")}
        </Button>
      </form>
    </Section>
  );
}
