// Formatting + small helpers shared by the admin-console views.

/** A decimal amount string → grouped display (no currency symbol). */
export function amount(value: string | undefined): string {
  const n = Number(value ?? "0");
  if (!Number.isFinite(n)) return value ?? "0";
  return n.toLocaleString("en-US", { maximumFractionDigits: 2 });
}

/** A decimal amount string → `$1,234.56`. */
export function usd(value: string | undefined): string {
  return `$${amount(value)}`;
}

/** A unix-seconds string → a coarse "3h ago" age (for queue/session rows). */
export function ago(unixSecs: string | undefined): string {
  const t = Number(unixSecs ?? "0");
  if (!t) return "—";
  const secs = Math.max(0, Math.floor(Date.now() / 1000) - t);
  if (secs < 60) return `${secs}s ago`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
  if (secs < 86_400) return `${Math.floor(secs / 3600)}h ago`;
  return `${Math.floor(secs / 86_400)}d ago`;
}

/** The role vocabulary, least→most privileged (matches the domain `Role`). */
export const ROLES = ["investor", "operator", "admin", "owner"] as const;

/** Tailwind token classes for a lifecycle/health status pill. */
export function statusTone(status: string): string {
  switch (status) {
    case "active":
    case "healthy":
      return "text-main-accent-t2";
    case "onboarding":
    case "degraded":
    case "staged":
      return "text-main-accent-t3";
    case "blocked":
    case "disabled":
    case "error":
      return "text-destructive";
    default:
      return "text-muted-foreground";
  }
}
