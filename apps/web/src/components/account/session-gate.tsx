"use client";

// Renders its children for a signed-in user, and otherwise says why it cannot.

import { Alert, AlertDescription, AlertTitle } from "@ariadshift/ui/components/alert";
import { Button, buttonVariants } from "@ariadshift/ui/components/button";
import type { ReactNode } from "react";
import { useAccount } from "./account-context";
import type { AccountUser } from "./auth-api";

/** The sign-in page URL that returns to `path` afterwards. */
export function signInHref(path: string): string {
  return `/sign-in?next=${encodeURIComponent(path)}`;
}

export function SessionGate({
  path,
  children,
}: {
  /** This page's path, to come back to after signing in. */
  path: string;
  children: (user: AccountUser) => ReactNode;
}) {
  const { session, refreshSession } = useAccount();
  switch (session.status) {
    case "loading":
      return (
        <p role="status" className="text-sm text-muted-foreground">
          Checking your session…
        </p>
      );
    case "error":
      return (
        <Alert variant="destructive">
          <AlertTitle>Cannot check your session</AlertTitle>
          <AlertDescription>{session.message}</AlertDescription>
          <div className="pt-2">
            <Button variant="outline" onClick={() => void refreshSession()}>
              Try again
            </Button>
          </div>
        </Alert>
      );
    case "signed-out":
      return (
        <div className="flex flex-col items-start gap-3">
          <p>You are not signed in.</p>
          <a href={signInHref(path)} className={buttonVariants()}>
            Sign in
          </a>
        </div>
      );
    case "signed-in":
      return <>{children(session.user)}</>;
  }
}
