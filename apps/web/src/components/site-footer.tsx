import Link from "next/link";
import { REPOSITORY_URL } from "@/lib/site";

export function SiteFooter() {
  return (
    <footer className="mt-16 border-t border-border">
      <div className="mx-auto flex max-w-5xl flex-wrap items-center justify-between gap-4 px-4 py-6 text-sm text-muted-foreground">
        <p>AriadShift is open source under the Apache-2.0 license.</p>
        <ul className="flex flex-wrap gap-4">
          <li>
            <Link className="hover:text-foreground" href="/privacy">
              Privacy
            </Link>
          </li>
          <li>
            <Link className="hover:text-foreground" href="/quality">
              Quality
            </Link>
          </li>
          <li>
            <a className="hover:text-foreground" href={REPOSITORY_URL}>
              Source code
            </a>
          </li>
        </ul>
      </div>
    </footer>
  );
}
