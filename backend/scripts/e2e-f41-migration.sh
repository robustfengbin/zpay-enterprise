#!/usr/bin/env bash
# F4.1 migration engine e2e against regtest rail A (NU5@1, old pool live).
# Draft — RAIL_* variables to be filled from luxun's persisted regtest config.
# Flow: login → wallet+orchard → fund (mine→shield) → private migration run
#       → approve → watch batches → kill/restart resume → final fold.
set -euo pipefail

API=${API:-http://127.0.0.1:8080/api/v1}
RAIL_RPC=${RAIL_RPC:-http://127.0.0.1:28232}   # rail A RPC (luxun)
ADMIN_USER=${ADMIN_USER:-admin}
ADMIN_PASS=${ADMIN_PASS:?set ADMIN_PASS}       # from backend/.env, not committed
CHECKER_USER=${CHECKER_USER:-}                 # second admin for maker≠checker leg (optional)
CHECKER_PASS=${CHECKER_PASS:-}

say() { printf '\n=== %s ===\n' "$*"; }
rpc() { curl -sf "$RAIL_RPC" -H 'content-type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":${2:-[]}}"; }
api() { local m=$1 p=$2; shift 2; curl -sf -X "$m" "$API$p" -H "Authorization: Bearer $TOKEN" \
        -H 'content-type: application/json' "$@"; }

say "0. rail A alive?"
rpc getblockchaininfo | head -c 300; echo

say "1. login"
TOKEN=$(curl -sf -X POST "$API/auth/login" -H 'content-type: application/json' \
  -d "{\"username\":\"$ADMIN_USER\",\"password\":\"$ADMIN_PASS\"}" | python3 -c 'import sys,json;print(json.load(sys.stdin)["token"])')

say "2. zcash wallet + orchard"
WALLET=$(api POST /wallets -d '{"name":"e2e-f41","chain":"zcash"}')
WID=$(echo "$WALLET" | python3 -c 'import sys,json;print(json.load(sys.stdin)["id"])')
api POST "/wallets/$WID/orchard/enable" -d '{}' >/dev/null
UA=$(api GET "/wallets/$WID/orchard/addresses" | python3 -c 'import sys,json;print(json.load(sys.stdin)[0]["address"])')
echo "wallet=$WID ua=$UA"

say "3. fund: mine to wallet taddr (rail A generatetoaddress), mature, shield t->z"
# Requires F4.0-b network-awareness: TADDR must come back regtest-encoded (tm...).
# Verified on rail A 07-24: generatetoaddress exists and validates the network.
TADDR=$(echo "$WALLET" | python3 -c 'import sys,json;print(json.load(sys.stdin)["address"])')
case "$TADDR" in
  tm*) ;;
  *) echo "FATAL: wallet taddr '$TADDR' is not regtest-encoded — F4.0-b fix not in effect"; exit 1;;
esac
rpc generatetoaddress "[101,\"$TADDR\"]" >/dev/null   # 1 spendable coinbase + 100 maturity
rpc getblockchaininfo | python3 -c 'import sys,json;print("tip:",json.load(sys.stdin)["result"]["blocks"])'

say "3b. shield: transparent -> own UA (backend real shielding path)"
PROP=$(api POST /transfers/orchard -d "{\"wallet_id\":$WID,\"to_address\":\"$UA\",\"amount\":\"3.0\",\"fund_source\":\"transparent\"}")
echo "$PROP" | head -c 300; echo
PID=$(echo "$PROP" | python3 -c 'import sys,json;print(json.load(sys.stdin)["proposal_id"])')
api POST "/transfers/orchard/$PID/execute" -d "$(echo "$PROP" | python3 -c '
import sys,json
p=json.load(sys.stdin)
print(json.dumps({"wallet_id":'$WID',"proposal_id":p["proposal_id"],"amount_zatoshis":p["amount_zatoshis"],
  "fee_zatoshis":p["fee_zatoshis"],"to_address":p["to_address"],"fund_source":"transparent",
  "is_shielding":True,"expiry_height":p["expiry_height"]}))')"
rpc generate '[15]' >/dev/null    # bury past MIN_CONFIRMATIONS=10
api POST /zcash/scan/sync >/dev/null || true; sleep 5
api GET "/wallets/$WID/migration-status"; echo   # expect spendable > 0, notes >= 1

say "4. migration run: private, 3 batches / 1h window"
RUN=$(api POST /migrations -d "{\"source_wallet_id\":$WID,\"mode\":\"private\",\"batch_count\":3,\"window_hours\":1}")
RID=$(echo "$RUN" | python3 -c 'import sys,json;print(json.load(sys.stdin)["run"]["id"])')
echo "run=$RID"
api POST "/migrations/$RID/execute"      # -> executing (or awaiting_approval if policy set)

say "5. (optional) approval leg — requires CHECKER_USER and a matching policy"
# create policy below threshold, re-execute, login as checker, approve:
# api POST "/migrations/$RID/approve"  (as checker; maker-approve must 403)

say "6. watch batches (executor ticks 30s; batch1 immediate)"
for i in $(seq 1 30); do
  api GET "/migrations/$RID" | python3 -c 'import sys,json;s=json.load(sys.stdin);print(s["run"]["status"], [(i["seq"],i["status"],i["tx_hash"] or "-") for i in s["items"]])'
  sleep 20
  rpc generate '[1]' >/dev/null 2>&1 || true   # keep chain moving for confirmations
done

say "7. resume test: kill backend mid-window, restart, verify remaining batches fire"
echo "manual: pm2 restart (delete+start if env changed) between batch 2 and 3; re-run step 6 watch"

say "8. asserts"
# - run folds to completed; every item submitted with tx_hash
# - old-pool balance ~0 (minus fees); notes re-scanned as spent
# - cancel path: second run + cancel → pending items canceled, no further broadcasts
