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

import type { ScopeHolder } from "@/shared/contracts";

// Revoking signs the person out of the panel on their next request, so it is confirmed —
// and the confirmation names who, since the roster row is out of sight behind the overlay.
export function RevokeScopeDialog({ holder, onCancel, onConfirm }: { holder: ScopeHolder | null; onCancel: () => void; onConfirm: (holder: ScopeHolder) => void }) {
  const t = useT();
  const who = holder?.email || holder?.grant?.user_id || "";
  return (
    <AlertDialog open={holder !== null} onOpenChange={(open) => !open && onCancel()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t("panelAccess.revokeTitle")}</AlertDialogTitle>
          <AlertDialogDescription>{t("panelAccess.revokeBody", { who })}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{t("ui.cancel")}</AlertDialogCancel>
          <AlertDialogAction onClick={() => holder && onConfirm(holder)}>{t("panelAccess.revoke")}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
