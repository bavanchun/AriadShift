"use client";

import { AccountActionsSection } from "./account-actions-section";
import { useAccountText } from "./account-ui";
import { ConnectedAppsSection } from "./connected-apps-section";
import { PasskeysSection } from "./passkeys-section";
import { PasswordSection } from "./password-section";
import { ProfileSection } from "./profile-section";
import { SessionGate } from "./session-gate";
import { TwoFactorSection } from "./two-factor-section";

export function AccountPage({ navigate }: { navigate?: (path: string) => void }) {
  const { t } = useAccountText();
  return (
    <div className="flex flex-col gap-6">
      <h1 className="text-3xl font-bold tracking-tight">{t("page.title")}</h1>
      <SessionGate path="/account">
        {(user) => (
          <div className="flex max-w-2xl flex-col gap-6">
            <ProfileSection user={user} />
            <PasswordSection />
            <PasskeysSection />
            <TwoFactorSection user={user} />
            <ConnectedAppsSection />
            <AccountActionsSection navigate={navigate} />
          </div>
        )}
      </SessionGate>
    </div>
  );
}
