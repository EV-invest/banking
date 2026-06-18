"use client";

import { useState, useEffect, type ReactNode } from "react";
import { ASSETS } from "@/shared/config/assets";

/**
 * Client island — owns the scroll-zoom state and applies it to the background
 * and content wrapper. Static copy (heading, body) is passed as `children` from
 * the server; the CTA is passed as `cta` so it renders inside the layout without
 * pulling extra logic into this component.
 */
export function HeroACanvas({
  children,
  cta,
}: {
  children: ReactNode;
  cta: ReactNode;
}) {
  const [zoom, setZoom] = useState(1);

  useEffect(() => {
    const onScroll = () => {
      const scrollY = window.scrollY;
      const threshold = window.innerHeight;
      if (scrollY < threshold) {
        setZoom(1 + (scrollY / threshold) * 3);
      }
    };
    window.addEventListener("scroll", onScroll);
    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  return (
    <>
      <div
        className="absolute inset-0 z-0 transition-transform duration-100 ease-out"
        style={{
          transform: `scale(${zoom})`,
          backgroundImage: `linear-gradient(to bottom, rgba(7, 13, 24, 0.78), rgba(7, 13, 24, 0.96)), url(${ASSETS.quynhon_future})`,
          backgroundSize: "cover",
          backgroundPosition: "center",
        }}
      />

      <div className="absolute inset-0 bg-[linear-gradient(to_right,#8080800a_1px,transparent_1px),linear-gradient(to_bottom,#8080800a_1px,transparent_1px)] bg-[size:14px_24px] pointer-events-none" />

      <div className="container relative z-10 text-center flex flex-col items-center justify-center h-full max-w-4xl px-4">
        <div
          className="transition-all duration-700 ease-out"
          style={{
            transform: `scale(${Math.max(0.8, 1 - (zoom - 1) * 0.15)})`,
            opacity: Math.max(0.1, 1 - (zoom - 1) * 0.5),
          }}
        >
          {children}
        </div>

        <div className="flex flex-col items-center gap-4">{cta}</div>
      </div>
    </>
  );
}
