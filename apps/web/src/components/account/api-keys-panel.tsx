"use client";

// API keys: list, create (the key is shown once) and revoke, through the auth
// service. The API accepts a key in the `x-api-key` header.

import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { PlusIcon } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useId, useState } from "react";
import { useAccount } from "./account-context";
import { AccountDialog } from "./account-dialog";
import {
  errorMessage,
  formatDate,
  INPUT_CLASS,
  OutcomeMessage,
  Section,
  TextField,
  useOutcome,
} from "./account-ui";
import type { ApiKeyInfo, CreatedApiKey } from "./auth-api";
import { displayHost } from "./cloud-config";
import { SessionGate } from "./session-gate";

/** Lifetimes offered when creating a key, in days; null never expires. */
export const KEY_LIFETIMES: ReadonlyArray<{ days: number | null; label: string }> = [
  { days: 30, label: "30 days" },
  { days: 90, label: "90 days" },
  { days: 365, label: "1 year" },
  { days: null, label: "No expiry" },
];

export function keyExpiry(key: ApiKeyInfo, now = Date.now()): string {
  if (!key.expiresAt) {
    return "Never expires";
  }
  return Date.parse(key.expiresAt) <= now
    ? `Expired ${formatDate(key.expiresAt)}`
    : `Expires ${formatDate(key.expiresAt)}`;
}

function keyLabel(key: ApiKeyInfo): string {
  return key.name ?? "Unnamed key";
}

function NewKeySecret({ created, onDone }: { created: CreatedApiKey; onDone: () => void }) {
  const inputId = useId();
  const [copied, setCopied] = useState<"idle" | "copied" | "failed">("idle");
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-1.5">
        <label htmlFor={inputId} className="text-sm font-medium">
          API key
        </label>
        <input
          id={inputId}
          readOnly
          data-autofocus
          value={created.key}
          onFocus={(event) => event.currentTarget.select()}
          className={`${INPUT_CLASS} font-mono`}
        />
      </div>
      <p className="text-sm font-medium">
        Copy the key now. It is stored only as a hash and is never shown again.
      </p>
      {copied === "failed" ? (
        <p className="text-sm text-destructive">Copying failed. Select the key and copy it.</p>
      ) : null}
      <div className="flex justify-end gap-2">
        <Button
          variant="outline"
          onClick={async () => {
            try {
              await navigator.clipboard.writeText(created.key);
              setCopied("copied");
            } catch {
              setCopied("failed");
            }
          }}
        >
          {copied === "copied" ? "Copied" : "Copy key"}
        </Button>
        <Button onClick={onDone}>Done</Button>
      </div>
    </div>
  );
}

function ApiKeys() {
  const { auth, config } = useAccount();
  const headingId = useId();
  const { outcome, succeed } = useOutcome();
  const [keys, setKeys] = useState<ApiKeyInfo[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [created, setCreated] = useState<CreatedApiKey | null>(null);
  const [name, setName] = useState("");
  const [lifetime, setLifetime] = useState("90");
  const [revoking, setRevoking] = useState<ApiKeyInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [dialogError, setDialogError] = useState<string | null>(null);
  const lifetimeId = useId();

  const load = useCallback(async () => {
    try {
      setKeys(await auth.listApiKeys());
      setLoadError(null);
    } catch (error) {
      setLoadError(errorMessage(error, "Could not load your API keys."));
    }
  }, [auth]);

  useEffect(() => {
    void load();
  }, [load]);

  const closeCreate = () => {
    // Drop the secret from memory as soon as the dialog closes.
    setCreated(null);
    setCreating(false);
  };

  const onCreate = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setDialogError(null);
    try {
      const days = lifetime === "never" ? null : Number(lifetime);
      const key = await auth.createApiKey(name.trim(), days);
      setCreated(key);
      setName("");
      succeed(`API key ${keyLabel(key)} created.`);
      await load();
    } catch (error) {
      setDialogError(errorMessage(error, "Could not create the key."));
    } finally {
      setBusy(false);
    }
  };

  const onRevoke = async () => {
    if (!revoking) {
      return;
    }
    setBusy(true);
    setDialogError(null);
    try {
      await auth.deleteApiKey(revoking.id);
      // Reload first: closing the dialog then sends focus to the heading, since the
      // row's Revoke button is gone.
      await load();
      const label = keyLabel(revoking);
      setRevoking(null);
      succeed(`API key ${label} revoked. Requests that use it are refused from now on.`);
    } catch (error) {
      setDialogError(errorMessage(error, "Could not revoke the key."));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section
      title="Your keys"
      headingId={headingId}
      description={
        <>
          Scripts and integrations send a key in the <code>x-api-key</code> header to{" "}
          {displayHost(config.apiUrl)}. A key acts with your account and its plan.
        </>
      }
    >
      {loadError ? (
        <OutcomeMessage outcome={{ kind: "error", message: loadError }} />
      ) : keys === null ? (
        <p className="text-sm text-muted-foreground">Loading API keys…</p>
      ) : keys.length === 0 ? (
        <p className="text-sm text-muted-foreground">No API keys yet.</p>
      ) : (
        <ul className="flex flex-col divide-y divide-border" aria-labelledby={headingId}>
          {keys.map((key) => (
            <li key={key.id} className="flex flex-wrap items-center justify-between gap-3 py-2">
              <span className="flex flex-col gap-0.5">
                <span className="flex items-center gap-2 font-medium">
                  {keyLabel(key)}
                  {key.enabled ? null : <Badge variant="outline">Disabled</Badge>}
                </span>
                <span className="text-xs text-muted-foreground">
                  {key.start ? <code>{key.start}…</code> : null} Created {formatDate(key.createdAt)}{" "}
                  · {keyExpiry(key)}
                </span>
              </span>
              <Button
                variant="destructive"
                size="sm"
                aria-label={`Revoke API key ${keyLabel(key)}`}
                onClick={() => {
                  setDialogError(null);
                  setRevoking(key);
                }}
              >
                Revoke<span className="sr-only"> API key {keyLabel(key)}</span>
              </Button>
            </li>
          ))}
        </ul>
      )}
      <OutcomeMessage outcome={outcome} />
      <Button
        className="self-start"
        onClick={() => {
          setDialogError(null);
          setCreating(true);
        }}
      >
        <PlusIcon aria-hidden="true" />
        Create API key
      </Button>

      {creating ? (
        <AccountDialog
          key={created ? "secret" : "form"}
          title={created ? "Your new API key" : "Create an API key"}
          description={
            created ? undefined : "Name the key after the script or service that will use it."
          }
          onClose={closeCreate}
        >
          {created ? (
            <NewKeySecret created={created} onDone={closeCreate} />
          ) : (
            <form onSubmit={onCreate} className="flex flex-col gap-4">
              <TextField
                label="Name"
                data-autofocus
                required
                maxLength={64}
                placeholder="For example: CI"
                value={name}
                onValueChange={setName}
                error={dialogError}
              />
              <div className="flex flex-col gap-1.5">
                <label htmlFor={lifetimeId} className="text-sm font-medium">
                  Expires after
                </label>
                <select
                  id={lifetimeId}
                  value={lifetime}
                  onChange={(event) => setLifetime(event.target.value)}
                  className={INPUT_CLASS}
                >
                  {KEY_LIFETIMES.map((option) => (
                    <option key={option.label} value={option.days ?? "never"}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="flex justify-end gap-2">
                <Button variant="ghost" onClick={closeCreate}>
                  Cancel
                </Button>
                <Button type="submit" disabled={busy}>
                  Create key
                </Button>
              </div>
            </form>
          )}
        </AccountDialog>
      ) : null}

      {revoking ? (
        <AccountDialog
          role="alertdialog"
          title={`Revoke ${keyLabel(revoking)}?`}
          description="Requests that use this key are refused at once. This cannot be undone."
          onClose={() => setRevoking(null)}
          returnFocus={() => document.getElementById(headingId)}
        >
          {dialogError ? (
            <OutcomeMessage outcome={{ kind: "error", message: dialogError }} />
          ) : null}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" data-autofocus onClick={() => setRevoking(null)}>
              Cancel
            </Button>
            <Button variant="destructive" disabled={busy} onClick={() => void onRevoke()}>
              Revoke key
            </Button>
          </div>
        </AccountDialog>
      ) : null}
    </Section>
  );
}

export function ApiKeysPanel() {
  return (
    <div className="flex flex-col gap-6">
      <h1 className="text-3xl font-bold tracking-tight">API keys</h1>
      <SessionGate path="/api-keys">{() => <ApiKeys />}</SessionGate>
    </div>
  );
}
