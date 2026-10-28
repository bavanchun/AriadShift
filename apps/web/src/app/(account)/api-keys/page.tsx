import type { Metadata } from "next";
import { ApiKeysPanel } from "@/components/account/api-keys-panel";

export const metadata: Metadata = {
  title: "API keys",
  description: "Create and revoke API keys for the AriadShift cloud API.",
};

export default function Page() {
  return <ApiKeysPanel />;
}
