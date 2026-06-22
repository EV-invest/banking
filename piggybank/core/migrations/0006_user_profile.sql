-- Editable user-profile fields on the `users` control plane.
--
-- All nullable TEXT (NULL = unset). These are identity metadata only — money stays
-- authoritative in TigerBeetle and is never re-bookkept here. email and status stay
-- read-only at the service boundary (email is the IdP's, status is admin-managed);
-- these 10 are the caller's own editable set, full-replaced by UpdateProfile.
ALTER TABLE users
    ADD COLUMN legal_name          TEXT,
    ADD COLUMN preferred_name      TEXT,
    ADD COLUMN phone               TEXT,
    ADD COLUMN date_of_birth       TEXT,
    ADD COLUMN nationality         TEXT,
    ADD COLUMN tax_residence       TEXT,
    ADD COLUMN residential_address TEXT,
    ADD COLUMN language            TEXT,
    ADD COLUMN base_currency       TEXT,
    ADD COLUMN timezone            TEXT;
