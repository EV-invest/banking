"use client";

import { Loader2, ShieldBan, ShieldCheck, TriangleAlert, X } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useState } from "react";

import { Button, Card, CardContent, Input, Skeleton } from "@evinvest/uikit";

import { fetchUser, fetchUserBalance, fetchUsers, reinstateUser, revokeSessions, setKycLevel, setUserRole, suspendUser, type UserFilters } from "@/entities/admin/api/admin-client";
import type { AdminUserProfile, AdminUserSummary, UserBalance } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { TipAnchor, type TipKey } from "@/shared/tips";
import { ROLES, ago, statusTone, usd } from "@/views/admin/lib/format";
import { AdminHeader, StatusDot } from "@/views/admin/ui/shell";

export function UsersView() {
  const [filters, setFilters] = useState<UserFilters>({});
  const [users, setUsers] = useState<AdminUserSummary[] | null>(null);
  const [total, setTotal] = useState("0");
  const [selected, setSelected] = useState<AdminUserSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Bumped after a drawer mutation to re-run the list fetch below.
  const [refresh, setRefresh] = useState(0);

  // Filter/refresh-driven fetch with an ordering guard: a slower earlier response
  // must not clobber a newer one (search fires per keystroke).
  useEffect(() => {
    let active = true;
    fetchUsers(filters)
      .then((list) => {
        if (!active) return;
        setUsers(list.users ?? []);
        setTotal(list.total ?? "0");
      })
      .catch((e: Error) => active && setError(e.message));
    return () => {
      active = false;
    };
  }, [filters, refresh]);

  return (
    <div className="space-y-6 px-8 pb-10 pt-6">
      <AdminHeader eyebrow="Administer" title="Users" subtitle="Investors and operators — identities, KYC, roles and sessions" />

      {error && (
        <p className="flex items-center gap-2 text-sm text-destructive">
          <TriangleAlert className="size-4" /> {error}
        </p>
      )}

      <div className="flex flex-wrap items-center gap-3">
        <Input
          placeholder="Search email or user id…"
          className="max-w-xs"
          defaultValue={filters.query ?? ""}
          onChange={(e) => setFilters((f) => ({ ...f, query: e.target.value || undefined }))}
        />
        <FilterSelect label="Role" value={filters.role} onChange={(role) => setFilters((f) => ({ ...f, role }))} options={ROLES} />
        <FilterSelect label="Status" value={filters.status} onChange={(status) => setFilters((f) => ({ ...f, status }))} options={["active", "disabled"]} />
        <span className="ml-auto text-sm text-muted-foreground">{Number(total).toLocaleString("en-US")} users</span>
      </div>

      <div className="flex gap-6">
        <Card className="min-w-0 flex-1">
          <CardContent className="p-0">
            {!users ? (
              <div className="p-6">
                <Skeleton className="h-64 w-full" />
              </div>
            ) : users.length === 0 ? (
              <p className="p-8 text-center text-sm text-muted-foreground">No users match these filters.</p>
            ) : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                    <th className="px-5 py-3 font-medium">User</th>
                    <th className="px-5 py-3 font-medium">Role</th>
                    <th className="px-5 py-3 font-medium">KYC</th>
                    <th className="px-5 py-3 font-medium">Status</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border">
                  {users.map((u) => (
                    <tr
                      key={u.user_id}
                      onClick={() => setSelected(u)}
                      className={cn("cursor-pointer transition-colors hover:bg-foreground/[0.03]", selected?.user_id === u.user_id && "bg-main-accent-t1/[0.06]")}
                    >
                      <td className="px-5 py-3">
                        <div className="flex items-center gap-3">
                          <Avatar email={u.email} />
                          <span className="min-w-0 truncate">{u.email || u.user_id.slice(0, 8)}</span>
                        </div>
                      </td>
                      <td className="px-5 py-3 capitalize">{u.role}</td>
                      <td className="px-5 py-3 text-muted-foreground">L{u.kyc_level}</td>
                      <td className="px-5 py-3">
                        <StatusDot status={u.status} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </CardContent>
        </Card>

        {selected && (
          // `key` remounts the drawer per user, so its uncontrolled inputs (KYC level)
          // reset — otherwise a stale value could be committed against the wrong user.
          <UserDrawer key={selected.user_id} summary={selected} onClose={() => setSelected(null)} onChanged={() => setRefresh((n) => n + 1)} />
        )}
      </div>
    </div>
  );
}

function FilterSelect({ label, value, onChange, options }: { label: string; value?: string; onChange: (v: string | undefined) => void; options: readonly string[] }) {
  return (
    <label className="inline-flex items-center gap-2 text-sm">
      <span className="text-muted-foreground">{label}:</span>
      <select
        value={value ?? ""}
        onChange={(e) => onChange(e.target.value || undefined)}
        className="rounded-md border border-border bg-main-surface px-2 py-1.5 text-sm capitalize outline-none focus:border-main-accent-t1"
      >
        <option value="">All</option>
        {options.map((o) => (
          <option key={o} value={o} className="capitalize">
            {o}
          </option>
        ))}
      </select>
    </label>
  );
}

function Avatar({ email }: { email: string }) {
  const initials = (email.split("@")[0] ?? "?")
    .split(/[._-]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((p) => p[0]?.toUpperCase() ?? "")
    .join("");
  return <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-main-accent-t1/15 text-[11px] font-semibold text-main-accent-t1">{initials || "?"}</span>;
}

function UserDrawer({ summary, onClose, onChanged }: { summary: AdminUserSummary; onClose: () => void; onChanged: () => void }) {
  const [profile, setProfile] = useState<AdminUserProfile | null>(null);
  const [balance, setBalance] = useState<UserBalance | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(() => {
    fetchUser(summary.user_id)
      .then(setProfile)
      .catch((e: Error) => setError(e.message));
    fetchUserBalance(summary.user_id)
      .then(setBalance)
      .catch(() => setBalance(null));
  }, [summary.user_id]);

  // The parent gives this drawer a `key` per user, so it remounts (state starts null) —
  // no synchronous reset needed here; `reload` only sets state in its async callbacks.
  useEffect(() => {
    reload();
  }, [reload]);

  const run = async (key: string, fn: () => Promise<unknown>) => {
    setBusy(key);
    setError(null);
    try {
      await fn();
      reload();
      onChanged();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(null);
    }
  };

  const status = profile?.status ?? summary.status;
  const role = profile?.role ?? summary.role;

  return (
    <Card className="w-[340px] shrink-0 self-start">
      <CardContent className="space-y-5 py-5">
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0">
            <p className="truncate font-semibold">{summary.email || summary.user_id.slice(0, 12)}</p>
            <p className="truncate text-xs text-muted-foreground">{summary.user_id}</p>
          </div>
          <button type="button" onClick={onClose} aria-label="Close" className="text-muted-foreground hover:text-foreground">
            <X className="size-4" />
          </button>
        </div>

        <div className="flex flex-wrap gap-1.5 text-xs">
          <Badge>{role}</Badge>
          <Badge>KYC L{profile?.kyc_level ?? summary.kyc_level}</Badge>
          <span className={cn("rounded-full px-2 py-0.5 font-medium capitalize", statusTone(status))}>{status}</span>
        </div>

        {error && (
          <p className="flex items-center gap-2 text-xs text-destructive">
            <TriangleAlert className="size-3.5" /> {error}
          </p>
        )}

        <Section title="Identity">
          <Row label="Joined" value={ago(summary.created_at)} />
          <Row label="Token version" value={`v${profile?.token_version ?? summary.token_version}`} tip="admin.users.identity.token-version" />
          <Row label="Balance" value={balance ? `${usd(balance.amount)} USDT` : "—"} />
        </Section>

        <Section title="Access & security">
          <label className="flex items-center justify-between gap-2 py-1 text-sm">
            <span className="flex items-center gap-1.5 text-muted-foreground">
              Role
              <TipAnchor anchor="admin.users.access.role" />
            </span>
            <select
              value={role}
              disabled={busy === "role"}
              onChange={(e) => run("role", () => setUserRole(summary.user_id, e.target.value))}
              className="rounded-md border border-border bg-main-surface px-2 py-1 text-sm capitalize outline-none focus:border-main-accent-t1"
            >
              {ROLES.map((r) => (
                <option key={r} value={r} className="capitalize">
                  {r}
                </option>
              ))}
            </select>
          </label>
          <label className="flex items-center justify-between gap-2 py-1 text-sm">
            <span className="flex items-center gap-1.5 text-muted-foreground">
              KYC level
              <TipAnchor anchor="admin.users.access.kyc-level" />
            </span>
            <input
              type="number"
              min={0}
              defaultValue={profile?.kyc_level ?? summary.kyc_level}
              disabled={busy === "kyc"}
              onBlur={(e) => {
                const level = Number(e.target.value);
                if (level !== (profile?.kyc_level ?? summary.kyc_level)) void run("kyc", () => setKycLevel(summary.user_id, level));
              }}
              className="w-16 rounded-md border border-border bg-main-surface px-2 py-1 text-sm outline-none focus:border-main-accent-t1"
            />
          </label>
          <Button type="button" variant="outline" size="sm" className="mt-2 w-full border-destructive/40 text-destructive hover:bg-destructive/10" disabled={busy === "revoke"} onClick={() => run("revoke", () => revokeSessions(summary.user_id))}>
            {busy === "revoke" ? <Loader2 className="size-3.5 animate-spin" /> : null}
            Revoke all sessions
          </Button>
          <p className="flex items-center gap-1.5 pt-1 text-[11px] text-muted-foreground">
            Bumps token_version — invalidates every JWT issued to this user.
            <TipAnchor anchor="admin.users.access.revoke-sessions" />
          </p>
        </Section>

        <div className="flex gap-2">
          {status === "disabled" ? (
            <Button type="button" variant="outline" size="sm" className="flex-1" disabled={busy === "status"} onClick={() => run("status", () => reinstateUser(summary.user_id))}>
              <ShieldCheck className="size-3.5" /> Reinstate
            </Button>
          ) : (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="flex-1 border-destructive/40 text-destructive hover:bg-destructive/10"
              disabled={busy === "status"}
              onClick={() => run("status", () => suspendUser(summary.user_id))}
            >
              <ShieldBan className="size-3.5" /> Suspend
            </Button>
          )}
          <TipAnchor anchor="admin.users.status.suspend" className="self-center" />
        </div>
      </CardContent>
    </Card>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="space-y-1 border-t border-border pt-4">
      <p className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground">{title}</p>
      {children}
    </div>
  );
}

function Row({ label, value, tip }: { label: string; value: string; tip?: TipKey }) {
  return (
    <div className="flex items-center justify-between py-1 text-sm">
      <span className="flex items-center gap-1.5 text-muted-foreground">
        {label}
        {tip && <TipAnchor anchor={tip} />}
      </span>
      <span className="tabular-nums">{value}</span>
    </div>
  );
}

function Badge({ children }: { children: ReactNode }) {
  return <span className="rounded-full bg-foreground/[0.06] px-2 py-0.5 font-medium capitalize text-main-mist">{children}</span>;
}
