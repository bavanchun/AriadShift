import type { Metadata } from "next";
import { JobsPanel } from "@/components/account/jobs-panel";

export const metadata: Metadata = {
  title: "Jobs",
  description: "Status, downloads and deletion of your AriadShift cloud conversions.",
};

export default function Page() {
  return <JobsPanel />;
}
