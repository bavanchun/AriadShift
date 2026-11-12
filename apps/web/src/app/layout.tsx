import type { Metadata } from "next";
import type { ReactNode } from "react";
import { SITE_URL } from "@/lib/site";

// `metadataBase` resolves relative Open Graph URLs for the pages that have no locale layout
// of their own, such as the static root redirect; without it they point at localhost.
export const metadata: Metadata = { metadataBase: new URL(SITE_URL) };

// Every page renders under /<locale>/ and the locale layout owns <html>, so this root layout
// only passes its children through. The static root page and the global 404 render their
// own document.
export default function RootLayout({ children }: { children: ReactNode }) {
  return children;
}
