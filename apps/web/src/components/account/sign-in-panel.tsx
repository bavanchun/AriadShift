"use client";

// Sign in with email and password or a passkey, finish a two-factor sign-in
// with a TOTP or backup code, or create an account.

import { Button, buttonVariants } from "@ariadshift/ui/components/button";
import { KeyRoundIcon } from "lucide-react";
import { type FormEvent, useEffect, useRef, useState } from "react";
import { useAccount } from "./account-context";
import {
  errorMessage,
  MIN_PASSWORD_LENGTH,
  type Outcome,
  OutcomeMessage,
  TextField,
} from "./account-ui";

/** A same-origin path to return to after signing in; anything else goes to /account. */
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
    announce("Signed in.");
    const next = safeNextPath(new URLSearchParams(window.location.search).get("next"));
    (navigate ?? ((path: string) => window.location.assign(path)))(next);
  };

  const run = async (action: () => Promise<void>, fallback: string) => {
    setBusy(true);
    setOutcome(null);
    try {
      await action();
    } catch (error) {
      setOutcome({ kind: "error", message: errorMessage(error, fallback) });
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
    }, "Could not sign in. Check your email and password.");
  };

  const onPasskey = () => {
    void run(async () => {
      await auth.signInPasskey();
      await finish();
    }, "Could not sign in with a passkey.");
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
    }, "That code was not accepted. Try again.");
  };

  const onSignUp = (event: FormEvent) => {
    event.preventDefault();
    if (password !== confirm) {
      setConfirmError("The passwords do not match.");
      return;
    }
    setConfirmError(null);
    void run(async () => {
      await auth.signUpEmail(name.trim(), email.trim(), password);
      setPassword("");
      setConfirm("");
      await finish();
    }, "Could not create the account.");
  };

  if (session.status === "signed-in" && mode !== "two-factor") {
    return (
      <div className="flex flex-col items-start gap-3">
        <p>
          You are signed in as <span className="font-medium">{session.user.email}</span>.
        </p>
        <a href="/account" className={buttonVariants()}>
          Go to your account
        </a>
      </div>
    );
  }

  const heading =
    mode === "sign-in" ? "Sign in" : mode === "sign-up" ? "Create an account" : "Two-factor check";

  return (
    <div className="flex max-w-md flex-col gap-6">
      <h1 ref={headingRef} tabIndex={-1} className="text-3xl font-bold tracking-tight outline-none">
        {heading}
      </h1>

      {mode === "sign-in" ? (
        <>
          <form onSubmit={onSignIn} className="flex flex-col gap-4">
            <TextField
              label="Email"
              type="email"
              autoComplete="username webauthn"
              required
              value={email}
              onValueChange={setEmail}
            />
            <TextField
              label="Password"
              type="password"
              autoComplete="current-password"
              required
              value={password}
              onValueChange={setPassword}
            />
            <OutcomeMessage outcome={outcome} />
            <Button type="submit" disabled={busy}>
              Sign in
            </Button>
          </form>
          <Button variant="outline" onClick={onPasskey} disabled={busy}>
            <KeyRoundIcon aria-hidden="true" />
            Sign in with a passkey
          </Button>
          <p className="text-sm">
            No account yet?{" "}
            <button
              type="button"
              className="text-primary underline-offset-4 hover:underline"
              onClick={() => {
                setOutcome(null);
                setMode("sign-up");
              }}
            >
              Create one
            </button>
          </p>
        </>
      ) : null}

      {mode === "two-factor" ? (
        <form onSubmit={onTwoFactor} className="flex flex-col gap-4">
          <p className="text-sm text-muted-foreground">
            {useBackupCode
              ? "Enter one of the backup codes you saved when you turned on two-factor authentication. Each code works once."
              : "Enter the 6-digit code from your authenticator app."}
          </p>
          <TextField
            label={useBackupCode ? "Backup code" : "Authentication code"}
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
              Trust this browser for 30 days
            </label>
          )}
          <OutcomeMessage outcome={outcome} />
          <Button type="submit" disabled={busy}>
            Verify
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
            {useBackupCode ? "Use an authenticator code instead" : "Use a backup code instead"}
          </button>
        </form>
      ) : null}

      {mode === "sign-up" ? (
        <>
          <form onSubmit={onSignUp} className="flex flex-col gap-4">
            <TextField
              label="Name"
              autoComplete="name"
              required
              value={name}
              onValueChange={setName}
            />
            <TextField
              label="Email"
              type="email"
              autoComplete="email"
              required
              value={email}
              onValueChange={setEmail}
            />
            <TextField
              label="Password"
              type="password"
              autoComplete="new-password"
              required
              minLength={MIN_PASSWORD_LENGTH}
              hint={`At least ${MIN_PASSWORD_LENGTH} characters.`}
              value={password}
              onValueChange={setPassword}
            />
            <TextField
              label="Confirm password"
              type="password"
              autoComplete="new-password"
              required
              value={confirm}
              onValueChange={setConfirm}
              error={confirmError}
            />
            <OutcomeMessage outcome={outcome} />
            <Button type="submit" disabled={busy}>
              Create account
            </Button>
          </form>
          <p className="text-sm">
            Already have an account?{" "}
            <button
              type="button"
              className="text-primary underline-offset-4 hover:underline"
              onClick={() => {
                setOutcome(null);
                setMode("sign-in");
              }}
            >
              Sign in
            </button>
          </p>
        </>
      ) : null}
    </div>
  );
}
