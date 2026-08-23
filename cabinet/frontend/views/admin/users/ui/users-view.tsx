"use client";

import { Loader2, ShieldBan, ShieldCheck, TriangleAlert, X } from "lucide-react";
import { type ReactNode, useState } from "react";

import { Button, Card, CardContent, Input, Select, SelectContent, SelectItem, SelectTrigger, Skeleton } from "@evinvest/uikit";

import { reinstateUser, revokeSessions, setKycLevel, setUserRole, suspendUser, type UserFilters } from "@/entities/admin/api/admin-client";
import { adminUserBalanceResource, adminUserResource, usersResource } from "@/entities/admin/model/admin-resource";
import type { AdminUserSummary } from "@/shared/contracts/admin";
import { TAG } from "@/shared/lib/cache-tags";
import { cn } from "@/shared/lib/cn";
import { revalidateTag, useResource } from "@/shared/lib/resource";
import { Panel, PanelPresence, PanelSwap, Settled, StaggerItem } from "@/shared/ui/motion";
import { TipAnchor, type TipKey } from "@/shared/tips";
import { ROLES, ago, formatUsd, statusTone } from "@/views/admin/lib/format";
import { AdminHeader, AdminScreen, StatusDot } from "@/views/admin/ui/shell";

export function UsersView() {
  const [filters, setFilters] = useState<UserFilters>({});
  const [selected, setSelected] = useState<AdminUserSummary | null>(null);

  // One cache entry per filter set, which also settles the ordering hazard the manual
  // fetch guarded by hand: a search fires per keystroke, and a slower earlier response now
  // lands on its own key instead of clobbering a newer one. Retyping a search already made
  // answers from cache.
  const list = useResource(usersResource, filters);
  const users = list.data ? (list.data.users ?? []) : null;
  const total = list.data?.total ?? "0";
  const error = users ? null : (list.error?.message ?? null);

  return (
    <AdminScreen className="space-y-6">
      <AdminHeader eyebrow="Administer" title="Users" subtitle="Investors and operators — identities, KYC, roles and sessions" />

      {error && (
        <StaggerItem as="p" className="flex items-center gap-2 text-sm text-destructive">
          <TriangleAlert className="size-4" /> {error}
        </StaggerItem>
      )}

      <StaggerItem className="flex flex-wrap items-center gap-3">
        <Input
          placeholder="Search email or user id…"
          className="max-w-xs"
          defaultValue={filters.query ?? ""}
          onChange={(e) => setFilters((f) => ({ ...f, query: e.target.value || undefined }))}
        />
        <FilterSelect label="Role" value={filters.role} onChange={(role) => setFilters((f) => ({ ...f, role }))} options={ROLES} />
        <FilterSelect label="Status" value={filters.status} onChange={(status) => setFilters((f) => ({ ...f, status }))} options={["active", "disabled"]} />
        <span className="ml-auto text-sm text-muted-foreground">{Number(total).toLocaleString("en-US")} users</span>
      </StaggerItem>

      {/* Table and drawer are one section: the drawer's open/close already owns the
          width of this row, and a second entrance on the same element would be
          animating against it. */}
      <StaggerItem className="flex gap-6">
        <Card className="min-w-0 flex-1">
          <CardContent className="p-0">
            <Settled
              loading={!users}
              skeleton={
                <div className="p-6">
                  <Skeleton className="h-64 w-full" />
                </div>
              }
            >
              {!users ? null : users.length === 0 ? (
                <p className="p-8 text-center text-sm text-muted-foreground">No users match these filters.</p>
              ) : (
                // `table-fixed` is load-bearing, not tidiness. Under the default
                // auto layout a column is as wide as its content, so `truncate` on
                // an address never engages: when the drawer opens and takes 340px
                // the browser compresses the columns instead, the longer addresses
                // wrap onto a second line, and every row grows taller — the table
                // stretched vertically for the whole width animation. Fixed layout
                // sizes the columns from the header row, so narrowing shortens the
                // addresses and the row heights never move. The other three columns
                // split what User leaves.
                <table className="w-full table-fixed text-sm">
                  <thead>
                    <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                      <th className="w-1/2 px-5 py-3 font-medium">User</th>
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
                        className={cn("cursor-pointer transition-colors hover:bg-foreground/5", selected?.user_id === u.user_id && "bg-main-accent-t1/10")}
                      >
                        <td className="px-5 py-3">
                          {/* The row is clickable for the mouse, but the identity cell carries the
                              real control: a bare `tr onClick` gives the keyboard no way in, and
                              the address is what names the row being opened. */}
                          <button
                            type="button"
                            aria-pressed={selected?.user_id === u.user_id}
                            onClick={() => setSelected(u)}
                            className="flex min-w-0 items-center gap-3 rounded-md text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
                          >
                            <Avatar email={u.email} />
                            <span className="min-w-0 truncate">{u.email || u.user_id.slice(0, 8)}</span>
                          </button>
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
            </Settled>
          </CardContent>
        </Card>

        {/* Three distinct motions, and they are not interchangeable: opening slides
            the panel in from the right, closing plays that in reverse (which needs
            the node to outlive the state change — hence PanelPresence), and picking
            a different row while the panel is already open leaves the frame where it
            is and cross-fades only the body. The panel is keyed on being open, not
            on which user, or every row click would play a full exit and enter.

            `collapse` is what makes the table widen rather than snap back: the
            drawer hands its 340px and the row's gap back over the course of the
            exit, so the table reflows with it each frame instead of jumping open
            once the drawer has already gone. `overflow-hidden` keeps the card
            inside at its own width while the wrapper narrows — without it the
            drawer's own text reflows on the way out. */}
        <PanelPresence>
          {selected && (
            <Panel
              key="user-drawer"
              // The open width lives here rather than in a class: Panel animates
              // to it, and an inline width from the animation would beat the
              // class anyway. 21.25rem is the `w-85` this used to carry.
              collapse={{ gap: "1.5rem", width: "21.25rem" }}
              className="shrink-0 self-start overflow-hidden"
            >
              <PanelSwap swapKey={selected.user_id}>
                {/* `key` remounts the drawer per user, so its uncontrolled inputs (KYC
                    level) reset — otherwise a stale value could be committed against
                    the wrong user. */}
                <UserDrawer key={selected.user_id} summary={selected} onClose={() => setSelected(null)} />
              </PanelSwap>
            </Panel>
          )}
        </PanelPresence>
      </StaggerItem>
    </AdminScreen>
  );
}

// A `div`, not a `label`: the uikit Select's trigger is a button, which a label has
// nothing to bind to. "All" stays a real, selectable item — clearing the filter has to
// be reachable — and maps back to `undefined`.
function FilterSelect({ label, value, onChange, options }: { label: string; value?: string; onChange: (v: string | undefined) => void; options: readonly string[] }) {
  return (
    <div className="inline-flex items-center gap-2 text-sm">
      <span className="text-muted-foreground">{label}:</span>
      <Select value={value ?? ""} onValueChange={(v) => onChange(v || undefined)}>
        <SelectTrigger size="sm" className="border-border bg-main-surface capitalize">
          <span className="truncate">{value ?? "All"}</span>
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="">All</SelectItem>
          {options.map((o) => (
            <SelectItem key={o} value={o} className="capitalize">
              {o}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}

function Avatar({ email }: { email: string }) {
  const initials = (email.split("@")[0] ?? "?")
    .split(/[._-]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((p) => p[0]?.toUpperCase() ?? "")
    .join("");
  return <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-main-accent-t1/15 text-xs font-semibold text-main-accent-t1">{initials || "?"}</span>;
}

function UserDrawer({ summary, onClose }: { summary: AdminUserSummary; onClose: () => void }) {
  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  // Keyed per user, so reopening a row already looked at shows its detail immediately.
  const detail = useResource(adminUserResource, summary.user_id);
  const balanceRead = useResource(adminUserBalanceResource, summary.user_id);
  const profile = detail.data ?? null;
  const balance = balanceRead.data ?? null;
  const error = actionError ?? (profile ? null : (detail.error?.message ?? null));

  const run = async (key: string, fn: () => Promise<unknown>) => {
    setBusy(key);
    setActionError(null);
    try {
      await fn();
      // A role change, a suspension or a KYC bump moves this user's detail AND the row for
      // them in whatever filtered list is behind this drawer. The tag covers both, so the
      // parent no longer needs a refresh counter threaded down here.
      revalidateTag(TAG.adminUsers);
    } catch (e) {
      setActionError((e as Error).message);
    } finally {
      setBusy(null);
    }
  };

  const status = profile?.status ?? summary.status;
  const role = profile?.role ?? summary.role;

  return (
    // Fixed width, NOT `w-full`, and this is the whole difference between the
    // drawer being revealed and being squeezed into place. `w-full` made the card
    // follow the wrapper's animating width, so its content relaid out on every
    // frame of the open: the address wrapped, the buttons stacked, and while the
    // panel was narrow the card stood 538px tall against the table's 220 —
    // dragging the whole row's height up and back down again over the animation.
    // Pinned to the panel's open width the layout is computed once and the panel
    // simply clips it, so the entrance is a wipe and nothing reflows.
    <Card className="w-85">
      <CardContent className="space-y-5 py-5">
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0">
            <p className="truncate font-semibold">{summary.email || summary.user_id.slice(0, 12)}</p>
            <p className="truncate text-xs text-muted-foreground">{summary.user_id}</p>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="rounded-md text-muted-foreground outline-none transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
          >
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
          <Row label="Balance" value={balance ? `${formatUsd(balance.amount)} USDT` : "—"} />
        </Section>

        <Section title="Access & security">
          <div className="flex items-center justify-between gap-2 py-1 text-sm">
            <span className="flex items-center gap-1.5 text-muted-foreground">
              Role
              <TipAnchor anchor="admin.users.access.role" />
            </span>
            <Select value={role} onValueChange={(next) => run("role", () => setUserRole(summary.user_id, next))}>
              <SelectTrigger size="sm" className="border-border bg-main-surface capitalize" disabled={busy === "role"}>
                <span className="capitalize">{role}</span>
              </SelectTrigger>
              <SelectContent>
                {ROLES.map((r) => (
                  <SelectItem key={r} value={r} className="capitalize">
                    {r}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
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
          <p className="flex items-center gap-1.5 pt-1 text-xs text-muted-foreground">
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
      <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">{title}</p>
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
  return <span className="rounded-full bg-foreground/5 px-2 py-0.5 font-medium capitalize text-foreground">{children}</span>;
}
