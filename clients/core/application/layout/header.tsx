import { Boxes } from "lucide-react";
import Link from "next/link";

export function Header() {
  return (
    <header className="sticky top-0 z-50 border-b border-border bg-background/80 backdrop-blur">
      <div className="container flex h-16 items-center justify-between">
        <Link href="/" className="flex items-center gap-2 font-serif text-lg">
          <Boxes className="size-5 text-main-accent-t1" />
          <span>EV Fund</span>
        </Link>
        <nav className="flex items-center gap-6 text-sm text-muted-foreground">
          <Link href="/" className="hover:text-foreground">
            Home
          </Link>
          {/* Page-level microfrontends from other services mount under their own
              path (e.g. /risk), resolved at runtime from the MFE registry. */}
          <span className="opacity-50">Services</span>
        </nav>
      </div>
    </header>
  );
}
