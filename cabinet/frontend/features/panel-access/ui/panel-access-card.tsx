"use client";

import { TriangleAlert } from "lucide-react";
import { type ReactNode, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Card, CardContent, Skeleton } from "@evinvest/uikit";

import { grantScope, revokeScope } from "@/entities/scope/api/scope-client";
import { canRevokeHolder, grantableRoles, type ScopeRole } from "@/entities/scope/lib/access";
import { scopeErrorKey, type ScopeAction } from "@/entities/scope/lib/errors";
import { scopeHoldersResource } from "@/entities/scope/model/scope-resource";
import { usePanelViewer } from "@/features/panel-access/model/use-panel-viewer";
import { GrantScopeForm } from "@/features/panel-access/ui/grant-scope-form";
import { RevokeScopeDialog } from "@/features/panel-access/ui/revoke-scope-dialog";
import { ScopeHoldersTable } from "@/features/panel-access/ui/scope-holders-table";
import type { ScopeHolder } from "@/shared/contracts";
import { RequestError, errorMessage } from "@/shared/lib/api-client";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { Settled } from "@/shared/ui/motion";

// Who may open this allocation's panel — the identity plane's `allocation:<service>`
// scope. Not the money grants beside it in the registry: nothing here moves or shows funds.
export function PanelAccessCard({ service, header, className }: { service: string; header?: ReactNode; className?: string }) {
  const t = useT();
  const { role } = usePanelViewer();
  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<ScopeHolder | null>(null);

  const read = useResource(scopeHoldersResource, service);
  const holders = read.data?.holders ?? [];

  const describe = (e: unknown, action: ScopeAction) => {
    const key = e instanceof RequestError && e.code === null ? scopeErrorKey(e.status, action) : null;
    return key ? t(key) : errorMessage(e, t);
  };
  const error = actionError ?? (read.data || !read.error ? null : describe(read.error, "list"));

  const run = async (key: string, action: ScopeAction, fn: () => Promise<unknown>) => {
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

  const grant = (email: string, scopeRole: ScopeRole) => run("grant", "grant", () => grantScope(service, email, scopeRole));
  const revoke = (holder: ScopeHolder) => {
    setConfirming(null);
    const userId = holder.grant?.user_id ?? "";
    void run(`revoke:${userId}`, "revoke", () => revokeScope(service, userId));
  };

  return (
    <Card className={cn("w-85", className)}>
      <CardContent className="space-y-5 py-5">
        {header}

        {error && (
          <p role="alert" className="flex items-center gap-2 text-xs text-accent-error">
            <TriangleAlert className="size-3.5 shrink-0" /> {error}
          </p>
        )}

        <GrantScopeForm roles={grantableRoles(role)} busy={busy === "grant"} onSubmit={grant} />

        <div className="space-y-2">
          <p className="text-xs font-semibold uppercase tracking-wide text-ink-soft">{t("panelAccess.roster")}</p>
          <Settled loading={!read.data && !read.error} skeleton={<Skeleton className="h-24 w-full" />}>
            {read.data && (
              <ScopeHoldersTable
                holders={holders}
                busyUserId={busy?.startsWith("revoke:") ? busy.slice("revoke:".length) : null}
                canRevoke={(holderRole) => canRevokeHolder(role, holderRole)}
                onRevoke={setConfirming}
              />
            )}
          </Settled>
        </div>
      </CardContent>
      <RevokeScopeDialog holder={confirming} onCancel={() => setConfirming(null)} onConfirm={revoke} />
    </Card>
  );
}
