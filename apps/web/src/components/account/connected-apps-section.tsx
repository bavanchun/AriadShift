"use client";

import { Button } from "@ariadshift/ui/components/button";
import { useCallback, useEffect, useId, useState } from "react";
import { useAccount } from "./account-context";
import { AccountDialog } from "./account-dialog";
import {
  errorMessage,
  formatDate,
  OutcomeMessage,
  Section,
  useAccountText,
  useOutcome,
} from "./account-ui";
import type { OAuthClientInfo, OAuthConsentInfo } from "./auth-api";

type RevokeTarget =
  | { kind: "consent"; item: OAuthConsentInfo; label: string }
  | { kind: "client"; item: OAuthClientInfo; label: string };

export function ConnectedAppsSection() {
  const { auth } = useAccount();
  const { t, locale } = useAccountText();
  const headingId = useId();
  const [consents, setConsents] = useState<OAuthConsentInfo[] | null>(null);
  const [clients, setClients] = useState<OAuthClientInfo[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<RevokeTarget | null>(null);
  const [busy, setBusy] = useState(false);
  const { outcome, succeed } = useOutcome();
  const [dialogError, setDialogError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [nextConsents, nextClients] = await Promise.all([
        auth.listOAuthConsents(),
        auth.listOAuthClients(),
      ]);
      setConsents(nextConsents);
      setClients(nextClients);
      setLoadError(null);
    } catch (error) {
      setLoadError(errorMessage(t, error, "connected_apps.load_failed"));
    }
  }, [auth, t]);

  useEffect(() => {
    void load();
  }, [load]);

  const onRevoke = async () => {
    if (!revoking) {
      return;
    }
    setBusy(true);
    setDialogError(null);
    try {
      if (revoking.kind === "consent") {
        await auth.deleteOAuthConsent(revoking.item.id);
      } else {
        await auth.deleteOAuthClient(revoking.item.clientId);
      }
      await load();
      const label = revoking.label;
      setRevoking(null);
      succeed(t("connected_apps.revoked", { name: label }));
    } catch (error) {
      setDialogError(errorMessage(t, error, "connected_apps.revoke_failed"));
    } finally {
      setBusy(false);
    }
  };

  const loading = consents === null || clients === null;
  const empty = !loading && consents.length === 0 && clients.length === 0;

  return (
    <Section
      title={t("connected_apps.title")}
      headingId={headingId}
      description={t("connected_apps.description")}
    >
      {loadError ? (
        <OutcomeMessage outcome={{ kind: "error", message: loadError }} />
      ) : loading ? (
        <p className="text-sm text-muted-foreground">{t("connected_apps.loading")}</p>
      ) : empty ? (
        <p className="text-sm text-muted-foreground">{t("connected_apps.empty")}</p>
      ) : (
        <ul className="flex flex-col divide-y divide-border" aria-labelledby={headingId}>
          {consents.map((consent) => {
            const label = consent.clientName ?? consent.clientId;
            const scopes =
              consent.scopes.length > 0
                ? consent.scopes.join(", ")
                : t("connected_apps.scopes_none");
            return (
              <li
                key={`consent-${consent.id}`}
                className="flex items-center justify-between gap-3 py-2"
              >
                <span className="flex flex-col">
                  <span className="font-medium">{label}</span>
                  <span className="text-xs text-muted-foreground">
                    {t("connected_apps.consent_details", {
                      scopes,
                      date: formatDate(consent.updatedAt || consent.createdAt, t, locale),
                    })}
                  </span>
                </span>
                <Button
                  variant="destructive"
                  size="sm"
                  aria-label={t("connected_apps.revoke_label", { name: label })}
                  onClick={() => {
                    setDialogError(null);
                    setRevoking({ kind: "consent", item: consent, label });
                  }}
                >
                  {t("actions.revoke")}
                </Button>
              </li>
            );
          })}
          {clients.map((client) => {
            const label = client.clientName ?? client.clientId;
            return (
              <li
                key={`client-${client.clientId}`}
                className="flex items-center justify-between gap-3 py-2"
              >
                <span className="flex flex-col">
                  <span className="font-medium">{label}</span>
                  <span className="text-xs text-muted-foreground">
                    {client.createdAt
                      ? t("connected_apps.client_details_dated", {
                          client_id: client.clientId,
                          date: formatDate(client.createdAt, t, locale),
                        })
                      : t("connected_apps.client_details", { client_id: client.clientId })}
                  </span>
                </span>
                <Button
                  variant="destructive"
                  size="sm"
                  aria-label={t("connected_apps.revoke_label", { name: label })}
                  onClick={() => {
                    setDialogError(null);
                    setRevoking({ kind: "client", item: client, label });
                  }}
                >
                  {t("actions.revoke")}
                </Button>
              </li>
            );
          })}
        </ul>
      )}
      <OutcomeMessage outcome={outcome} />

      {revoking ? (
        <AccountDialog
          role="alertdialog"
          title={t("connected_apps.revoke_title", { name: revoking.label })}
          description={t(
            revoking.kind === "consent"
              ? "connected_apps.revoke_consent_description"
              : "connected_apps.revoke_client_description",
          )}
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
              {t("actions.revoke")}
            </Button>
          </div>
        </AccountDialog>
      ) : null}
    </Section>
  );
}
