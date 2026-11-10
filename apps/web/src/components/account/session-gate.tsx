"use client";

// Renders its children for a signed-in user, and otherwise says why it cannot.

import { Alert, AlertDescription, AlertTitle } from "@ariadshift/ui/components/alert";
import { Button, buttonVariants } from "@ariadshift/ui/components/button";
import { useLocale } from "next-intl";
import type { ReactNode } from "react";
import { localePath } from "@/i18n/paths";
import { useAccount } from "./account-context";
import { useAccountText } from "./account-ui";
import type { AccountUser } from "./auth-api";

/**
 * The sign-in page URL of `locale` that returns to `path` afterwards. `path` is an
 * internal path without the locale prefix, for example `/jobs`.
 */
export function signInHref(locale: string, path: string): string {
  return `${localePath(locale, "/sign-in")}?next=${encodeURIComponent(path)}`;
}

/** `signInHref` for the active locale. */
export function useSignInHref(): (path: string) => string {
  const locale = useLocale();
  return (path) => signInHref(locale, path);
}

export function SessionGate({
  path,
  children,
}: {
  /** This page's path without the locale prefix, to come back to after signing in. */
  path: string;
  children: (user: AccountUser) => ReactNode;
}) {
  const { session, refreshSession } = useAccount();
  const { t } = useAccountText();
  const toSignIn = useSignInHref();
  switch (session.status) {
    case "loading":
      return (
        <p role="status" className="text-sm text-muted-foreground">
          {t("session.checking")}
        </p>
      );
    case "error":
      return (
        <Alert variant="destructive">
          <AlertTitle>{t("session.error_title")}</AlertTitle>
          <AlertDescription>{session.message}</AlertDescription>
          <div className="pt-2">
            <Button variant="outline" onClick={() => void refreshSession()}>
              {t("actions.try_again")}
            </Button>
          </div>
        </Alert>
      );
    case "signed-out":
      return (
        <div className="flex flex-col items-start gap-3">
          <p>{t("session.signed_out")}</p>
          <a href={toSignIn(path)} className={buttonVariants()}>
            {t("actions.sign_in")}
          </a>
        </div>
      );
    case "signed-in":
      return <>{children(session.user)}</>;
  }
}
