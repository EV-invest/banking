-- 0050: the concrete `bank:*` permissions concierge resolves for a user's seat, mirrored
-- from PERMISSIONS_CHANGED. NULL: concierge has not said yet. Nothing reads it until the
-- authorization gate moves from the role to these.
--
-- `bridge_deferred_event` carries it like every other field `bridge::apply` reads, or a
-- parked PERMISSIONS_CHANGED would replay as "holds nothing".
--
-- REVERSIBILITY: ALTER TABLE users DROP COLUMN permissions;
--                ALTER TABLE bridge_deferred_event DROP COLUMN permissions;
SET LOCAL lock_timeout = '3s';

ALTER TABLE users ADD COLUMN permissions TEXT[];
ALTER TABLE bridge_deferred_event ADD COLUMN permissions TEXT[] NOT NULL DEFAULT '{}';
