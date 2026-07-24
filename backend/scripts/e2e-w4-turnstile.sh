#!/usr/bin/env bash
# W4 — cross-activation turnstile e2e on a FRESH isolated rail (NU6.3 @ 500).
#
# RUN AND VERIFIED 2026-07-24 on rail B′ (see below). Each turnstile tx takes
# ~30s of real proving; the executor computes the ZIP-317 double-bundle fee
# itself (turnstile_fee_zatoshis), no fee override needed.
#
# The rail MUST be fresh (below the NU6.3 activation height): the legacy pool
# can only be funded pre-activation. A drained/crossed rail cannot be reused —
# stand up a new one. Verified rail recipe (zebrad v6.2.x debug build):
#   [network]  network = "Regtest"        # REQUIRED: only Regtest auto-commits
#                                         # genesis (start.rs); a custom Testnet
#                                         # deadlocks on "state is empty".
#                                         # Regtest is a special Testnet: chain
#                                         # reports "test", addrs tm/utest1.
#   [network.testnet_parameters.activation_heights]
#   NU5 = 1 ... "NU6.2" = 1
#   "NU6.3" = 500
#   [rpc]      listen_addr = "127.0.0.1:28234"; enable_cookie_auth = false
#   plus a dedicated backend (own port + own DB) pointed at the rail RPC.
#
# Covers the one path rail A cannot (PRD §8): legacy-pool spend →
# turnstile crossing into Ironwood, across the NU6.3 activation boundary.
#
# Flow:
#   pre-activation  (height < 500): fund transparent → shield into the
#                   LEGACY Orchard pool → scan, confirm notes
#   cross           mine past 500 (activation)
#   post-activation create private migration run → executor builds
#                   turnstile-crossing txs (V2 spend → V3/Ironwood output,
#                   tx v6) → all batches land
#   asserts         legacy pool balance ≈ 0 (fees only), Ironwood pool
#                   holds the funds, in-legacy-pool transfer is REFUSED
#                   with a clear error, F4.2 batch transfer works in the
#                   new pool, audit export labels pools correctly
set -euo pipefail

API=${API:-http://127.0.0.1:8080/api/v1}
RAIL_B_RPC=${RAIL_B_RPC:?set RAIL_B_RPC (rail B node, NU6.3@500)}
ACTIVATION_HEIGHT=${ACTIVATION_HEIGHT:-500}
ADMIN_USER=${ADMIN_USER:-admin}
ADMIN_PASS=${ADMIN_PASS:?set ADMIN_PASS}

say() { printf '\n=== %s ===\n' "$*"; }
rpc() { curl -sf "$RAIL_B_RPC" -H 'content-type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":${2:-[]}}"; }
api() { local m=$1 p=$2; shift 2; curl -sf -X "$m" "$API$p" -H "Authorization: Bearer $TOKEN" \
        -H 'content-type: application/json' "$@"; }
height() { rpc getblockchaininfo | python3 -c 'import sys,json;print(json.load(sys.stdin)["result"]["blocks"])'; }

say "0. rail B alive, pre-activation?"
rpc getblockchaininfo | python3 -c '
import sys,json
r=json.load(sys.stdin)["result"]
print("chain=",r["chain"],"height=",r["blocks"])
nu63=[u for u in r.get("upgrades",{}).values() if "6.3" in u.get("name","") or "Ironwood" in u.get("name","")]
print("nu6.3 upgrade entry:", nu63 or "check upgrades map manually")'
H=$(height)
[ "$H" -lt "$ACTIVATION_HEIGHT" ] || { echo "FATAL: already past activation ($H >= $ACTIVATION_HEIGHT); need a fresh rail B"; exit 1; }

say "1. login + wallet (backend must point at rail B: update RPC in Settings first)"
TOKEN=$(curl -sf -X POST "$API/auth/login" -H 'content-type: application/json' \
  -d "{\"username\":\"$ADMIN_USER\",\"password\":\"$ADMIN_PASS\"}" | python3 -c 'import sys,json;print(json.load(sys.stdin)["token"])')
W=$(api POST /wallets -d '{"name":"w4-turnstile","chain":"zcash"}')
WID=$(echo "$W" | python3 -c 'import sys,json;print(json.load(sys.stdin)["id"])')
TADDR=$(echo "$W" | python3 -c 'import sys,json;print(json.load(sys.stdin)["address"])')
UA=$(api GET "/wallets/$WID/orchard/addresses" | python3 -c 'import sys,json;d=json.load(sys.stdin);print((d if isinstance(d,list) else d.get("addresses",[]))[0]["address"])')
echo "wallet=$WID taddr=$TADDR"

say "2. PRE-ACTIVATION: fund + shield into LEGACY Orchard pool"
rpc generatetoaddress "[101,\"$TADDR\"]" >/dev/null
PROP=$(api POST /transfers/orchard -d "{\"wallet_id\":$WID,\"to_address\":\"$UA\",\"amount\":\"3.0\",\"fund_source\":\"transparent\"}")
PID=$(echo "$PROP" | python3 -c 'import sys,json;print(json.load(sys.stdin)["proposal_id"])')
EXP=$(echo "$PROP" | python3 -c 'import sys,json;print(json.load(sys.stdin)["expiry_height"])')
api POST "/transfers/orchard/$PID/execute" -d "{\"wallet_id\":$WID,\"proposal_id\":\"$PID\",\"amount_zatoshis\":300000000,\"fee_zatoshis\":15000,\"to_address\":\"$UA\",\"fund_source\":\"transparent\",\"is_shielding\":true,\"expiry_height\":$EXP}" >/dev/null
rpc generatetoaddress "[15,\"$TADDR\"]" >/dev/null
api POST /zcash/scan/sync -d '{}' >/dev/null; sleep 5
api GET "/wallets/$WID/migration-status"; echo   # expect legacy spendable > 0

say "3. CROSS ACTIVATION: mine past height $ACTIVATION_HEIGHT"
H=$(height)
NEED=$(( ACTIVATION_HEIGHT + 10 - H ))
[ "$NEED" -gt 0 ] && rpc generatetoaddress "[$NEED,\"$TADDR\"]" >/dev/null
echo "height now: $(height) (activation $ACTIVATION_HEIGHT crossed)"
api POST /zcash/scan/sync -d '{}' >/dev/null; sleep 5

say "4. POST-ACTIVATION: a shielded transfer now IS the turnstile (P1.2c)"
# There is no in-pool refusal any more: post-activation the PostNu63 gate
# routes every shielded spend through the turnstile builder (old-pool spend →
# Ironwood outputs, v6). Deshielding (Z→T) and shielding (T→Z) still fail
# closed with a clear message until their transparent-side construction lands.

say "5. TURNSTILE: immediate migration run via the production executor"
# NOTE on private multi-batch mode: until the middle-batch change-to-old-pool
# branch lands (add_orchard_change_output keeps change in the legacy pool for
# non-final batches), a private run's first batch would carry ALL change into
# Ironwood and starve the remaining batches. Re-run this section with
# mode=private on a fresh rail once that branch is merged.
RUN=$(api POST /migrations -d "{\"source_wallet_id\":$WID,\"mode\":\"immediate\"}")
RID=$(echo "$RUN" | python3 -c 'import sys,json;print(json.load(sys.stdin)["run"]["id"])')
api POST "/migrations/$RID/execute"; echo
echo "watch: batches should be v6 txs, V2 legacy spend -> V3 Ironwood output"
for i in $(seq 1 40); do
  api GET "/migrations/$RID" | python3 -c 'import sys,json;s=json.load(sys.stdin);print(s["run"]["status"],[(i["seq"],i["status"],(i["tx_hash"] or "-")[:16]) for i in s["items"]])'
  ST=$(api GET "/migrations/$RID" | python3 -c 'import sys,json;print(json.load(sys.stdin)["run"]["status"])')
  [ "$ST" = "completed" ] || [ "$ST" = "partial" ] || [ "$ST" = "failed" ] && break
  rpc generatetoaddress "[1,\"$TADDR\"]" >/dev/null 2>&1 || true
  sleep 20
done

say "6. ASSERTS"
api POST /zcash/scan/sync -d '{}' >/dev/null; sleep 5
api GET "/wallets/$WID/migration-status"; echo
cat <<'EOF'
manual checks (✓ = verified on rail B′ 2026-07-24):
  [✓] migration run completed, each tx version 6 with orchard(2)+ironwood(2)
      actions (rpc getrawtransaction <tx> 1)
  [✓] legacy pool spendable == 0 after scan; spent notes carry spent_in_tx of
      the turnstile txs. A zero-value legacy note may remain: the old pool's
      same-address padding output decrypts to us (harmless; filtered in spend
      selection).
  [✓] on-chain reconciliation: initial − fees == total Ironwood outputs
      (wallet-visible balance needs the dual-pool scanner — until it lands the
      Ironwood side is on-chain-provable but blind in migration-status)
  [ ] Ironwood-pool note re-spend (F4.2 in new pool) — needs dual-pool scanner
      + Ironwood spend path
  [ ] audit/disclosure export labels pool correctly — needs pool column
  [ ] private multi-batch run with middle-batch change kept in the legacy pool
      — needs the change-to-old-pool branch; re-run on a fresh rail
EOF
