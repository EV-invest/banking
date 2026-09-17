"use client";

import { useT } from "@evinvest/i18n/react";

import { useUnreadCount } from "@/entities/notification/model/notification-store";
import { KycStatusDot } from "@/features/kyc";

// The one corner the Account tab has, and the two facts that want it: the unread count
// (the rail shows it on Notifications) and the verification state (the rail shows it on
// Profile, #395). On a 20px icon they cannot share the corner, so the count wins — it is
// the fact that changes while the reader is away, and the one a tap on the tab resolves;
// the dot is standing state that Profile, one tap further, states in full. So the dot is
// drawn only while there is nothing unread.
//
// The count reads the shared store; the rail is mounted (CSS-hidden) below `lg` too, so
// its polling feeds this badge and nothing here has to poll a second time.
//
// Sits in the icon's top-right corner; the caller's icon wrapper is `relative`.
export function AccountTabBadge() {
  const t = useT();
  const unread = useUnreadCount();
  if (!unread) return <KycStatusDot className="absolute -right-1 -top-0.5" />;
  return (
    // Capped at 99+ like the rail pill: the tab has even less room. The accessible name
    // carries the real number, the one piece of information the cap throws away.
    <span
      aria-label={t("notif.unreadCount", { n: unread })}
      className="absolute -right-2.5 -top-1.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-primary px-1 text-xs font-semibold leading-none tabular-nums text-on-primary"
    >
      {unread > 99 ? "99+" : unread}
    </span>
  );
}
