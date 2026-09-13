-- 0030: products are closed by default — an allocation now carries WHO may see it and
-- who may put money in, on an axis of its own beside the lifecycle.
--
-- Until now `open` meant "listed to everyone and taking money from anyone". That
-- conflated two decisions an operator makes separately: whether the product deals at
-- all, and whom it deals with. A product being prepared for a handful of invited
-- investors had no honest state — `draft` hid it from the invitees too, `open` showed
-- it to the world. This migration gives the second decision its own column and its own
-- per-investor exception list.
--
-- `access` is the product's DEFAULT level — `hidden` | `view` | `invest` — and
-- `allocation_access_grants` raises individual investors above it. The level an
-- investor effectively holds is the higher of the two (the domain's
-- `AllocationAccess::effective`); the catalog shows an `open` product to a caller at
-- `view` or above, and `Subscribe` admits a caller at `invest` only. Redemptions are
-- never gated on access — a locked product must not trap the units already in it.
--
-- A CHECK-constrained TEXT rather than an ENUM, like every vocabulary column here
-- (`allocations.state` in 0021, `allocations.icon` in 0027): widening the set stays an
-- ordinary migration. The list is `domain::allocations::AllocationAccess`, held to this
-- one by `domain_access_levels_match_the_wire_contract` (domain ↔ wire) and by the
-- integration test that writes every domain level into the column.
--
-- `NOT NULL DEFAULT` in one `ADD COLUMN` records the default in the catalog without a
-- rewrite (PG11+), but the CHECK is verified against existing rows under ACCESS
-- EXCLUSIVE — `allocations` holds one row per product, so that scan is microseconds. Do
-- not copy this shape onto a large table (see 0027 for the split-and-VALIDATE form).
--
-- 'view' is what a NEW registration lands on — visible, so the catalog shows what is
-- coming, but locked. It is also what a row written by the CURRENTLY DEPLOYED code takes
-- (its INSERT names its columns, so the default fills the gap), which is the right
-- answer for a product registered mid-rollout: nobody is let in by accident.
ALTER TABLE allocations
    ADD COLUMN access TEXT NOT NULL DEFAULT 'view'
        CHECK (access IN ('hidden', 'view', 'invest'));

-- NOT RETROACTIVE, deliberately. Every product that is `open` today is open to everyone
-- today, and this release must not change that under the investors already looking at
-- it: `open` rows backfill to `invest`, so the catalog and the subscribe button behave
-- exactly as before the column existed. `draft` and `closed` rows take the column default
-- (`view`) — a draft has never dealt, a closed product no longer deals, and an operator
-- re-opening either decides its access as a separate step, as for a new product.
UPDATE allocations SET access = 'invest' WHERE state = 'open';

COMMENT ON COLUMN allocations.access IS
    'Default access level — hidden | view | invest. Who sees the product and who may subscribe, before per-user grants. Orthogonal to state; never gates redemptions.';

-- One row per (product, investor) raised above the default. `level` cannot be `hidden`:
-- a grant only ever ADDS (effective = max(default, grant)), so a hidden grant would be a
-- no-op that reads as a lockout. Both user columns reference `users` — a grant names a
-- money-plane investor and a money-plane operator, resolved the way every admin RPC
-- resolves a target (concierge-first via the bridge mirror, then the banking id). No
-- `ON DELETE`: users are disabled, never deleted. No cascade from `allocations` either:
-- an allocation is never deleted, and a grant outliving its product would be a bug
-- worth refusing rather than silently sweeping.
CREATE TABLE allocation_access_grants (
    service    TEXT        NOT NULL REFERENCES allocations (service),
    user_id    UUID        NOT NULL REFERENCES users (id),
    level      TEXT        NOT NULL CHECK (level IN ('view', 'invest')),
    granted_by UUID        NOT NULL REFERENCES users (id),
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (service, user_id)
);

COMMENT ON TABLE allocation_access_grants IS
    'Per-investor exceptions to allocations.access: the investor holds max(default, level). Written by GrantAllocationAccess, removed by RevokeAllocationAccess (AllocationManage). Audit facts live in event_log under the allocation aggregate.';
COMMENT ON COLUMN allocation_access_grants.level IS
    'view | invest — never hidden; a grant only raises.';
COMMENT ON COLUMN allocation_access_grants.granted_by IS
    'The AllocationManage holder who raised this investor — the audit answer to "who let them in".';
