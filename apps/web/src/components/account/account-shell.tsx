"use client";

// Frame of the account pages. Without configured cloud origins it shows only
// a "cloud not configured" notice, so a local-only build never calls a server.

import { Alert, AlertDescription, AlertTitle } from "@ariadshift/ui/components/alert";
import { buttonVariants } from "@ariadshift/ui/components/button";
import { CloudOffIcon } from "lucide-react";
import { usePathname } from "next/navigation";
import type { ReactNode } from "react";
import { type AccountCloudConfig, cloudConfig, displayHost } from "@/lib/cloud/config";
import { AccountProvider, useAccount } from "./account-context";
import type { AuthApi } from "./auth-api";
import { signInHref } from "./session-gate";

const SECTIONS = [
  { href: "/account", label: "Account" },
  { href: "/api-keys", label: "API keys" },
  { href: "/jobs", label: "Jobs" },
] as const;

export function CloudNotConfigured() {
  return (
    <Alert>
      <CloudOffIcon aria-hidden="true" />
      <AlertTitle>Cloud is not configured</AlertTitle>
      <AlertDescription>
        <p>
          This copy of AriadShift runs on your device only. Accounts, API keys and cloud jobs need a
          server; its operator sets <code>NEXT_PUBLIC_ARIAD_API_URL</code> and{" "}
          <code>NEXT_PUBLIC_ARIAD_AUTH_URL</code> when building the site.
        </p>
        <p>
          You can still <a href="/app">convert files in your browser</a>.
        </p>
      </AlertDescription>
    </Alert>
  );
}

function SessionLink() {
  const { session } = useAccount();
  const pathname = usePathname();
  if (session.status !== "signed-in") {
    return (
      <a
        href={signInHref(pathname ?? "/account")}
        className="text-sm text-primary underline-offset-4 hover:underline"
      >
        Sign in
      </a>
    );
  }
  return (
    <span className="text-sm text-muted-foreground">
      Signed in as <span className="font-medium text-foreground">{session.user.email}</span>
    </span>
  );
}

function AccountNavigation() {
  const pathname = usePathname();
  const { config } = useAccount();
  return (
    <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border pb-3">
      <nav aria-label="Account">
        <ul className="flex flex-wrap gap-1">
          {SECTIONS.map((section) => {
            const current = pathname === section.href;
            return (
              <li key={section.href}>
                <a
                  href={section.href}
                  aria-current={current ? "page" : undefined}
                  className={buttonVariants({ variant: current ? "secondary" : "ghost" })}
                >
                  {section.label}
                </a>
              </li>
            );
          })}
        </ul>
      </nav>
      <div className="flex flex-col items-end gap-0.5">
        <SessionLink />
        <span className="text-xs text-muted-foreground">Server: {displayHost(config.apiUrl)}</span>
      </div>
    </div>
  );
}

export interface AccountShellProps {
  children: ReactNode;
  /** Defaults to the build's configuration. */
  config?: AccountCloudConfig | null;
  auth?: AuthApi;
}

export function AccountShell({ children, config: customConfig, auth }: AccountShellProps) {
  const config = customConfig === undefined ? cloudConfig() : customConfig;
  if (!config) {
    return <CloudNotConfigured />;
  }
  return (
    <AccountProvider config={config} auth={auth}>
      <div className="flex flex-col gap-6">
        <AccountNavigation />
        {children}
      </div>
    </AccountProvider>
  );
}
