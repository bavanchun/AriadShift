"use client";

// Shared state of the account pages: the cloud origins, the auth client, the
// current session, and the page's two live regions (polite for progress and
// results, assertive for failures).

import { useTranslations } from "next-intl";
import type { ReactNode } from "react";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { AccountCloudConfig } from "@/lib/cloud/config";
import { type AccountUser, type AuthApi, createBetterAuthApi } from "./auth-api";

export type SessionState =
  | { status: "loading" }
  | { status: "signed-out" }
  | { status: "signed-in"; user: AccountUser }
  | { status: "error"; message: string };

export interface AccountContextValue {
  config: AccountCloudConfig;
  auth: AuthApi;
  session: SessionState;
  refreshSession: () => Promise<void>;
  /** Announces a message to screen readers without moving focus. */
  announce: (message: string, urgency?: "polite" | "assertive") => void;
}

const AccountContext = createContext<AccountContextValue | null>(null);

export function useAccount(): AccountContextValue {
  const value = useContext(AccountContext);
  if (!value) {
    throw new Error("useAccount must be used inside AccountProvider");
  }
  return value;
}

export interface AccountProviderProps {
  config: AccountCloudConfig;
  /** Defaults to Better Auth against `config.authUrl`. */
  auth?: AuthApi;
  children: ReactNode;
}

interface Announcement {
  message: string;
  /** Changes on every announcement, so repeating a message re-announces it. */
  key: number;
}

export function AccountProvider({ config, auth: customAuth, children }: AccountProviderProps) {
  const auth = useMemo(
    () => customAuth ?? createBetterAuthApi(config.authUrl),
    [customAuth, config.authUrl],
  );
  const t = useTranslations("web.account");
  // Read when a failure happens, so a changed translator never refetches the session.
  const unreachable = useRef("");
  unreachable.current = t("errors.service_unreachable");
  const [session, setSession] = useState<SessionState>({ status: "loading" });
  const [polite, setPolite] = useState<Announcement>({ message: "", key: 0 });
  const [assertive, setAssertive] = useState<Announcement>({ message: "", key: 0 });

  const refreshSession = useCallback(async () => {
    try {
      const user = await auth.getSession();
      setSession(user ? { status: "signed-in", user } : { status: "signed-out" });
    } catch {
      setSession({
        status: "error",
        message: unreachable.current,
      });
    }
  }, [auth]);

  useEffect(() => {
    void refreshSession();
  }, [refreshSession]);

  const announce = useCallback((message: string, urgency: "polite" | "assertive" = "polite") => {
    const set = urgency === "assertive" ? setAssertive : setPolite;
    set((previous) => ({ message, key: previous.key + 1 }));
  }, []);

  const value = useMemo(
    () => ({ config, auth, session, refreshSession, announce }),
    [config, auth, session, refreshSession, announce],
  );

  return (
    <AccountContext.Provider value={value}>
      {children}
      <div className="sr-only" aria-live="polite" aria-atomic="true">
        <span key={polite.key}>{polite.message}</span>
      </div>
      <div className="sr-only" aria-live="assertive" aria-atomic="true">
        <span key={assertive.key}>{assertive.message}</span>
      </div>
    </AccountContext.Provider>
  );
}
