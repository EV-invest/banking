#!/usr/bin/env bash
# FB-24 / CROSS-5: guard the cross-repo concierge pin.
#
# Banking compiles the identity wire contract from the `evconcierge_contracts` git dep
# (Cargo.toml) and re-aliases the cabinet's identity TS from concierge's proto. This
# asserts that pin is trustworthy:
#   1. the pinned rev is an ANCESTOR of concierge `origin/main` (not an orphaned /
#      force-pushed SHA — the supply-chain fragility the audit flagged), and
#   2. the proto BYTES at the pin match what's on origin/main today (no silent drift
#      between "the proto banking compiles against" and "the proto concierge ships").
#
# Run in CI (and locally before bumping the pin). Needs network access to the concierge
# remote; exits non-zero on any violation so a bad pin can't merge.
set -euo pipefail

repo="$(git rev-parse --show-toplevel)"
remote="https://github.com/EV-invest/concierge.git"

pin="$(grep -oE 'evconcierge_contracts = \{ git = "[^"]+", (rev|tag) = "[^"]+"' "$repo/Cargo.toml" | grep -oE '(rev|tag) = "[^"]+"' | sed -E 's/.*"([^"]+)"/\1/')"
if [ -z "$pin" ]; then
	echo "::error::could not read the evconcierge_contracts pin from Cargo.toml" >&2
	exit 1
fi
echo "pinned evconcierge_contracts -> $pin"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
git -C "$work" init -q
git -C "$work" remote add origin "$remote"
git -C "$work" fetch -q --depth=200 origin main
# Resolve the pin (rev or annotated tag) to a concrete commit, fetching the tag if needed.
git -C "$work" fetch -q --depth=200 origin "$pin" 2>/dev/null || true
git -C "$work" fetch -q --tags --depth=1 origin 2>/dev/null || true
pin_commit="$(git -C "$work" rev-parse -q --verify "${pin}^{commit}" 2>/dev/null || git -C "$work" rev-parse -q --verify "$pin" 2>/dev/null || echo "")"
if [ -z "$pin_commit" ]; then
	echo "::error::pinned rev/tag '$pin' is not reachable from the concierge remote (orphaned or unpushed)" >&2
	exit 1
fi

if ! git -C "$work" merge-base --is-ancestor "$pin_commit" origin/main; then
	echo "::error::pinned rev $pin_commit is NOT an ancestor of concierge origin/main" >&2
	exit 1
fi
echo "ok: pin is an ancestor of origin/main"

# The proto bytes banking depends on must match origin/main's. The identity surface the
# cabinet straddles lives in concierge/v1/{directory,auth}.proto.
for p in concierge/v1/directory.proto concierge/v1/auth.proto; do
	pinned="$(git -C "$work" show "$pin_commit:contracts/proto/$p")"
	head="$(git -C "$work" show "origin/main:contracts/proto/$p")"
	if [ "$pinned" != "$head" ]; then
		echo "::error::contracts/proto/$p differs between the pin and origin/main — bump the pin deliberately" >&2
		exit 1
	fi
done
echo "ok: vendored identity proto bytes match origin/main"
