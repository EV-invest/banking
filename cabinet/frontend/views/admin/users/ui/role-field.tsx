"use client";

// The role control, and the two options that are missing from it for two different reasons.
//
// `owner` is refused in BOTH directions: a seat is a persisted column whose only routes in
// are the consilium's admission and the genesis seed, and whose only routes out are a
// removal or a resignation. So an owner's row disables the control outright — three choices
// that would all be refused is the `FAILED_PRECONDITION` complaint this file was already
// once fixed for, where the console offered a change and the plane said no.
//
// `admin` is refused only in the GRANTING direction, and that asymmetry is load-bearing.
// An operator who can appoint operators can appoint accomplices, so admission is a proposal
// the owners vote through. Taking the role away is deliberately still ONE act, because
// containing a rogue operator must never be the slower path — which is why an admin's row
// keeps a live, enabled control here rather than being greyed out the way an owner's is.
// If you are tempted to make these two cases symmetrical: that is the bug, not the tidy-up.

import { Loader2, ShieldPlus } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Select, SelectContent, SelectItem, SelectTrigger } from "@evinvest/uikit";

import { setUserRole } from "@/entities/admin/api/admin-client";
import { openAdminAdmission } from "@/entities/governance/api/governance-client";
import { Link } from "@/shared/ui/cabinet-link";
import { TipAnchor } from "@/shared/tips";
import { ASSIGNABLE_ROLES, roleLabel } from "@/views/admin/lib/format";
import { ReasonAction } from "@/views/admin/users/ui/reason-action";

export interface RoleFieldProps {
  userId: string;
  role: string;
  busy: string | null;
  run: (key: string, fn: () => Promise<unknown>) => Promise<void>;
}

export function RoleField({ userId, role, busy, run }: RoleFieldProps) {
  const t = useT();
  const [proposing, setProposing] = useState(false);
  const [reason, setReason] = useState("");

  const seated = role === "owner";
  const working = busy === "role";

  return (
    <div className="flex flex-col gap-1.5 py-1">
      <div className="flex items-center justify-between gap-2 text-sm">
        <span className="flex items-center gap-1.5 text-muted-foreground">
          {t("admin.users.role")}
          <TipAnchor anchor="admin.users.access.role" />
        </span>
        {/* Disabled on the trigger, which is the button: the uikit's `Select` root takes no
            `disabled` of its own, and a trigger that cannot be pressed is the only door in.
            Disabled ONLY for an owner — an admin's trigger stays live so the demotion below
            it is one press away. The current role renders on the trigger even when it is not
            among the items, the same way `KycField` shows a tier it cannot re-send. */}
        <Select value={role} onValueChange={(next) => void run("role", () => setUserRole(userId, next))}>
          <SelectTrigger size="sm" className="border-border bg-main-surface" disabled={working || seated}>
            <span className="flex items-center gap-1.5">
              {working && <Loader2 className="size-3 animate-spin" />}
              {roleLabel(role, t)}
            </span>
          </SelectTrigger>
          <SelectContent>
            {ASSIGNABLE_ROLES.map((r) => (
              <SelectItem key={r} value={r}>
                {roleLabel(r, t)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <p className="text-xs leading-relaxed text-muted-foreground">
        {seated ? t("admin.users.ownerSeatHeld") : t("admin.users.adminViaProposal")}{" "}
        {/* The destination is the link text, so the sentence stops outside it — a trailing
            full stop inside the anchor would be underlined and clickable. */}
        <Link href="/consilium" className="rounded-xs underline underline-offset-2 outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
          {t("nav.consilium")}
        </Link>
        .
      </p>

      {/* Offered only where it could carry: an owner already outranks admin, and an account
          that is already admin has nothing to be admitted to. */}
      {!seated && role !== "admin" && (
        <ReasonAction
          open={proposing}
          onOpen={() => setProposing(true)}
          onCancel={() => setProposing(false)}
          label={t("admin.users.proposeAdmin")}
          hint={t("admin.users.proposeAdminHint")}
          icon={<ShieldPlus className="size-3.5" />}
          busy={working}
          reason={reason}
          setReason={setReason}
          inputId="admin-admission-reason"
          onSubmit={() =>
            void run("role", async () => {
              await openAdminAdmission(userId, reason.trim());
              setProposing(false);
              setReason("");
            })
          }
        />
      )}
    </div>
  );
}
