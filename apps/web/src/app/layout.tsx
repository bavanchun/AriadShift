import type { ReactNode } from "react";

// Every page renders under /<locale>/ and the locale layout owns <html>, so this root layout
// only passes its children through. The static root page and the global 404 render their
// own document.
export default function RootLayout({ children }: { children: ReactNode }) {
  return children;
}
