import type { Metadata } from "next";
import { AccountPage } from "@/components/account/account-page";

export const metadata: Metadata = {
  title: "Account",
  description: "Your AriadShift cloud account: profile, password, passkeys and two-factor sign-in.",
};

export default function Page() {
  return <AccountPage />;
}
