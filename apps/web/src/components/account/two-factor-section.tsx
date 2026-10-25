"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { type FormEvent, useId, useState } from "react";
import { useAccount } from "./account-context";
import { AccountDialog } from "./account-dialog";
import { errorMessage, OutcomeMessage, Section, TextField, useOutcome } from "./account-ui";
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
  const [copied, setCopied] = useState<"idle" | "copied" | "failed">("idle");
  return (
    <div className="flex flex-col gap-2">
      <p className="text-sm">
        Save these backup codes somewhere safe. Each one signs you in once if you lose your
        authenticator. They are not shown again.
      </p>
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
          ? "Copied"
          : copied === "failed"
            ? "Copy failed; select the codes instead"
            : "Copy backup codes"}
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

  const run = async (action: () => Promise<void>, fallback: string) => {
    setBusy(true);
    setDialogError(null);
    try {
      await action();
    } catch (error) {
      setDialogError(errorMessage(error, fallback));
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
      }, "Could not start two-factor setup. Check your password.");
    } else if (dialog?.step === "disable") {
      void run(async () => {
        await auth.disableTwoFactor(password);
        await refreshSession();
        setDialog(null);
        succeed("Two-factor authentication is off.");
      }, "Could not turn off two-factor authentication. Check your password.");
    } else if (dialog?.step === "regenerate") {
      void run(async () => {
        const codes = await auth.regenerateBackupCodes(password);
        open({ step: "codes", codes });
        succeed("New backup codes created. The old ones no longer work.");
      }, "Could not create new backup codes. Check your password.");
    }
  };

  const onVerify = (event: FormEvent) => {
    event.preventDefault();
    void run(async () => {
      await auth.verifyTotp(code.replace(/\s+/g, ""));
      await refreshSession();
      setDialog(null);
      succeed("Two-factor authentication is on.");
    }, "That code was not accepted. Check the time on your device and try again.");
  };

  const passwordTitle =
    dialog?.step === "enable-password"
      ? "Turn on two-factor authentication"
      : dialog?.step === "disable"
        ? "Turn off two-factor authentication"
        : "Create new backup codes";

  return (
    <Section
      title="Two-factor authentication"
      headingId={headingId}
      description="Ask for a code from an authenticator app after your password."
    >
      <p className="flex items-center gap-2 text-sm">
        Status: {user.twoFactorEnabled ? <Badge>On</Badge> : <Badge variant="outline">Off</Badge>}
      </p>
      <OutcomeMessage outcome={outcome} />
      <div className="flex flex-wrap gap-2">
        {user.twoFactorEnabled ? (
          <>
            <Button variant="outline" onClick={() => open({ step: "regenerate" })}>
              Create new backup codes
            </Button>
            <Button variant="destructive" onClick={() => open({ step: "disable" })}>
              Turn off two-factor authentication
            </Button>
          </>
        ) : (
          <Button variant="outline" onClick={() => open({ step: "enable-password" })}>
            Turn on two-factor authentication
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
          description="Confirm your password to continue."
          onClose={() => setDialog(null)}
          returnFocus={returnFocus}
        >
          <form onSubmit={onPassword} className="flex flex-col gap-4">
            <TextField
              label="Password"
              type="password"
              autoComplete="current-password"
              data-autofocus
              required
              value={password}
              onValueChange={setPassword}
              error={dialogError}
            />
            <div className="flex justify-end gap-2">
              <Button variant="ghost" onClick={() => setDialog(null)}>
                Cancel
              </Button>
              <Button
                type="submit"
                variant={dialog.step === "disable" ? "destructive" : "default"}
                disabled={busy}
              >
                Continue
              </Button>
            </div>
          </form>
        </AccountDialog>
      ) : null}

      {dialog?.step === "enable-verify" ? (
        <AccountDialog
          key={dialog.step}
          title="Set up your authenticator"
          description="Add AriadShift to your authenticator app, then enter the code it shows."
          onClose={() => setDialog(null)}
          returnFocus={returnFocus}
        >
          <div className="flex flex-col gap-2 text-sm">
            <p>Enter this setup key in the app:</p>
            <p className="rounded-lg bg-muted p-3 font-mono break-all">
              {totpSecret(dialog.setup.totpUri) ?? "Unavailable"}
            </p>
            <a
              href={dialog.setup.totpUri}
              className="self-start text-primary underline-offset-4 hover:underline"
            >
              Open in an authenticator app on this device
            </a>
          </div>
          {dialog.setup.backupCodes.length > 0 ? (
            <BackupCodes codes={dialog.setup.backupCodes} />
          ) : null}
          <form onSubmit={onVerify} className="flex flex-col gap-4">
            <TextField
              label="Authentication code"
              inputMode="numeric"
              autoComplete="one-time-code"
              data-autofocus
              required
              value={code}
              onValueChange={setCode}
              error={dialogError}
            />
            <div className="flex justify-end gap-2">
              <Button variant="ghost" onClick={() => setDialog(null)}>
                Cancel
              </Button>
              <Button type="submit" disabled={busy}>
                Turn on
              </Button>
            </div>
          </form>
        </AccountDialog>
      ) : null}

      {dialog?.step === "codes" ? (
        <AccountDialog
          key={dialog.step}
          title="New backup codes"
          onClose={() => setDialog(null)}
          returnFocus={returnFocus}
        >
          <BackupCodes codes={dialog.codes} />
          <div className="flex justify-end">
            <Button data-autofocus onClick={() => setDialog(null)}>
              Done
            </Button>
          </div>
        </AccountDialog>
      ) : null}
    </Section>
  );
}
