"use client";

import { Button } from "@ariadshift/ui/components/button";
import { type FormEvent, useState } from "react";
import { useAccount } from "./account-context";
import { errorMessage, OutcomeMessage, Section, TextField, useOutcome } from "./account-ui";
import type { AccountUser } from "./auth-api";

export function ProfileSection({ user }: { user: AccountUser }) {
  const { auth, refreshSession } = useAccount();
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
      succeed("Name saved.");
    } catch (error) {
      fail(errorMessage(error, "Could not save the name."));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section title="Profile">
      <form onSubmit={onSubmit} className="flex flex-col gap-4">
        <TextField label="Name" autoComplete="name" required value={name} onValueChange={setName} />
        <div className="flex flex-col gap-1.5">
          <span className="text-sm font-medium">Email</span>
          <span className="text-sm">{user.email}</span>
        </div>
        <OutcomeMessage outcome={outcome} />
        <Button type="submit" className="self-start" disabled={busy}>
          Save name
        </Button>
      </form>
    </Section>
  );
}
