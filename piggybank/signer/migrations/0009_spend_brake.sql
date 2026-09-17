-- 0009: spend_brake — the operator's emergency brake on the signer, in the signer's own
-- database, read before every signature.
--
-- Every ceiling the signer enforces comes from its environment, loaded once at boot. Tightening
-- one — or stopping every payout while an incident is understood — therefore meant a redeploy,
-- and a redeploy is the slowest lever there is when the hub is the thing suspected. This table
-- is the fast lever: one row an operator UPDATEs by hand, and the next signing request already
-- sees it. It can only ever tighten. Each ceiling here is taken as `min(env, brake)` on every
-- request, so a value above the environment's leaves the environment in force, and `halted`
-- refuses every signature outright (`permission_denied`, which the hub answers by parking the
-- withdrawal until an operator unparks it — the same verdict as any other policy refusal).
--
-- Why exactly one row: the brake is a global posture, not a per-wallet setting, and a single
-- `id = 1` under a CHECK is the simplest way to make "the row" a definite thing the signer can
-- read with no WHERE clause to get wrong. The seed below is what makes it exist; a MISSING row
-- is not "released" but a refusal — the signer cannot tell a deleted row from a broken
-- database, and fails closed on both (`Internal`, which parks too). Release is
-- `UPDATE spend_brake SET halted = false`, never `DELETE`.
--
-- Why these six ceilings and no others: they are the three window-level bounds the policy
-- keeps (one per treasury payout, treasury USDT per rail per hour, native coin per wallet per
-- rail per hour). The per-signature caps — fee budget, gas top-up, treasury native — are each
-- subsumed by the native window (the window is their sum), so braking the window brakes them.
-- NULL means "the environment's ceiling applies", and `> 0` because a zero is what `halted` is
-- for (the environment refuses a zero at boot for the same reason). Units are those of the
-- matching variable: `max_transfer_usdt` and `max_treasury_usdt_per_hour` in whole USDT like
-- `SIGNER_MAX_TRANSFER_USDT` / `SIGNER_MAX_TREASURY_USDT_PER_HOUR`; the four native columns in
-- the rail's base units — wei, wei, SUN, nanoton — like `SIGNER_MAX_NATIVE_SPEND_PER_HOUR_*`,
-- NUMERIC(39,0) for the same reason `native_spend.spend` is (a `u128` does not fit a BIGINT).
--
-- No RPC reads or writes this table on purpose. The hub is the adversary in the signer's threat
-- model: it holds a valid service token and can call every signer RPC, so an RPC to release
-- the brake would hand the brake to the thing it is meant to stop. Since #173 the hub's role
-- has no CONNECT on this database at all; the operator reaches it as `postgres` over the unix
-- socket on the host (`docs/RUNBOOK-withdrawals.md`). `updated_at` is set by the trigger so an
-- operator typing under pressure does not have to remember it — it dates the posture in the
-- boot log and the per-request refusal.
--
-- Every UPDATE also leaves a row in `spend_brake_history`: who (`current_user`), from where
-- (`inet_client_addr()` — NULL is the unix socket, which is how the operator connects), and
-- the row before and after as JSON. This is detection, not protection: the table's owner and
-- a superuser can DELETE from it, and the trigger records nothing if the row itself is
-- deleted (a missing row refuses every signature, which is loud on its own). What it buys is
-- a trace of a RELEASE — the transition an attacker who reached this database would make,
-- and the one that is otherwise visible only as refusals stopping. The signer logs the same
-- transition from its side (`spend brake CHANGED`) on the first request that sees a
-- different row; the two are independent witnesses. The history write lives in the same
-- trigger function as the `updated_at` stamp so that there is exactly one thing firing on
-- UPDATE and no way to have the stamp without the trace — and because a BEFORE ROW trigger
-- runs before the CHECKs, an UPDATE the constraints reject rolls the history row back with
-- it, so the history only ever holds changes that took effect.
--
-- Irreversible? No: additive, and the previous binary neither reads nor writes these tables.
-- But the previous binary does NOT boot over a database that has this migration applied:
-- `sqlx::migrate!().run` compares `_sqlx_migrations` with the migrations compiled in and
-- refuses a version it does not know ("migration 9 was previously applied but is missing in
-- the resolved migrations"). This repo carries no down files (see 0001-0008), so rolling
-- back is: first `DELETE FROM _sqlx_migrations WHERE version = 9` as `postgres` over the unix
-- socket (the tables can stay — the old binary never reads them), THEN roll the image back.
-- Without that first step the only way out is forward.
SET lock_timeout = '3s';

CREATE TABLE spend_brake (
    id                                SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    halted                            BOOLEAN NOT NULL DEFAULT false,
    max_transfer_usdt                 BIGINT NULL CHECK (max_transfer_usdt > 0),
    max_treasury_usdt_per_hour        BIGINT NULL CHECK (max_treasury_usdt_per_hour > 0),
    max_native_spend_per_hour_bep20   NUMERIC(39, 0) NULL CHECK (max_native_spend_per_hour_bep20 > 0),
    max_native_spend_per_hour_polygon NUMERIC(39, 0) NULL CHECK (max_native_spend_per_hour_polygon > 0),
    max_native_spend_per_hour_trc20   NUMERIC(39, 0) NULL CHECK (max_native_spend_per_hour_trc20 > 0),
    max_native_spend_per_hour_ton     NUMERIC(39, 0) NULL CHECK (max_native_spend_per_hour_ton > 0),
    reason                            TEXT NULL,
    updated_at                        TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Append-only from the signer's point of view: nothing in the binary reads, updates or
-- deletes it. `id` orders the entries (`changed_at` can tie within one transaction).
CREATE TABLE spend_brake_history (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    changed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    by_role    TEXT NOT NULL,
    from_addr  INET NULL,
    old_row    JSONB NOT NULL,
    new_row    JSONB NOT NULL
);

-- `updated_at` is stamped first so the history holds the row exactly as the signer will read
-- it back, stamp included.
CREATE FUNCTION spend_brake_on_update() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    NEW.updated_at := now();
    INSERT INTO spend_brake_history (by_role, from_addr, old_row, new_row)
    VALUES (current_user, inet_client_addr(), to_jsonb(OLD), to_jsonb(NEW));
    RETURN NEW;
END;
$$;

CREATE TRIGGER spend_brake_on_update
    BEFORE UPDATE ON spend_brake
    FOR EACH ROW EXECUTE FUNCTION spend_brake_on_update();

-- The one row the signer reads. Released, with every ceiling left to the environment.
INSERT INTO spend_brake (id) VALUES (1);
