-- The concierge user id of each bridge-mirrored user. Identity is OWNED by the concierge
-- plane; this is a foreign reference (NOT a local key) the money plane stores so it can
-- resolve a local row from the handle the cabinet BFF carries — the concierge `sub` — when
-- minting a money-plane token via AuthService.IssueUserToken (the concierge→banking seam).
--
-- Set by the one-way bridge `CREATED` consumer (see infrastructure/bridge.rs). UNIQUE so one
-- local row maps to one concierge user; NULL for any row provisioned before this column.
ALTER TABLE users ADD COLUMN concierge_user_id UUID UNIQUE;
