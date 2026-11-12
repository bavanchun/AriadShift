"use client";

// API keys: list, create (the key is shown once) and revoke, through the auth
// service. The API accepts a key in the `x-api-key` header.

import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { PlusIcon } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useId, useState } from "react";
import { displayHost } from "@/lib/cloud/config";
import { useAccount } from "./account-context";
import { AccountDialog } from "./account-dialog";
import {
  errorMessage,
  formatDate,
  INPUT_CLASS,
  OutcomeMessage,
  Section,
  TextField,
  type Translate,
  useAccountText,
  useOutcome,
} from "./account-ui";
import type { ApiKeyInfo, CreatedApiKey } from "./auth-api";
import { SessionGate } from "./session-gate";

/** Lifetimes offered when creating a key, in days; null never expires. */
export const KEY_LIFETIMES: ReadonlyArray<{
  days: number | null;
  label: (t: Translate) => string;
}> = [
  { days: 30, label: (t) => t("api_keys.lifetime_days", { days: 30 }) },
  { days: 90, label: (t) => t("api_keys.lifetime_days", { days: 90 }) },
  { days: 365, label: (t) => t("api_keys.lifetime_year") },
  { days: null, label: (t) => t("api_keys.lifetime_never") },
];

export function keyExpiry(key: ApiKeyInfo, t: Translate, locale: string, now = Date.now()): string {
  if (!key.expiresAt) {
    return t("api_keys.never_expires");
  }
  const date = formatDate(key.expiresAt, t, locale);
  return t(Date.parse(key.expiresAt) <= now ? "api_keys.expired_on" : "api_keys.expires_on", {
    date,
  });
}

function keyLabel(key: ApiKeyInfo, t: Translate): string {
  return key.name ?? t("api_keys.unnamed");
}

function NewKeySecret({ created, onDone }: { created: CreatedApiKey; onDone: () => void }) {
  const { t } = useAccountText();
  const inputId = useId();
  const [copied, setCopied] = useState<"idle" | "copied" | "failed">("idle");
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-1.5">
        <label htmlFor={inputId} className="text-sm font-medium">
          {t("api_keys.secret_label")}
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
      <p className="text-sm font-medium">{t("api_keys.secret_notice")}</p>
      {copied === "failed" ? (
        <p className="text-sm text-destructive">{t("api_keys.copy_failed")}</p>
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
          {copied === "copied" ? t("actions.copied") : t("api_keys.copy_key")}
        </Button>
        <Button onClick={onDone}>{t("actions.done")}</Button>
      </div>
    </div>
  );
}

function ApiKeys() {
  const { auth, config } = useAccount();
  const { t, locale } = useAccountText();
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
      setLoadError(errorMessage(t, error, "api_keys.load_failed"));
    }
  }, [auth, t]);

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
      succeed(t("api_keys.created", { name: keyLabel(key, t) }));
      await load();
    } catch (error) {
      setDialogError(errorMessage(t, error, "api_keys.create_failed"));
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
      const label = keyLabel(revoking, t);
      setRevoking(null);
      succeed(t("api_keys.revoked", { name: label }));
    } catch (error) {
      setDialogError(errorMessage(t, error, "api_keys.revoke_failed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section
      title={t("api_keys.section_title")}
      headingId={headingId}
      description={t.rich("api_keys.section_description", {
        code: (chunks) => <code>{chunks}</code>,
        host: displayHost(config.apiUrl),
      })}
    >
      {loadError ? (
        <OutcomeMessage outcome={{ kind: "error", message: loadError }} />
      ) : keys === null ? (
        <p className="text-sm text-muted-foreground">{t("api_keys.loading")}</p>
      ) : keys.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("api_keys.empty")}</p>
      ) : (
        <ul className="flex flex-col divide-y divide-border" aria-labelledby={headingId}>
          {keys.map((key) => (
            <li key={key.id} className="flex flex-wrap items-center justify-between gap-3 py-2">
              <span className="flex flex-col gap-0.5">
                <span className="flex items-center gap-2 font-medium">
                  {keyLabel(key, t)}
                  {key.enabled ? null : <Badge variant="outline">{t("api_keys.disabled")}</Badge>}
                </span>
                <span className="text-xs text-muted-foreground">
                  {key.start ? <code>{key.start}…</code> : null}{" "}
                  {t("api_keys.key_details", {
                    date: formatDate(key.createdAt, t, locale),
                    expiry: keyExpiry(key, t, locale),
                  })}
                </span>
              </span>
              <Button
                variant="destructive"
                size="sm"
                aria-label={t("api_keys.revoke_label", { name: keyLabel(key, t) })}
                onClick={() => {
                  setDialogError(null);
                  setRevoking(key);
                }}
              >
                {t.rich("api_keys.revoke_button", {
                  name: keyLabel(key, t),
                  hidden: (chunks) => <span className="sr-only">{chunks}</span>,
                })}
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
        {t("api_keys.create_button")}
      </Button>

      {creating ? (
        <AccountDialog
          key={created ? "secret" : "form"}
          title={created ? t("api_keys.secret_title") : t("api_keys.create_title")}
          description={created ? undefined : t("api_keys.create_description")}
          onClose={closeCreate}
        >
          {created ? (
            <NewKeySecret created={created} onDone={closeCreate} />
          ) : (
            <form onSubmit={onCreate} className="flex flex-col gap-4">
              <TextField
                label={t("fields.name")}
                data-autofocus
                required
                maxLength={64}
                placeholder={t("api_keys.name_placeholder")}
                value={name}
                onValueChange={setName}
                error={dialogError}
              />
              <div className="flex flex-col gap-1.5">
                <label htmlFor={lifetimeId} className="text-sm font-medium">
                  {t("api_keys.expires_after")}
                </label>
                <select
                  id={lifetimeId}
                  value={lifetime}
                  onChange={(event) => setLifetime(event.target.value)}
                  className={INPUT_CLASS}
                >
                  {KEY_LIFETIMES.map((option) => (
                    <option key={option.days ?? "never"} value={option.days ?? "never"}>
                      {option.label(t)}
                    </option>
                  ))}
                </select>
              </div>
              <div className="flex justify-end gap-2">
                <Button type="button" variant="ghost" onClick={closeCreate}>
                  {t("actions.cancel")}
                </Button>
                <Button type="submit" disabled={busy}>
                  {t("api_keys.create_submit")}
                </Button>
              </div>
            </form>
          )}
        </AccountDialog>
      ) : null}

      {revoking ? (
        <AccountDialog
          role="alertdialog"
          title={t("api_keys.revoke_title", { name: keyLabel(revoking, t) })}
          description={t("api_keys.revoke_description")}
          onClose={() => setRevoking(null)}
          returnFocus={() => document.getElementById(headingId)}
        >
          {dialogError ? (
            <OutcomeMessage outcome={{ kind: "error", message: dialogError }} />
          ) : null}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" data-autofocus onClick={() => setRevoking(null)}>
              {t("actions.cancel")}
            </Button>
            <Button variant="destructive" disabled={busy} onClick={() => void onRevoke()}>
              {t("api_keys.revoke_confirm")}
            </Button>
          </div>
        </AccountDialog>
      ) : null}
    </Section>
  );
}

export function ApiKeysPanel() {
  const { t } = useAccountText();
  return (
    <div className="flex flex-col gap-6">
      <h1 className="text-3xl font-bold tracking-tight">{t("api_keys.page_title")}</h1>
      <SessionGate path="/api-keys">{() => <ApiKeys />}</SessionGate>
    </div>
  );
}
