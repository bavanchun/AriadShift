"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { type FormEvent, useId, useState } from "react";
import { useAccount } from "./account-context";
import { AccountDialog } from "./account-dialog";
import {
  errorMessage,
  OutcomeMessage,
  Section,
  TextField,
  useAccountText,
  useOutcome,
} from "./account-ui";
import type { AccountUser, TwoFactorSetup } from "./auth-api";

/** The shared secret of an `otpauth://` URI, in groups of four for typing. */
export function totpSecret(uri: string): string | null {
  try {
    const secret = new URL(uri).searchParams.get("secret");
    return secret ? (secret.match(/.{1,4}/g) ?? [secret]).join(" ") : null;
  } catch {
    return null;
  }
}

function BackupCodes({ codes }: { codes: string[] }) {
  const { t } = useAccountText();
  const [copied, setCopied] = useState<"idle" | "copied" | "failed">("idle");
  return (
    <div className="flex flex-col gap-2">
      <p className="text-sm">{t("two_factor.backup_intro")}</p>
      <ul className="grid grid-cols-2 gap-1 rounded-lg bg-muted p-3 font-mono text-sm">
        {codes.map((code) => (
          <li key={code}>{code}</li>
        ))}
      </ul>
      <Button
        variant="outline"
        size="sm"
        className="self-start"
        onClick={async () => {
          try {
            await navigator.clipboard.writeText(codes.join("\n"));
            setCopied("copied");
          } catch {
            setCopied("failed");
          }
        }}
      >
        {copied === "copied"
          ? t("actions.copied")
          : copied === "failed"
            ? t("two_factor.copy_failed")
            : t("two_factor.copy_codes")}
      </Button>
    </div>
  );
}

type Dialog =
  | { step: "enable-password" }
  | { step: "enable-verify"; setup: TwoFactorSetup }
  | { step: "disable" }
  | { step: "regenerate" }
  | { step: "codes"; codes: string[] };

export function TwoFactorSection({ user }: { user: AccountUser }) {
  const { auth, refreshSession } = useAccount();
  const { t } = useAccountText();
  const { outcome, succeed } = useOutcome();
  const headingId = useId();
  // The opening button is replaced when the status changes, so focus falls back to the heading.
  const returnFocus = () => document.getElementById(headingId);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [dialogError, setDialogError] = useState<string | null>(null);

  const open = (next: Dialog) => {
    setPassword("");
    setCode("");
    setDialogError(null);
    setDialog(next);
  };

  const run = async (action: () => Promise<void>, fallbackKey: string) => {
    setBusy(true);
    setDialogError(null);
    try {
      await action();
    } catch (error) {
      setDialogError(errorMessage(t, error, fallbackKey));
    } finally {
      setBusy(false);
    }
  };

  const onPassword = (event: FormEvent) => {
    event.preventDefault();
    if (dialog?.step === "enable-password") {
      void run(async () => {
        const setup = await auth.enableTwoFactor(password);
        setPassword("");
        open({ step: "enable-verify", setup });
      }, "two_factor.enable_failed");
    } else if (dialog?.step === "disable") {
      void run(async () => {
        await auth.disableTwoFactor(password);
        await refreshSession();
        setDialog(null);
        succeed(t("two_factor.disabled"));
      }, "two_factor.disable_failed");
    } else if (dialog?.step === "regenerate") {
      void run(async () => {
        const codes = await auth.regenerateBackupCodes(password);
        open({ step: "codes", codes });
        succeed(t("two_factor.regenerated"));
      }, "two_factor.regenerate_failed");
    }
  };

  const onVerify = (event: FormEvent) => {
    event.preventDefault();
    void run(async () => {
      await auth.verifyTotp(code.replace(/\s+/g, ""));
      await refreshSession();
      setDialog(null);
      succeed(t("two_factor.enabled"));
    }, "two_factor.verify_failed");
  };

  const passwordTitle =
    dialog?.step === "enable-password"
      ? t("two_factor.enable")
      : dialog?.step === "disable"
        ? t("two_factor.disable")
        : t("two_factor.regenerate");

  return (
    <Section
      title={t("two_factor.title")}
      headingId={headingId}
      description={t("two_factor.description")}
    >
      <p className="flex items-center gap-2 text-sm">
        {user.twoFactorEnabled
          ? t.rich("two_factor.status_on", { badge: (chunks) => <Badge>{chunks}</Badge> })
          : t.rich("two_factor.status_off", {
              badge: (chunks) => <Badge variant="outline">{chunks}</Badge>,
            })}
      </p>
      <OutcomeMessage outcome={outcome} />
      <div className="flex flex-wrap gap-2">
        {user.twoFactorEnabled ? (
          <>
            <Button variant="outline" onClick={() => open({ step: "regenerate" })}>
              {t("two_factor.regenerate")}
            </Button>
            <Button variant="destructive" onClick={() => open({ step: "disable" })}>
              {t("two_factor.disable")}
            </Button>
          </>
        ) : (
          <Button variant="outline" onClick={() => open({ step: "enable-password" })}>
            {t("two_factor.enable")}
          </Button>
        )}
      </div>

      {dialog &&
      (dialog.step === "enable-password" ||
        dialog.step === "disable" ||
        dialog.step === "regenerate") ? (
        <AccountDialog
          key={dialog.step}
          title={passwordTitle}
          description={t("two_factor.confirm_password")}
          onClose={() => setDialog(null)}
          returnFocus={returnFocus}
        >
          <form onSubmit={onPassword} className="flex flex-col gap-4">
            <TextField
              label={t("fields.password")}
              type="password"
              autoComplete="current-password"
              data-autofocus
              required
              value={password}
              onValueChange={setPassword}
              error={dialogError}
            />
            <div className="flex justify-end gap-2">
              <Button type="button" variant="ghost" onClick={() => setDialog(null)}>
                {t("actions.cancel")}
              </Button>
              <Button
                type="submit"
                variant={dialog.step === "disable" ? "destructive" : "default"}
                disabled={busy}
              >
                {t("actions.continue")}
              </Button>
            </div>
          </form>
        </AccountDialog>
      ) : null}

      {dialog?.step === "enable-verify" ? (
        <AccountDialog
          key={dialog.step}
          title={t("two_factor.setup_title")}
          description={t("two_factor.setup_description")}
          onClose={() => setDialog(null)}
          returnFocus={returnFocus}
        >
          <div className="flex flex-col gap-2 text-sm">
            <p>{t("two_factor.setup_key_intro")}</p>
            <p className="rounded-lg bg-muted p-3 font-mono break-all">
              {totpSecret(dialog.setup.totpUri) ?? t("two_factor.setup_key_unavailable")}
            </p>
            <a
              href={dialog.setup.totpUri}
              className="self-start text-primary underline-offset-4 hover:underline"
            >
              {t("two_factor.open_in_app")}
            </a>
          </div>
          {dialog.setup.backupCodes.length > 0 ? (
            <BackupCodes codes={dialog.setup.backupCodes} />
          ) : null}
          <form onSubmit={onVerify} className="flex flex-col gap-4">
            <TextField
              label={t("fields.authentication_code")}
              inputMode="numeric"
              autoComplete="one-time-code"
              data-autofocus
              required
              value={code}
              onValueChange={setCode}
              error={dialogError}
            />
            <div className="flex justify-end gap-2">
              <Button type="button" variant="ghost" onClick={() => setDialog(null)}>
                {t("actions.cancel")}
              </Button>
              <Button type="submit" disabled={busy}>
                {t("two_factor.turn_on")}
              </Button>
            </div>
          </form>
        </AccountDialog>
      ) : null}

      {dialog?.step === "codes" ? (
        <AccountDialog
          key={dialog.step}
          title={t("two_factor.codes_title")}
          onClose={() => setDialog(null)}
          returnFocus={returnFocus}
        >
          <BackupCodes codes={dialog.codes} />
          <div className="flex justify-end">
            <Button data-autofocus onClick={() => setDialog(null)}>
              {t("actions.done")}
            </Button>
          </div>
        </AccountDialog>
      ) : null}
    </Section>
  );
}
