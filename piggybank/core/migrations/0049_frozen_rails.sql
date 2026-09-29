-- 0049: an operator can freeze a configured rail at runtime — no new deposit addresses, no
-- new withdrawals, queued ones held, and no gas alert for a treasury nobody means to fund.
-- A row is a freeze; unfreezing deletes it.
CREATE TABLE frozen_rails (
	network TEXT PRIMARY KEY CHECK (network IN ('bep20', 'trc20', 'ton', 'polygon')),
	frozen_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
