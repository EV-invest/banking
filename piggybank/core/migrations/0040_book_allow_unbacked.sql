-- 0040: the operator's acknowledgement that a book trades units the fund holds no cash for.
--
-- `allocations.backing` (0039) says whether the fund's claim holds the cash behind a
-- product's units. On an `in_kind` product a redemption is refused and the book is the
-- holders' only exit — which makes the book the place a buyer pays cash for a claim on an
-- asset held in kind, one they cannot redeem. Opening such a book is a decision an operator
-- must make knowingly, and the terminal must be able to tell buyers what they are buying.
-- This flag is that acknowledgement: the hub refuses to open the book on an `in_kind`
-- product without it (`SetBookPolicy`), refuses every order on an `in_kind` product's
-- open book without it (`PlaceOrder` — the backing can flip AFTER the book opened, on the
-- first in-kind mint, and a book that kept trading would sell unbacked units nobody
-- acknowledged), and the terminal shows a notice when it is set. Harmless on a `cash`
-- product: it acknowledges in advance.
--
-- Backfill BY DATA: a book that is open today on a product whose units are `in_kind` has
-- been trading unbacked units all along, with the operator's knowledge — in production
-- that is `service_arb`. Leaving it `false` would close that book by deployment, with no
-- operator having decided so; the flag is set for it here so the deploy changes nothing
-- the holders can see. A closed book, or an open one on a `cash` product, stays `false`:
-- the acknowledgement is given when the book opens on unbacked units, not presumed.
--
-- Backward compatible in both directions of a rolling deploy: a constant `DEFAULT` on
-- Postgres ≥ 11 adds the column without rewriting the table (a handful of rows), a pod
-- that predates the column never selects it, and its UPSERT names only the columns it
-- knows — `ON CONFLICT DO UPDATE SET` leaves this one where it stands, so a policy edit
-- from an old pod mid-rollout cannot un-acknowledge a book. The one window is a policy
-- row CREATED by an old pod during the rollout, which lands on `false`; the new pod then
-- refuses that product's orders with the reason spelled out and the operator sets the
-- flag from the console. Short, visible and recoverable. No `lock_timeout`, like every
-- neighbouring migration: the table is tiny and the DDL is instant.
--
-- Migrations run at hub boot. Reversible: `ALTER TABLE book_policies DROP COLUMN
-- allow_unbacked_trading` loses nothing a pre-0040 binary can read.
ALTER TABLE book_policies
    ADD COLUMN allow_unbacked_trading BOOLEAN NOT NULL DEFAULT false;

COMMENT ON COLUMN book_policies.allow_unbacked_trading IS
    'Operator acknowledged that the book trades units the fund holds no cash for (allocations.backing = in_kind). Required to open, and to take an order on, an in_kind product''s book; the terminal shows buyers a notice.';

UPDATE book_policies bp
   SET allow_unbacked_trading = true
  FROM allocations a
 WHERE a.service = bp.service
   AND bp.book_open
   AND a.backing = 'in_kind';
