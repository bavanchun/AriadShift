"use client";

import { Button } from "@ariadshift/ui/components/button";
import { useCallback, useEffect, useId, useState } from "react";
import { useAccount } from "./account-context";
import { AccountDialog } from "./account-dialog";
import { errorMessage, formatDate, OutcomeMessage, Section, useOutcome } from "./account-ui";
import type { OAuthClientInfo, OAuthConsentInfo } from "./auth-api";

type RevokeTarget =
  | { kind: "consent"; item: OAuthConsentInfo; label: string }
  | { kind: "client"; item: OAuthClientInfo; label: string };

export function ConnectedAppsSection() {
  const { auth } = useAccount();
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
      setLoadError(errorMessage(error, "Could not load your connected apps."));
    }
  }, [auth]);

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
      succeed(`${label} revoked.`);
    } catch (error) {
      setDialogError(errorMessage(error, "Could not revoke access."));
    } finally {
      setBusy(false);
    }
  };

  const loading = consents === null || clients === null;
  const empty = !loading && consents.length === 0 && clients.length === 0;

  return (
    <Section
      title="Connected apps"
      headingId={headingId}
      description="Apps and MCP clients you have authorized to use your account, and OAuth clients you have registered."
    >
      {loadError ? (
        <OutcomeMessage outcome={{ kind: "error", message: loadError }} />
      ) : loading ? (
        <p className="text-sm text-muted-foreground">Loading connected apps…</p>
      ) : empty ? (
        <p className="text-sm text-muted-foreground">
          No connected apps or registered OAuth clients yet.
        </p>
      ) : (
        <ul className="flex flex-col divide-y divide-border" aria-labelledby={headingId}>
          {consents.map((consent) => {
            const label = consent.clientName ?? consent.clientId;
            const scopes = consent.scopes.length > 0 ? consent.scopes.join(", ") : "none";
            return (
              <li
                key={`consent-${consent.id}`}
                className="flex items-center justify-between gap-3 py-2"
              >
                <span className="flex flex-col">
                  <span className="font-medium">{label}</span>
                  <span className="text-xs text-muted-foreground">
                    Authorized app · Scopes: {scopes} · Authorized{" "}
                    {formatDate(consent.updatedAt || consent.createdAt)}
                  </span>
                </span>
                <Button
                  variant="destructive"
                  size="sm"
                  aria-label={`Revoke ${label}`}
                  onClick={() => {
                    setDialogError(null);
                    setRevoking({ kind: "consent", item: consent, label });
                  }}
                >
                  Revoke
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
                    Registered OAuth client ({client.clientId})
                    {client.createdAt ? ` · Registered ${formatDate(client.createdAt)}` : ""}
                  </span>
                </span>
                <Button
                  variant="destructive"
                  size="sm"
                  aria-label={`Revoke ${label}`}
                  onClick={() => {
                    setDialogError(null);
                    setRevoking({ kind: "client", item: client, label });
                  }}
                >
                  Revoke
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
          title={`Revoke ${revoking.label}?`}
          description={
            revoking.kind === "consent"
              ? "This app will lose access to your account and its active refresh tokens will stop working until you authorize it again."
              : "Deleting this registered OAuth client revokes its credentials, consents, and active tokens."
          }
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
              Revoke
            </Button>
          </div>
        </AccountDialog>
      ) : null}
    </Section>
  );
}
