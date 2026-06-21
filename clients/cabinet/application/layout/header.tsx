import { Boxes } from "lucide-react";
import Link from "next/link";

import { UserMenu } from "@/application/layout/user-menu";

export function Header() {
  return (
    <header className="sticky top-0 z-50 border-b border-border bg-background/80 backdrop-blur">
      <div className="container flex h-16 items-center justify-between">
        <Link href="/" className="flex items-center gap-2 font-serif text-lg">
          <Boxes className="size-5 text-main-accent-t1" />
          <span>EV Banking</span>
        </Link>
        <nav className="flex items-center gap-6 text-sm text-muted-foreground">
          <Link href="/" className="hover:text-foreground">
            Home
          </Link>
          <Link href="/wallet" className="hover:text-foreground">
            Wallet
          </Link>
          {/* Page-level microfrontends from other services mount under their own
              path (e.g. /risk), resolved at runtime from the MFE registry. */}
          <span className="opacity-50">Services</span>
          <UserMenu />
        </nav>
      </div>
    </header>
  );
}
