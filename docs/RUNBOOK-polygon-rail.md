# Runbook — bringing up the Polygon (PoS) USDT rail

Polygon is the money plane's **second EVM rail**. It shares the entire EVM code path with
BEP20 (BSC) — `evm_rpc` + one `ChainCustody` / `DepositWatcher` / `WithdrawalWatcher` / `Sweep`
instance per rail, keyed by `EvmConfig::network` — so operationally it behaves exactly like the
BSC rail, with two differences that matter:

- **USDT is 6-decimal on Polygon** (BEP20 is 18-decimal). The custody edge scales on-chain
  amounts to the canonical 18-dp ledger unit via `Usdt::from_onchain`; you never see this, but
  it is why the sweep's `POLYGON_SWEEP_MIN_USDT` default is `1_000_000` (1 USDT @ 6-dp), not `1e18`.
- **Gas is paid in POL** (formerly MATIC), not BNB. Both are 18-dp native coins, so the wei math
  is identical; only what you fund the wallets with changes.

The rail is **off until `POLYGON_RPC_URL` is set** — an unconfigured rail runs no watcher, mints
no deposit address, and serves nothing on the wallet surface (same no-op-when-unconfigured stance
as every other rail).

## Chain facts

| Fact | Mainnet | Amoy testnet |
| --- | --- | --- |
| Chain id (`POLYGON_CHAIN_ID`) | `137` (default) | `80002` |
| USDT contract (`POLYGON_USDT_CONTRACT`) | `0xc2132D05D31c914a87C6611C10748AEb04B58e8F` (default) | a self-deployed / faucet test ERC-20 |
| USDT decimals | 6 (fixed in `domain`) | 6 |
| Native gas coin | POL | POL (from an Amoy faucet) |
| Confirmations (`POLYGON_CONFIRMATIONS`) | `128` (default) | `128` (lower for a faster test loop if you like) |

Address form is the standard EVM `0x…` (a user's Polygon deposit address is a *different*
key/address from their BEP20 one — the signer scopes keys per `(user, network)`).

## Environment variables

Required to turn the rail on:

- `POLYGON_RPC_URL` — an `eth_getLogs`-capable JSON-RPC endpoint. Free public nodes gate/throttle
  `eth_getLogs`; use a keyed provider (Alchemy/QuickNode/Infura) for anything past a demo.
  `POLYGON_LOGS_RPC_URL` optionally splits the deposit scan onto a second endpoint.

Sensible defaults (override only to deviate): `POLYGON_USDT_CONTRACT`, `POLYGON_CHAIN_ID` (137),
`POLYGON_CONFIRMATIONS` (128), `POLYGON_POLL_SECS` (6), `POLYGON_MAX_BLOCK_RANGE` (500),
`POLYGON_GAS_LIMIT` (100_000), `POLYGON_DEPOSIT_START_BLOCK` (unset ⇒ watch from head).

Sweep (opt-in, moves user funds on-chain — leave OFF until funded): `POLYGON_SWEEP_ENABLED`
(falls back to the global `SWEEP_ENABLED`), `POLYGON_SWEEP_MIN_USDT` (1_000_000 = 1 USDT @ 6-dp),
`POLYGON_SWEEP_GAS_DROP_MULTIPLE`, `POLYGON_SWEEP_MIN_GAS_DROP_WEI`, `POLYGON_SWEEP_TOPUP_GRACE_SECS`,
`POLYGON_SWEEP_POLL_SECS`.

## Sequenced bring-up (testnet first)

1. **Amoy testnet.** Set `POLYGON_RPC_URL=<amoy endpoint>`, `POLYGON_CHAIN_ID=80002`,
   `POLYGON_USDT_CONTRACT=<test ERC-20>`. Boot the hub; the log prints the resolved **Polygon
   treasury** and **gas-station** addresses (from the signer). Deposits + withdrawals are live;
   the sweep is still off.
2. **Fund the treasury.** Send test USDT (liquidity to pay withdrawals) and a little POL (gas to
   sign them) to the treasury address. Withdrawals only dispatch once the on-chain treasury covers
   the net + gas — an underfunded rail *queues* (accept-and-queue), never fails.
3. **Prove the loop.** Deposit test USDT to a user's Polygon deposit address → it credits after
   `POLYGON_CONFIRMATIONS`. Request a withdrawal → it broadcasts, and the confirmation watcher
   auto-settles it after N confs. Check `GetTreasury` (admin) shows the Polygon rail's liquidity.
4. **Arm the sweep (optional).** Fund the **gas station** with POL, then set
   `POLYGON_SWEEP_ENABLED=true`. It consolidates user deposit balances into the treasury (topping
   up each address's gas from the station first). Only turn this on *after* the gas station holds
   POL, or every sweep cycle logs `SENDER OUT OF FUNDS`.
5. **Mainnet.** Repeat 1–4 with the mainnet endpoint (drop the chain-id/contract overrides — the
   defaults are mainnet). Fund with real USDT + POL. Keep the sweep off until the mainnet gas
   station is funded.

## Safety notes

- **Nonce isolation.** Polygon's account-nonce sequence lives in `withdrawal_broadcasts` scoped
  `WHERE network = 'polygon'`, disjoint from BEP20's `'bep20'` scope. The two EVM rails can never
  read or advance each other's nonces.
- **Stuck / parked withdrawals** on Polygon recover exactly like BSC — see
  [`RUNBOOK-withdrawals.md`](./RUNBOOK-withdrawals.md). The cardinal rule holds: never void a
  withdrawal once its broadcast may have reached the chain.
- **Confirmations.** 128 is the conservative exchange standard for Polygon (its probabilistic
  finality has historically reorged deeper than BSC). Lower it only with a clear reason; post-Rio
  finality is faster but 128 stays safe.
