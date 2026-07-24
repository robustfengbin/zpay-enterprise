#!/usr/bin/env bash
# W4 — rail B (NU6.3 @ 500) cross-activation turnstile e2e.
# Draft, prepared ahead of rail B: RAIL_B_* vars get their real values when
# luxun stands the rail up (F4.0-c dual-pool must be merged first).
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

say "4. POST-ACTIVATION: legacy in-pool transfer must be REFUSED (PRD F4.1.9)"
set +e
REFUSE=$(api POST /transfers/orchard -d "{\"wallet_id\":$WID,\"to_address\":\"$UA\",\"amount\":\"0.1\",\"fund_source\":\"shielded\"}" 2>&1)
set -e
echo "in-legacy-pool transfer response (expect clear refusal or turnstile-only routing): $REFUSE" | head -c 400; echo

say "5. TURNSTILE: private migration run 3 batches / 1h"
RUN=$(api POST /migrations -d "{\"source_wallet_id\":$WID,\"mode\":\"private\",\"batch_count\":3,\"window_hours\":1}")
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
manual checks:
  [ ] migration run completed 3/3, each tx is version 6 (rpc getrawtransaction <tx> 1 | grep version)
  [ ] legacy pool balance ~= 0 (fees only); Ironwood pool holds the funds
      (orchard_notes rows carry the right pool column once F4.0-c adds it)
  [ ] Ironwood-pool note re-spend works: small F4.2 batch transfer to another wallet completes
  [ ] audit/disclosure export labels pool correctly (F1.1)
  [ ] run the amount reconciliation: initial - fees == final Ironwood balance
EOF
