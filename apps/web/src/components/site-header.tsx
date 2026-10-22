import { ThemeToggle } from "@ariadshift/ui/components/theme-toggle";
import { NAVIGATION, SITE_NAME } from "@/lib/site";

// Plain links, not client-side navigation: /app is cross-origin isolated by its response
// headers, which only apply to a document loaded from it, and pages left from /app must
// not stay isolated.
export function SiteHeader() {
  return (
    <header className="border-b border-border">
      <div className="mx-auto flex max-w-5xl flex-wrap items-center gap-x-6 gap-y-2 px-4 py-3">
        <a href="/" className="flex items-center gap-2 font-bold tracking-tight">
          <img src="/brand/ariadshift-app-icon.svg" alt="" width={32} height={32} />
          <span className="text-lg">{SITE_NAME}</span>
        </a>
        <nav aria-label="Main" className="order-3 w-full sm:order-none sm:w-auto">
          <ul className="flex flex-wrap gap-x-4 gap-y-1 text-sm">
            {NAVIGATION.map((item) => (
              <li key={item.href}>
                <a
                  href={item.href}
                  className="rounded-sm text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring"
                >
                  {item.label}
                </a>
              </li>
            ))}
          </ul>
        </nav>
        <ThemeToggle className="ml-auto" />
      </div>
    </header>
  );
}
