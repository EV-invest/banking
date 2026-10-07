"use client";

import { useT } from "@evinvest/i18n/react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@evinvest/uikit";

import type { GrantHolder } from "@/shared/contracts";

// Revoking takes effect in the panel on the person's next request, so it is confirmed — and
// the confirmation names who and what, since the roster row is out of sight behind the overlay.
export function RevokeGrantDialog({ holder, onCancel, onConfirm }: { holder: GrantHolder | null; onCancel: () => void; onConfirm: (holder: GrantHolder) => void }) {
  const t = useT();
  const who = holder?.email || holder?.grant?.user_id || "";
  const target = holder?.grant?.target ?? "";
  return (
    <AlertDialog open={holder !== null} onOpenChange={(open) => !open && onCancel()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t("panelAccess.revokeTitle", "Revoke panel access?")}</AlertDialogTitle>
          <AlertDialogDescription>{t("panelAccess.revokeBody", "{who} will lose {target}. You can grant it again later.", { who, target })}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{t("ui.cancel", "Cancel")}</AlertDialogCancel>
          <AlertDialogAction onClick={() => holder && onConfirm(holder)}>{t("panelAccess.revoke", "Revoke")}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
