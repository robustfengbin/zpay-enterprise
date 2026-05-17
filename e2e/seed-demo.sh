#!/usr/bin/env bash
# Populate the running backend with English-named demo data for screenshot /
# marketing use. Idempotent for create-once-per-run — produces fresh IDs each
# time (the script does not delete; pair with `./e2e/smoke.sh --reset` first
# if you want a clean slate).
#
# Usage:
#   ./e2e/seed-demo.sh                       # against http://127.0.0.1:8080
#   E2E_API=https://zpaystage.fastaitop.com/api/v1 ./e2e/seed-demo.sh
#
# Requires .env.secrets (or E2E_ADMIN_PASSWORD env) to read the admin password.

set -uo pipefail

API="${E2E_API:-http://127.0.0.1:8080/api/v1}"
BACKEND_DIR="${E2E_BACKEND_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/backend}"

RED=$'\033[31m'; GREEN=$'\033[32m'; CYAN=$'\033[36m'; DIM=$'\033[2m'; RST=$'\033[0m'

log()  { printf '%s[seed]%s %s\n' "$CYAN" "$RST" "$*"; }
ok()   { printf '  %s✓%s %s\n' "$GREEN" "$RST" "$*"; }
fail() { printf '  %s✗%s %s\n  %sresponse:%s %s\n' "$RED" "$RST" "$1" "$DIM" "$RST" "$2" >&2; exit 1; }

api() {
  local method="$1" path="$2" auth="${3:-}" body="${4:-}"
  local args=( -sS -X "$method" "${API}${path}" -w '\n%{http_code}' )
  [[ -n "$auth" ]] && args+=( -H "Authorization: Bearer $auth" )
  [[ -n "$body" ]] && args+=( -H 'Content-Type: application/json' --data "$body" )
  curl "${args[@]}"
}
parse() { RESP="${1%$'\n'*}"; STATUS="${1##*$'\n'}"; }
jget()  { python3 -c "import sys,json;v=json.load(sys.stdin)
for k in '$1'.split('.'):
    v = v[int(k)] if k.isdigit() else v[k]
print(v)"; }

# -----------------------------------------------------------------------------
# Auth — read admin pw from .env.secrets unless E2E_ADMIN_PASSWORD is set
# -----------------------------------------------------------------------------
log "auth as admin"
ADMIN_PASS="${E2E_ADMIN_PASSWORD:-}"
if [[ -z "$ADMIN_PASS" ]]; then
  if [[ ! -f "$BACKEND_DIR/.env.secrets" ]]; then
    fail "missing $BACKEND_DIR/.env.secrets and E2E_ADMIN_PASSWORD env not set" ""
  fi
  ADMIN_PASS=$(grep '^WEB3_SECURITY__ADMIN_INITIAL_PASSWORD=' "$BACKEND_DIR/.env.secrets" | cut -d= -f2)
fi
[[ -z "$ADMIN_PASS" ]] && fail "admin password not found" ""

parse "$(api POST /auth/login '' "{\"username\":\"admin\",\"password\":\"$ADMIN_PASS\"}")"
[[ "$STATUS" == "200" ]] || fail "admin login" "$RESP ($STATUS)"
TOKEN=$(echo "$RESP" | jget token)
ok "logged in as admin"

# -----------------------------------------------------------------------------
# Wallets — English business-style names
# -----------------------------------------------------------------------------
log "creating wallets"
declare -A WALLET_IDS
for spec in \
  'Corporate Treasury - ZEC|zcash' \
  'Operations Wallet - ZEC|zcash' \
  'USD Treasury - USDT|ethereum' \
  'Engineering Payroll - USDT|ethereum'; do
  NAME="${spec%|*}"
  CHAIN="${spec#*|}"
  parse "$(api POST /wallets "$TOKEN" "{\"name\":\"$NAME\",\"chain\":\"$CHAIN\"}")"
  if [[ "$STATUS" =~ ^20[01]$ ]]; then
    WID=$(echo "$RESP" | jget id)
    WALLET_IDS["$NAME"]="$WID"
    ok "wallet '$NAME' id=$WID ($CHAIN)"
  else
    fail "wallet '$NAME'" "$RESP ($STATUS)"
  fi
done
# Pick the ZEC corporate treasury as the canonical demo wallet
DEMO_ZEC_WALLET="${WALLET_IDS[Corporate Treasury - ZEC]}"
DEMO_USDT_WALLET="${WALLET_IDS[Engineering Payroll - USDT]}"

# Activate one of each chain so the Transfer page has a default sender
parse "$(api PUT "/wallets/$DEMO_ZEC_WALLET/activate" "$TOKEN")"
[[ "$STATUS" == "200" ]] && ok "activated ZEC corporate treasury"
parse "$(api PUT "/wallets/$DEMO_USDT_WALLET/activate" "$TOKEN")"
[[ "$STATUS" == "200" ]] && ok "activated USDT engineering payroll"

# -----------------------------------------------------------------------------
# Approval policies — realistic enterprise thresholds
# -----------------------------------------------------------------------------
log "creating approval policies"
for spec in \
  'global|ethereum|USDT|5000.0|240' \
  'global|zcash|ZEC|50.0|360'; do
  SCOPE="${spec%%|*}"; rest="${spec#*|}"
  CHAIN="${rest%%|*}"; rest="${rest#*|}"
  TOKEN_NAME="${rest%%|*}"; rest="${rest#*|}"
  AMOUNT="${rest%%|*}"; SLA="${rest##*|}"
  parse "$(api POST /approval-policies "$TOKEN" \
    "{\"scope\":\"$SCOPE\",\"chain\":\"$CHAIN\",\"token\":\"$TOKEN_NAME\",\"amount_threshold\":\"$AMOUNT\",\"sla_minutes\":$SLA,\"required_count\":1,\"enabled\":true}")"
  if [[ "$STATUS" =~ ^20[01]$ ]]; then
    PID=$(echo "$RESP" | jget id)
    ok "policy id=$PID — $SCOPE / $CHAIN / $TOKEN_NAME / ≥$AMOUNT / ${SLA}min SLA"
  else
    fail "policy $SCOPE/$CHAIN/$TOKEN_NAME" "$RESP ($STATUS)"
  fi
done

# -----------------------------------------------------------------------------
# Employees — diverse English names + departments via tags
# -----------------------------------------------------------------------------
log "creating employees"
# Use validated zcash/ethereum addresses (real format but unowned)
# Ethereum: well-known burn-style addresses with valid checksum
# Zcash: t1 transparent addresses (we use generated demo addresses)
ETH_ADDRS=(
  "0x742d35Cc6634C0532925a3b844Bc454e4438f44e"
  "0x4a679253410272dd5232b3ff7cf5dbb88f295319"
  "0xAb5801a7D398351b8bE11C439e05C5B3259aeC9B"
  "0xBe0eB53F46cd790Cd13851d5EFf43D12404d33E8"
)
ZEC_ADDRS=(
  "t1MuYerfu1kbxuZvGgdAVn7Wisv8PLUtNFk"
  "t1cEVXapVBTVzaK5pmjTBFg1wEUhS3WKEFE"
  "t1Qsc2499S51U4Wnns92TE75Db3uY85c9ek"
)

CODE_BASE="DEMO$(date +%s)"
i=0
for spec in \
  'Alice Chen|Engineering|verified|zcash' \
  'Marcus Williams|Engineering|verified|zcash' \
  'Priya Patel|Product Design|verified|zcash' \
  'Diego Hernandez|Operations|pending|ethereum' \
  'Yui Tanaka|Engineering|verified|zcash' \
  'James O Brien|Finance|verified|ethereum'; do
  NAME="${spec%%|*}"; rest="${spec#*|}"
  DEPT="${rest%%|*}"; rest="${rest#*|}"
  KYC="${rest%%|*}"; CHAIN="${rest##*|}"
  CODE="${CODE_BASE}-$((i + 1))"
  if [[ "$CHAIN" == "ethereum" ]]; then
    ADDR="${ETH_ADDRS[$((i % ${#ETH_ADDRS[@]}))]}"
  else
    ADDR="${ZEC_ADDRS[$((i % ${#ZEC_ADDRS[@]}))]}"
  fi
  TAGS="{\"department\":\"$DEPT\",\"kyc_status\":\"$KYC\",\"preferred_token\":\"$([[ $CHAIN == ethereum ]] && echo USDT || echo ZEC)\"}"
  parse "$(api POST /payroll/employees "$TOKEN" \
    "{\"employee_code\":\"$CODE\",\"name\":\"$NAME\",\"wallet_address\":\"$ADDR\",\"chain\":\"$CHAIN\",\"tags\":$TAGS,\"active\":true}")"
  if [[ "$STATUS" =~ ^20[01]$ ]]; then
    ok "employee '$NAME' ($DEPT, $CHAIN)"
  else
    fail "employee '$NAME'" "$RESP ($STATUS)"
  fi
  i=$((i + 1))
done

# -----------------------------------------------------------------------------
# Payroll run — May 2026, realistic salary amounts.
#
# ZEC ≈ $500 USD (2026-05-17 reference) so each employee gets 8–15 ZEC =
# $4k–$7.5k monthly. Total ≈ 65 ZEC ≈ $32.5k — designed to land above the
# global ZEC ≥ 50 approval policy threshold so executing the run flips it
# to `awaiting_approval` immediately, populating the approval UI for
# marketing screenshots.
# -----------------------------------------------------------------------------
log "creating sample payroll run (May 2026, ~\$32.5k total)"
ITEMS='[
  {"employee_address":"t1MuYerfu1kbxuZvGgdAVn7Wisv8PLUtNFk","amount":"12.00","memo":"May 2026 — Alice Chen (Senior Engineer)"},
  {"employee_address":"t1cEVXapVBTVzaK5pmjTBFg1wEUhS3WKEFE","amount":"15.00","memo":"May 2026 — Marcus Williams (Staff Engineer)"},
  {"employee_address":"t1Qsc2499S51U4Wnns92TE75Db3uY85c9ek","amount":"10.00","memo":"May 2026 — Priya Patel (Senior Designer)"},
  {"employee_address":"t1MuYerfu1kbxuZvGgdAVn7Wisv8PLUtNFk","amount":"8.00","memo":"May 2026 — Yui Tanaka (Engineer)"},
  {"employee_address":"t1cEVXapVBTVzaK5pmjTBFg1wEUhS3WKEFE","amount":"10.00","memo":"May 2026 — base salary"},
  {"employee_address":"t1Qsc2499S51U4Wnns92TE75Db3uY85c9ek","amount":"10.00","memo":"May 2026 — base + Q1 performance bonus"}
]'
parse "$(api POST /payroll/runs "$TOKEN" \
  "{\"pay_period\":\"2026-05\",\"source_wallet_id\":$DEMO_ZEC_WALLET,\"notes\":\"Monthly engineering + design payroll\",\"items\":$ITEMS}")"
if [[ "$STATUS" =~ ^20[01]$ ]]; then
  RUN_ID=$(echo "$RESP" | jget run_id)
  ok "payroll run id=$RUN_ID (pay_period 2026-05, 6 items, total 65 ZEC ≈ \$32.5k)"
else
  fail "payroll run" "$RESP ($STATUS)"
fi

# Execute the run — total 65 ZEC > 50 threshold so this flips run.status
# to awaiting_approval without touching the chain (no balance check at the
# approval pivot path).
log "executing run to trigger F2.1 approval pivot"
parse "$(api POST "/payroll/runs/$RUN_ID/execute" "$TOKEN")"
if [[ "$STATUS" == "200" ]] && [[ "$(echo "$RESP" | jget result)" == "awaiting_approval" ]]; then
  POLICY_ID=$(echo "$RESP" | jget policy_id)
  ok "run $RUN_ID auto-pivoted to awaiting_approval (matched policy id=$POLICY_ID)"
else
  log "  (run did not pivot — check policy thresholds; raw response: $RESP)"
fi

# -----------------------------------------------------------------------------
# Auditor — external CPA firm for Q1 2026
# -----------------------------------------------------------------------------
log "creating demo auditor"
parse "$(api POST /auditors "$TOKEN" \
  "{\"email\":\"j.morgan@cpa-demo.com\",\"name\":\"Jennifer Morgan, CPA\",\"wallet_ids\":[$DEMO_ZEC_WALLET],\"scope_start\":\"2026-01-01T00:00:00Z\",\"scope_end\":\"2026-03-31T23:59:59Z\",\"max_count\":20}")"
if [[ "$STATUS" =~ ^20[01]$ ]]; then
  AUDITOR_ID=$(echo "$RESP" | jget auditor_id)
  TEMP_PW=$(echo "$RESP" | jget temp_password)
  ok "auditor id=$AUDITOR_ID 'Jennifer Morgan, CPA' for wallet $DEMO_ZEC_WALLET (Q1 2026 scope, 20 disclosures)"
  log "  email: j.morgan@cpa-demo.com"
  log "  temp_password: $TEMP_PW   ← paste into /auditor/login (don't screenshot in production)"
else
  fail "auditor" "$RESP ($STATUS)"
fi

# -----------------------------------------------------------------------------
printf '\n%s═══ Demo seed complete ═══%s\n' "$DIM" "$RST"
printf '  Wallets       : %d\n' ${#WALLET_IDS[@]}
printf '  Policies      : 2 (USDT ≥ 5000, ZEC ≥ 50 ≈ \$25k)\n'
printf '  Employees     : 6 (Engineering / Product Design / Operations / Finance)\n'
printf '  Payroll run   : 1 (May 2026, 6 items, 65 ZEC ≈ \$32.5k, pivoted to awaiting_approval)\n'
printf '  Auditor       : Jennifer Morgan, CPA (Q1 2026, 20 disclosures)\n'
printf '\nFor screenshots, log in to the staging UI and walk:\n'
printf '  - /payroll/employees       — 6 English-name employee cards\n'
printf '  - /approval/policies       — 2 realistic enterprise thresholds (USDT ≥5000 / ZEC ≥50)\n'
printf '  - /payroll/runs            — list with 1 row, status: awaiting_approval, 65 ZEC\n'
printf '  - /payroll/runs/%-3s        — detail with awaiting_approval outcome card + 6 items\n' "$RUN_ID"
printf '  - /auditor/manage          — Jennifer Morgan as scoped external auditor\n'
printf '  - /auditor/login (incog)   — log in as auditor with temp password above\n'
printf '\nNote: /approval/queue + /approval/my-pending are transfer-level pages and will\n'
printf '  be empty until a real /transfers POST hits a wallet with on-chain balance.\n'
printf '  M1 demo screenshots should use the PayrollRun awaiting_approval state instead.\n'
