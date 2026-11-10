"use client";

// Frame of the account pages. Without configured cloud origins it shows only
// a "cloud not configured" notice, so a local-only build never calls a server.

import { Alert, AlertDescription, AlertTitle } from "@ariadshift/ui/components/alert";
import { buttonVariants } from "@ariadshift/ui/components/button";
import { CloudOffIcon } from "lucide-react";
import { usePathname } from "next/navigation";
import type { ReactNode } from "react";
import { splitLocalePath } from "@/i18n/paths";
import { useLocalePath } from "@/i18n/use-locale-path";
import { type AccountCloudConfig, cloudConfig, displayHost } from "@/lib/cloud/config";
import { AccountProvider, useAccount } from "./account-context";
import { useAccountText } from "./account-ui";
import type { AuthApi } from "./auth-api";
import { useSignInHref } from "./session-gate";

const SECTIONS = [
  { path: "/account", label: "account" },
  { path: "/api-keys", label: "api_keys" },
  { path: "/jobs", label: "jobs" },
] as const;

export function CloudNotConfigured() {
  const { t } = useAccountText();
  const toLocale = useLocalePath();
  return (
    <Alert>
      <CloudOffIcon aria-hidden="true" />
      <AlertTitle>{t("shell.not_configured_title")}</AlertTitle>
      <AlertDescription>
        <p>
          {t.rich("shell.not_configured_body", {
            code: (chunks) => <code>{chunks}</code>,
            api_variable: "NEXT_PUBLIC_ARIAD_API_URL",
            auth_variable: "NEXT_PUBLIC_ARIAD_AUTH_URL",
          })}
        </p>
        <p>
          {t.rich("shell.not_configured_convert", {
            link: (chunks) => <a href={toLocale("/app")}>{chunks}</a>,
          })}
        </p>
      </AlertDescription>
    </Alert>
  );
}

function SessionLink() {
  const { session } = useAccount();
  const { t } = useAccountText();
  const toSignIn = useSignInHref();
  const pathname = usePathname();
  if (session.status !== "signed-in") {
    return (
      <a
        href={toSignIn(pathname ? splitLocalePath(pathname).path : "/account")}
        className="text-sm text-primary underline-offset-4 hover:underline"
      >
        {t("actions.sign_in")}
      </a>
    );
  }
  return (
    <span className="text-sm text-muted-foreground">
      {t.rich("shell.signed_in_user", {
        email: session.user.email,
        user: (chunks) => <span className="font-medium text-foreground">{chunks}</span>,
      })}
    </span>
  );
}

function AccountNavigation() {
  const pathname = usePathname();
  const { config } = useAccount();
  const { t } = useAccountText();
  const toLocale = useLocalePath();
  const currentPath = pathname ? splitLocalePath(pathname).path : null;
  return (
    <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border pb-3">
      <nav aria-label={t("shell.nav_label")}>
        <ul className="flex flex-wrap gap-1">
          {SECTIONS.map((section) => {
            const current = currentPath === section.path;
            return (
              <li key={section.path}>
                <a
                  href={toLocale(section.path)}
                  aria-current={current ? "page" : undefined}
                  className={buttonVariants({ variant: current ? "secondary" : "ghost" })}
                >
                  {t(`shell.nav.${section.label}`)}
                </a>
              </li>
            );
          })}
        </ul>
      </nav>
      <div className="flex flex-col items-end gap-0.5">
        <SessionLink />
        <span className="text-xs text-muted-foreground">
          {t("shell.server", { host: displayHost(config.apiUrl) })}
        </span>
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
