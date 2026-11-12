"use client";

// Sign in with email and password or a passkey, finish a two-factor sign-in
// with a TOTP or backup code, or create an account.

import { Button, buttonVariants } from "@ariadshift/ui/components/button";
import { KeyRoundIcon } from "lucide-react";
import { type FormEvent, useEffect, useRef, useState } from "react";
import { Turnstile } from "@/components/cloud/turnstile";
import { localePath } from "@/i18n/paths";
import { useLocalePath } from "@/i18n/use-locale-path";
import { CLOUD } from "@/lib/cloud/config";
import { useAccount } from "./account-context";
import {
  errorMessage,
  MIN_PASSWORD_LENGTH,
  type Outcome,
  OutcomeMessage,
  TextField,
  useAccountText,
} from "./account-ui";

/**
 * A same-origin path to return to after signing in; anything else goes to /account.
 * The path has no locale prefix: the sign-in page adds the active locale again.
 */
export function safeNextPath(value: string | null): string {
  if (!value?.startsWith("/") || value.startsWith("//") || value.includes("\\")) {
    return "/account";
  }
  const origin =
    typeof location === "object" && location?.origin ? location.origin : "http://localhost";
  try {
    const url = new URL(value, origin);
    const decodedPath = decodeURIComponent(url.pathname);
    if (
      url.origin !== origin ||
      url.pathname.startsWith("//") ||
      url.pathname.includes("\\") ||
      decodedPath.startsWith("//") ||
      decodedPath.includes("\\") ||
      new URL(decodedPath, origin).origin !== origin
    ) {
      return "/account";
    }
    return `${url.pathname}${url.search}${url.hash}`;
  } catch {
    return "/account";
  }
}

type Mode = "sign-in" | "sign-up" | "two-factor";

export interface SignInPanelProps {
  /** Navigates after a successful sign-in; defaults to a full page load. */
  navigate?: (path: string) => void;
}

export function SignInPanel({ navigate }: SignInPanelProps) {
  const { auth, session, refreshSession, announce } = useAccount();
  const { t, locale } = useAccountText();
  const toLocale = useLocalePath();
  const [mode, setMode] = useState<Mode>("sign-in");
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [code, setCode] = useState("");
  const [useBackupCode, setUseBackupCode] = useState(false);
  const [trustDevice, setTrustDevice] = useState(false);
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<Outcome>(null);
  const [confirmError, setConfirmError] = useState<string | null>(null);
  const [signupToken, setSignupToken] = useState<string | null>(null);
  const [challengeReset, setChallengeReset] = useState(0);
  const siteKey = CLOUD?.turnstileSiteKey;
  const headingRef = useRef<HTMLHeadingElement>(null);

  // Move focus to a new step's heading so keyboard and screen reader users follow it.
  const shownMode = useRef(mode);
  useEffect(() => {
    if (shownMode.current !== mode) {
      shownMode.current = mode;
      headingRef.current?.focus();
    }
  }, [mode]);

  const finish = async () => {
    await refreshSession();
    announce(t("sign_in.signed_in"));
    const next = safeNextPath(new URLSearchParams(window.location.search).get("next"));
    (navigate ?? ((path: string) => window.location.assign(path)))(localePath(locale, next));
  };

  const run = async (action: () => Promise<void>, fallbackKey: string) => {
    setBusy(true);
    setOutcome(null);
    try {
      await action();
    } catch (error) {
      setOutcome({ kind: "error", message: errorMessage(t, error, fallbackKey) });
    } finally {
      setBusy(false);
    }
  };

  const onSignIn = (event: FormEvent) => {
    event.preventDefault();
    void run(async () => {
      const { twoFactorRequired } = await auth.signInEmail(email.trim(), password);
      setPassword("");
      if (twoFactorRequired) {
        setMode("two-factor");
        return;
      }
      await finish();
    }, "sign_in.sign_in_failed");
  };

  const onPasskey = () => {
    void run(async () => {
      await auth.signInPasskey();
      await finish();
    }, "sign_in.passkey_failed");
  };

  const onTwoFactor = (event: FormEvent) => {
    event.preventDefault();
    void run(async () => {
      const value = code.trim();
      if (useBackupCode) {
        await auth.verifyBackupCode(value);
      } else {
        await auth.verifyTotp(value.replace(/\s+/g, ""), trustDevice);
      }
      setCode("");
      await finish();
    }, "sign_in.code_rejected");
  };

  const onSignUp = (event: FormEvent) => {
    event.preventDefault();
    if (password !== confirm) {
      setConfirmError(t("sign_in.passwords_mismatch"));
      return;
    }
    setConfirmError(null);
    if (busy) return;
    if (siteKey && !signupToken) {
      setOutcome({ kind: "error", message: t("sign_in.bot_check") });
      return;
    }
    void run(async () => {
      try {
        await auth.signUpEmail(
          name.trim(),
          email.trim(),
          password,
          ...(signupToken ? ([signupToken] as const) : []),
        );
      } finally {
        setSignupToken(null);
        setChallengeReset((value) => value + 1);
      }
      setPassword("");
      setConfirm("");
      await finish();
    }, "sign_in.create_failed");
  };

  if (session.status === "signed-in" && mode !== "two-factor") {
    return (
      <div className="flex flex-col items-start gap-3">
        <p>
          {t.rich("sign_in.signed_in_user", {
            email: session.user.email,
            user: (chunks) => <span className="font-medium">{chunks}</span>,
          })}
        </p>
        <a href={toLocale("/account")} className={buttonVariants()}>
          {t("sign_in.go_to_account")}
        </a>
      </div>
    );
  }

  const heading =
    mode === "sign-in"
      ? t("sign_in.title")
      : mode === "sign-up"
        ? t("sign_in.create_title")
        : t("sign_in.two_factor_title");

  return (
    <div className="flex max-w-md flex-col gap-6">
      <h1 ref={headingRef} tabIndex={-1} className="text-3xl font-bold tracking-tight outline-none">
        {heading}
      </h1>

      {mode === "sign-in" ? (
        <>
          <form onSubmit={onSignIn} className="flex flex-col gap-4">
            <TextField
              label={t("fields.email")}
              type="email"
              autoComplete="username webauthn"
              required
              value={email}
              onValueChange={setEmail}
            />
            <TextField
              label={t("fields.password")}
              type="password"
              autoComplete="current-password"
              required
              value={password}
              onValueChange={setPassword}
            />
            <OutcomeMessage outcome={outcome} />
            <Button type="submit" disabled={busy}>
              {t("actions.sign_in")}
            </Button>
          </form>
          <Button variant="outline" onClick={onPasskey} disabled={busy}>
            <KeyRoundIcon aria-hidden="true" />
            {t("sign_in.passkey")}
          </Button>
          <p className="text-sm">
            {t.rich("sign_in.no_account", {
              switch: (chunks) => (
                <button
                  type="button"
                  className="text-primary underline-offset-4 hover:underline"
                  onClick={() => {
                    setOutcome(null);
                    setMode("sign-up");
                  }}
                >
                  {chunks}
                </button>
              ),
            })}
          </p>
        </>
      ) : null}

      {mode === "two-factor" ? (
        <form onSubmit={onTwoFactor} className="flex flex-col gap-4">
          <p className="text-sm text-muted-foreground">
            {t(useBackupCode ? "sign_in.backup_help" : "sign_in.totp_help")}
          </p>
          <TextField
            label={useBackupCode ? t("sign_in.backup_code") : t("fields.authentication_code")}
            inputMode={useBackupCode ? "text" : "numeric"}
            autoComplete="one-time-code"
            required
            value={code}
            onValueChange={setCode}
          />
          {useBackupCode ? null : (
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={trustDevice}
                onChange={(event) => setTrustDevice(event.target.checked)}
              />
              {t("sign_in.trust_device")}
            </label>
          )}
          <OutcomeMessage outcome={outcome} />
          <Button type="submit" disabled={busy}>
            {t("sign_in.verify")}
          </Button>
          <button
            type="button"
            className="self-start text-sm text-primary underline-offset-4 hover:underline"
            onClick={() => {
              setCode("");
              setOutcome(null);
              setUseBackupCode((value) => !value);
            }}
          >
            {t(useBackupCode ? "sign_in.use_authenticator" : "sign_in.use_backup")}
          </button>
        </form>
      ) : null}

      {mode === "sign-up" ? (
        <>
          <form onSubmit={onSignUp} className="flex flex-col gap-4">
            <TextField
              label={t("fields.name")}
              autoComplete="name"
              required
              value={name}
              onValueChange={setName}
            />
            <TextField
              label={t("fields.email")}
              type="email"
              autoComplete="email"
              required
              value={email}
              onValueChange={setEmail}
            />
            <TextField
              label={t("fields.password")}
              type="password"
              autoComplete="new-password"
              required
              minLength={MIN_PASSWORD_LENGTH}
              hint={t("fields.password_hint", { min: MIN_PASSWORD_LENGTH })}
              value={password}
              onValueChange={setPassword}
            />
            <TextField
              label={t("sign_in.confirm_password")}
              type="password"
              autoComplete="new-password"
              required
              value={confirm}
              onValueChange={setConfirm}
              error={confirmError}
            />
            {siteKey ? (
              <Turnstile
                siteKey={siteKey}
                action="signup"
                resetKey={challengeReset}
                onToken={setSignupToken}
              />
            ) : null}
            <OutcomeMessage outcome={outcome} />
            <Button type="submit" disabled={busy || Boolean(siteKey && !signupToken)}>
              {t("sign_in.create_account")}
            </Button>
          </form>
          <p className="text-sm">
            {t.rich("sign_in.have_account", {
              switch: (chunks) => (
                <button
                  type="button"
                  className="text-primary underline-offset-4 hover:underline"
                  onClick={() => {
                    setOutcome(null);
                    setMode("sign-in");
                  }}
                >
                  {chunks}
                </button>
              ),
            })}
          </p>
        </>
      ) : null}
    </div>
  );
}
