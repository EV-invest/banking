"use client";

import { useState, useEffect } from "react";
import { Button } from "@evinvest/uikit";
import { Logo } from "@/shared/ui/logo";
import { notifyPlaceholder } from "@/shared/lib/utils";

export function Header() {
  const [hasScrolled, setHasScrolled] = useState(false);

  useEffect(() => {
    const handleScroll = () => setHasScrolled(window.scrollY > 50);
    window.addEventListener("scroll", handleScroll);
    return () => window.removeEventListener("scroll", handleScroll);
  }, []);

  // 1. HEADER (Minimalist, floating)
  return (
    <header
      className={`fixed top-0 left-0 w-full z-[60] transition-all duration-500 border-b ${
        hasScrolled
          ? "bg-main-black/90 backdrop-blur-md border-main-mist/10 py-4"
          : "bg-transparent border-transparent py-6"
      }`}
    >
      {}
      <div className="container flex justify-between items-center">
        <div className="flex items-center gap-3">
          <Logo className="w-10 h-10 text-white" />
          <div className="flex flex-col">
            <span className="font-serif-display font-bold text-lg tracking-wider text-white">
              EV INVESTMENT
            </span>
            <span className="text-[9px] font-mono-tech tracking-[0.3em] text-main-accent-t1 uppercase">
              Quy Nhon Fund
            </span>
          </div>
        </div>

        <nav className="hidden md:flex items-center gap-8 font-mono-tech text-xs tracking-widest uppercase">
          <a
            href="#hero"
            className="hover:text-main-accent-t1 transition-colors"
          >
            Home
          </a>
          <a
            href="#portfolio"
            className="hover:text-main-accent-t1 transition-colors"
          >
            Portfolio
          </a>
          <a
            href="#calculator"
            className="hover:text-main-accent-t1 transition-colors"
          >
            Terminal
          </a>
          <a
            href="#research"
            className="hover:text-main-accent-t1 transition-colors"
          >
            Research
          </a>
          <a
            href="#team"
            className="hover:text-main-accent-t1 transition-colors"
          >
            Team
          </a>
        </nav>

        <div className="flex items-center gap-4">
          <Button
            variant="outline"
            className="hidden sm:inline-flex font-mono-tech text-xs tracking-wider border-main-accent-t1 text-main-accent-t1 hover:bg-main-accent-t1 hover:text-main-black transition-all duration-300 bg-transparent"
            onClick={() => notifyPlaceholder("Investor Portal Login")}
          >
            Investor Portal
          </Button>
        </div>
      </div>
    </header>
  );
}
