import { buttonVariants } from "@ariadshift/ui/components/button";
import Link from "next/link";

export default function NotFound() {
  return (
    <div className="flex flex-col items-start gap-4">
      <h1 className="text-3xl font-bold tracking-tight">Page not found</h1>
      <p className="text-muted-foreground">This page does not exist.</p>
      <Link href="/" className={buttonVariants()}>
        Go to the home page
      </Link>
    </div>
  );
}
