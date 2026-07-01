-- Mirror the concierge access role onto the banking user projection.
--
-- Identity (including the role) is OWNED by the concierge plane; banking only mirrors
-- the slice it needs to gate money ops. This column is maintained solely by the
-- one-way lifecycle bridge (the ROLE_CHANGED event, and the role snapshot carried on
-- every lifecycle row) — the money plane never writes it directly. It lets the
-- operator RPCs (treasury/valuation/redemptions/read-any-balance) gate on the same
-- role the identity plane granted.
ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'investor';
