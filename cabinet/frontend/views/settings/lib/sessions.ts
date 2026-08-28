// Session-list display helpers: what device a captured User-Agent describes, and the meta
// line under it.

import { Laptop, type LucideIcon, Smartphone } from "lucide-react";

import type { Session } from "@/shared/contracts";

// Best-effort device label from the captured User-Agent — enough to recognise a device,
// not a parser. Mobile UAs get the phone glyph.
export function deviceOf(ua: string | undefined): { label: string; icon: LucideIcon } {
  const u = (ua ?? "").toLowerCase();
  if (!u) return { label: "Unknown device", icon: Laptop };
  const mobile = /iphone|android|mobile/.test(u);
  const browser = /edg/.test(u) ? "Edge" : /firefox|fxios/.test(u) ? "Firefox" : /chrome|crios/.test(u) ? "Chrome" : /safari/.test(u) ? "Safari" : "Browser";
  const os = /iphone|ipad|ios|crios|fxios/.test(u) ? "iOS" : /android/.test(u) ? "Android" : /mac os|macintosh/.test(u) ? "macOS" : /windows/.test(u) ? "Windows" : /linux/.test(u) ? "Linux" : "device";
  return { label: `${browser} · ${os}`, icon: mobile ? Smartphone : Laptop };
}

export function metaOf(s: Session, name: string): string {
  const ip = (s.ip ?? "").trim();
  const when = s.current ? "Active now" : lastSeen(s.last_seen);
  return [name, ip || null, when].filter(Boolean).join(" · ");
}

function lastSeen(value: number | string | undefined): string {
  const secs = Number(value ?? 0);
  if (!Number.isFinite(secs) || secs <= 0) return "active recently";
  const diff = Date.now() - secs * 1000;
  if (diff < 60_000) return "active now";
  const mins = Math.floor(diff / 60_000);
  if (mins < 60) return `last active ${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `last active ${hrs}h ago`;
  return `last active ${Math.floor(hrs / 24)}d ago`;
}
