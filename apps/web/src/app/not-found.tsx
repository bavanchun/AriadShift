import { DEFAULT_LOCALE } from "@/i18n/locales";

// Global 404 for URLs outside any locale. It renders its own document because the root
// layout only passes children through. English is the fallback: the visitor's locale is not
// known here, so the page links to the default-locale home page.
export default function GlobalNotFound() {
  return (
    <html lang="en">
      <body
        style={{
          fontFamily: "system-ui, sans-serif",
          margin: "3rem auto",
          maxWidth: "40rem",
          padding: "0 1rem",
        }}
      >
        <h1>Page not found</h1>
        <p>This page does not exist.</p>
        <p>
          <a href={`/${DEFAULT_LOCALE}`}>Go to the home page</a>
        </p>
      </body>
    </html>
  );
}
