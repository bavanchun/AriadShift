import type { Metadata } from "next";
import { SignInPanel } from "@/components/account/sign-in-panel";

export const metadata: Metadata = {
  title: "Sign in",
  description: "Sign in to your AriadShift cloud account or create one.",
};

export default function Page() {
  return <SignInPanel />;
}
