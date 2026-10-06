"use client";

import { TriangleAlert } from "lucide-react";
import { type ReactNode, useState } from "react";

import type { Translate } from "@evinvest/i18n";
import { useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, Button, Card, CardContent, Skeleton } from "@evinvest/uikit";

import { grantPermission, revokePermission } from "@/entities/grant/api/grant-client";
import { type GrantAction, grantErrorKey, type GrantErrorKey } from "@/entities/grant/lib/errors";
import { grantHoldersResource } from "@/entities/grant/model/grant-resource";
import { GrantForm } from "@/features/panel-access/ui/grant-form";
import { GrantHoldersTable } from "@/features/panel-access/ui/grant-holders-table";
import { RevokeGrantDialog } from "@/features/panel-access/ui/revoke-grant-dialog";
import type { GrantHolder } from "@/shared/contracts";
import { RequestError, errorMessage } from "@/shared/lib/api-client";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { Settled } from "@/shared/ui/motion";

const grantErrorWords = (t: Translate): Record<GrantErrorKey, string> => ({
  "panelAccess.err.invalid": t("panelAccess.err.invalid", "Check the email address and what you're granting, then try again."),
  "panelAccess.err.forbiddenList": t("panelAccess.err.forbiddenList", "You don't have permission to manage access to this panel."),
  "panelAccess.err.forbidden": t("panelAccess.err.forbidden", "You can't make this change. You can grant and revoke only what your own access lets you hand on."),
  "panelAccess.err.notHolder": t("panelAccess.err.notHolder", "This person no longer holds this access."),
  "panelAccess.err.cannotGrant": t("panelAccess.err.cannotGrant", "Access can't be granted to this address."),
  "panelAccess.err.tooMany": t("panelAccess.err.tooMany", "Too many attempts. Try again later."),
});

// What people hold in the tenant's namespace — the grants its panel gates on. Not the money
// grants beside it in the registry: nothing here moves or shows funds.
export function PanelAccessCard({ namespace, header, className }: { namespace: string; header?: ReactNode; className?: string }) {
  const t = useT();
  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<GrantHolder | null>(null);

  const read = useResource(grantHoldersResource, namespace);
  const holders = read.data?.holders ?? [];
  const known = [...new Set(holders.flatMap((h) => (h.grant?.target ? [h.grant.target] : [])))];

  const describe = (e: unknown, action: GrantAction) => {
    const key = e instanceof RequestError && e.code === null ? grantErrorKey(e.status, action) : null;
    return key ? grantErrorWords(t)[key] : errorMessage(e, t);
  };
  // A failed read belongs to the roster it failed to fill; the banner is for actions.
  const readError = read.data || !read.error ? null : describe(read.error, "list");

  const run = async (key: string, action: GrantAction, fn: () => Promise<unknown>) => {
    setBusy(key);
    setActionError(null);
    try {
      await fn();
      await read.refresh();
      return true;
    } catch (e) {
      setActionError(describe(e, action));
      return false;
    } finally {
      setBusy(null);
    }
  };

  const grant = (email: string, target: string) => run("grant", "grant", () => grantPermission(namespace, email, target));
  const revoke = (holder: GrantHolder) => {
    setConfirming(null);
    const { id, user_id: userId = "", target = "" } = holder.grant ?? {};
    void run(`revoke:${String(id ?? "")}`, "revoke", () => revokePermission(namespace, userId, target));
  };

  return (
    <Card className={cn("w-85", className)}>
      <CardContent className="space-y-5 py-5">
        {header}

        {actionError && (
          <Alert variant="destructive">
            <TriangleAlert className="size-4" />
            <AlertDescription>{actionError}</AlertDescription>
          </Alert>
        )}

        <GrantForm namespace={namespace} known={known} busy={busy === "grant"} onSubmit={grant} />

        <div className="space-y-2">
          <p className="text-xs font-semibold uppercase tracking-wide text-ink-soft">{t("panelAccess.roster", "Who has access")}</p>
          <Settled loading={!read.data && !read.error} skeleton={<Skeleton className="h-24 w-full" />}>
            {readError && (
              <Alert variant="destructive">
                <TriangleAlert className="size-4" />
                <AlertDescription className="flex flex-col items-start gap-2">
                  {readError}
                  <Button type="button" variant="outline" size="sm" onClick={() => void read.refresh()}>
                    {t("status.tryAgain", "Try again")}
                  </Button>
                </AlertDescription>
              </Alert>
            )}
            {read.data && (
              <GrantHoldersTable holders={holders} busyGrantId={busy?.startsWith("revoke:") ? busy.slice("revoke:".length) : null} onRevoke={setConfirming} />
            )}
          </Settled>
        </div>
      </CardContent>
      <RevokeGrantDialog holder={confirming} onCancel={() => setConfirming(null)} onConfirm={revoke} />
    </Card>
  );
}
