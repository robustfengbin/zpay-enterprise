#!/usr/bin/env bash
# E2E happy-path smoke for zpay-enterprise M1 (F1.1 + F2.1 + F3.1).
#
# Default: assumes backend pm2 is already up against current DB; runs all
#          flows additively against a clean DB.
# --reset: drops + recreates the web3_wallet MySQL database, deletes
#          backend/.env.secrets, then restarts pm2 so secrets + admin user
#          regenerate from scratch.
#
# Exit code 0 on full pass, non-zero on first failed assertion.

set -uo pipefail

# ---------------------------------------------------------------------------
# Config — overridable via env
# ---------------------------------------------------------------------------
API="${E2E_API:-http://127.0.0.1:8080/api/v1}"
DB_CONTAINER="${E2E_DB_CONTAINER:-zhiling-admin-mysql}"
DB_USER="${E2E_DB_USER:-root}"
DB_PASS="${E2E_DB_PASS:-rootpw_change_me}"
DB_NAME="${E2E_DB_NAME:-web3_wallet}"
PM2_APP="${E2E_PM2_APP:-web3-wallet-backend}"
BACKEND_DIR="${E2E_BACKEND_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/backend}"

RESET=false
for arg in "$@"; do
  case "$arg" in
    --reset) RESET=true ;;
    -h|--help)
      sed -n '2,12p' "$0"
      exit 0 ;;
    *) echo "unknown arg: $arg" >&2; exit 2 ;;
  esac
done

# ---------------------------------------------------------------------------
# Pretty output
# ---------------------------------------------------------------------------
RED=$'\033[31m'; GREEN=$'\033[32m'; YELLOW=$'\033[33m'; CYAN=$'\033[36m'; DIM=$'\033[2m'; RST=$'\033[0m'
PASS=0
FAIL=0
STEP=0

log()   { printf '%s[%-5s]%s %s\n' "$CYAN" "INFO" "$RST" "$*"; }
warn()  { printf '%s[%-5s]%s %s\n' "$YELLOW" "WARN" "$RST" "$*"; }
err()   { printf '%s[%-5s]%s %s\n' "$RED" "ERROR" "$RST" "$*" >&2; }

step() {
  STEP=$((STEP+1))
  printf '\n%s──── #%d %s ────%s\n' "$DIM" "$STEP" "$1" "$RST"
}

assert_pass() {
  PASS=$((PASS+1))
  printf '  %s✓%s %s\n' "$GREEN" "$RST" "$1"
}

assert_fail() {
  FAIL=$((FAIL+1))
  printf '  %s✗%s %s\n' "$RED" "$RST" "$1"
  printf '    %sresponse:%s %s\n' "$DIM" "$RST" "${2:-<no body>}"
}

# Capture (stdout=body, stderr=http_code, exit=0/curl-err)
api() {
  local method="$1" path="$2" auth="${3:-}" body="${4:-}"
  local args=( -sS -X "$method" "${API}${path}" )
  [[ -n "$auth" ]] && args+=( -H "Authorization: Bearer $auth" )
  if [[ -n "$body" ]]; then
    args+=( -H 'Content-Type: application/json' --data "$body" )
  fi
  args+=( -w '\n%{http_code}' )
  curl "${args[@]}"
}

# Parse api() output: $1=full output; sets $RESP and $STATUS
parse_api() {
  RESP="${1%$'\n'*}"
  STATUS="${1##*$'\n'}"
}

jget() { python3 -c "import sys,json; v=json.load(sys.stdin);
for k in '$1'.split('.'):
    v = v[int(k)] if k.isdigit() else v[k]
print(v)"; }

# ---------------------------------------------------------------------------
# Optional reset
# ---------------------------------------------------------------------------
if $RESET; then
  step "RESET (--reset): drop+recreate DB + wipe .env.secrets + pm2 restart"

  if ! docker ps --format '{{.Names}}' | grep -q "^${DB_CONTAINER}$"; then
    err "MySQL container '${DB_CONTAINER}' not running"; exit 1
  fi
  docker exec "$DB_CONTAINER" mysql -u"$DB_USER" -p"$DB_PASS" \
    -e "DROP DATABASE IF EXISTS ${DB_NAME}; CREATE DATABASE ${DB_NAME} CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;" 2>&1 \
    | grep -v '^mysql:.*Using a password' || true
  log "DB ${DB_NAME} dropped + recreated"

  rm -f "$BACKEND_DIR/.env.secrets"
  log ".env.secrets removed (will regenerate)"

  if pm2 describe "$PM2_APP" >/dev/null 2>&1; then
    pm2 restart "$PM2_APP" >/dev/null
    log "pm2 ${PM2_APP} restarted"
  else
    (cd "$BACKEND_DIR" && pm2 start ecosystem.config.cjs >/dev/null)
    log "pm2 ${PM2_APP} started"
  fi

  printf '  %swaiting for backend to come up' "$DIM"
  for _ in $(seq 1 60); do
    if curl -fs "$API/health" >/dev/null 2>&1; then printf '%s\n' "$RST"; break; fi
    printf '.'; sleep 1
  done
  if ! curl -fs "$API/health" >/dev/null; then
    err "backend did not come up within 60s"; exit 1
  fi
fi

# ---------------------------------------------------------------------------
# Step: health
# ---------------------------------------------------------------------------
step "Backend health"
OUT=$(api GET /health)
parse_api "$OUT"
if [[ "$STATUS" == "200" ]] && [[ "$(echo "$RESP" | jget status)" == "ok" ]]; then
  assert_pass "health → ok ($STATUS)"
else
  assert_fail "health" "$RESP ($STATUS)"; exit 1
fi

# ---------------------------------------------------------------------------
# Step: admin login (reads password from .env.secrets)
# ---------------------------------------------------------------------------
step "Admin login"
ADMIN_PASS=$(grep '^WEB3_SECURITY__ADMIN_INITIAL_PASSWORD=' "$BACKEND_DIR/.env.secrets" | cut -d= -f2)
if [[ -z "$ADMIN_PASS" ]]; then
  err "could not read ADMIN_INITIAL_PASSWORD from .env.secrets"
  exit 1
fi
OUT=$(api POST /auth/login '' "{\"username\":\"admin\",\"password\":\"$ADMIN_PASS\"}")
parse_api "$OUT"
if [[ "$STATUS" == "200" ]]; then
  TOKEN=$(echo "$RESP" | jget token)
  assert_pass "admin login → token len=${#TOKEN}"
else
  assert_fail "admin login" "$RESP ($STATUS)"; exit 1
fi

# ---------------------------------------------------------------------------
# Step: wallets — create eth + zcash + activate eth (needed for transfers)
# ---------------------------------------------------------------------------
step "Wallet create (eth + zcash)"
OUT=$(api POST /wallets "$TOKEN" '{"name":"e2e-eth","chain":"ethereum"}')
parse_api "$OUT"
if [[ "$STATUS" =~ ^20[01]$ ]]; then
  ETH_WALLET_ID=$(echo "$RESP" | jget id)
  assert_pass "ethereum wallet created id=$ETH_WALLET_ID"
else
  assert_fail "eth wallet" "$RESP ($STATUS)"; exit 1
fi
OUT=$(api POST /wallets "$TOKEN" '{"name":"e2e-zec","chain":"zcash"}')
parse_api "$OUT"
if [[ "$STATUS" =~ ^20[01]$ ]]; then
  ZEC_WALLET_ID=$(echo "$RESP" | jget id)
  assert_pass "zcash wallet created id=$ZEC_WALLET_ID"
else
  assert_fail "zec wallet" "$RESP ($STATUS)"; exit 1
fi
OUT=$(api PUT "/wallets/$ETH_WALLET_ID/activate" "$TOKEN")
parse_api "$OUT"
[[ "$STATUS" == "200" ]] && assert_pass "eth wallet activated" || assert_fail "activate" "$RESP ($STATUS)"

# ---------------------------------------------------------------------------
# Step: F3.1 Employees CRUD
# ---------------------------------------------------------------------------
step "F3.1 Employees CRUD"
EMPL_CODE="E2E$$"
OUT=$(api POST /payroll/employees "$TOKEN" "{\"employee_code\":\"$EMPL_CODE\",\"name\":\"Eve\",\"wallet_address\":\"0x742d35Cc6634C0532925a3b844Bc454e4438f44e\",\"chain\":\"ethereum\",\"tags\":{\"preferred_token\":\"USDC\"},\"active\":true}")
parse_api "$OUT"
if [[ "$STATUS" =~ ^20[01]$ ]]; then
  EID=$(echo "$RESP" | jget id)
  assert_pass "employee POST id=$EID code=$EMPL_CODE"
else
  assert_fail "employee POST" "$RESP ($STATUS)"; exit 1
fi
OUT=$(api GET /payroll/employees "$TOKEN")
parse_api "$OUT"
[[ "$STATUS" == "200" ]] && assert_pass "employees GET list" || assert_fail "GET list" "$RESP ($STATUS)"
OUT=$(api PUT "/payroll/employees/$EID" "$TOKEN" '{"name":"FrozenEve","active":false}')
parse_api "$OUT"
[[ "$STATUS" == "200" ]] && [[ "$(echo "$RESP" | jget active)" == "False" ]] \
  && assert_pass "employee PUT toggled inactive" \
  || assert_fail "employee PUT" "$RESP ($STATUS)"
OUT=$(api DELETE "/payroll/employees/$EID" "$TOKEN")
parse_api "$OUT"
[[ "$STATUS" == "200" ]] && assert_pass "employee DELETE soft-deleted" || assert_fail "DELETE" "$RESP ($STATUS)"

# ---------------------------------------------------------------------------
# Step: F2.1 ApprovalPolicy CRUD (POST, GET, PUT, DELETE)
# ---------------------------------------------------------------------------
step "F2.1 ApprovalPolicy CRUD"
OUT=$(api POST /approval-policies "$TOKEN" '{"scope":"global","chain":"ethereum","token":"USDT","amount_threshold":"100.0","required_count":1,"enabled":true}')
parse_api "$OUT"
if [[ "$STATUS" =~ ^20[01]$ ]]; then
  POL_ID=$(echo "$RESP" | jget id)
  if [[ "$(echo "$RESP" | jget enabled)" == "True" ]]; then
    assert_pass "policy POST id=$POL_ID enabled=bool"
  else
    assert_fail "policy enabled not bool" "$RESP"
  fi
else
  assert_fail "policy POST" "$RESP ($STATUS)"; exit 1
fi
OUT=$(api PUT "/approval-policies/$POL_ID" "$TOKEN" '{"amount_threshold":"50.0"}')
parse_api "$OUT"
[[ "$STATUS" == "200" ]] && assert_pass "policy PUT update" || assert_fail "policy PUT" "$RESP ($STATUS)"
OUT=$(api DELETE "/approval-policies/$POL_ID" "$TOKEN")
parse_api "$OUT"
[[ "$STATUS" == "200" ]] && assert_pass "policy DELETE" || assert_fail "policy DELETE" "$RESP ($STATUS)"

# ---------------------------------------------------------------------------
# Step: F1.1 ViewingKey 3-step (export → download → re-download 403)
# ---------------------------------------------------------------------------
step "F1.1 ViewingKey 3-step (zcash)"
OUT=$(api POST "/wallets/$ZEC_WALLET_ID/viewing-keys/export" "$TOKEN" "{\"key_type\":\"UFVK\",\"password\":\"$ADMIN_PASS\"}")
parse_api "$OUT"
if [[ "$STATUS" =~ ^20[01]$ ]]; then
  DTOK=$(echo "$RESP" | jget download_token)
  assert_pass "VK export issued token len=${#DTOK}"
else
  assert_fail "VK export" "$RESP ($STATUS)"; exit 1
fi
OUT=$(api GET "/viewing-keys/download/$DTOK" "$TOKEN")
parse_api "$OUT"
if [[ "$STATUS" == "200" ]] && [[ "$RESP" == *uview1* ]]; then
  assert_pass "VK download returned ZIP-316 uview1 string"
else
  assert_fail "VK download" "$RESP ($STATUS)"; exit 1
fi
OUT=$(api GET "/viewing-keys/download/$DTOK" "$TOKEN")
parse_api "$OUT"
[[ "$STATUS" == "403" ]] && assert_pass "VK re-download → 403 (single-use)" || assert_fail "VK re-download not 403" "$RESP ($STATUS)"

# ---------------------------------------------------------------------------
# Step: F1.1 Auditor full flow
# ---------------------------------------------------------------------------
step "F1.1 Auditor full flow"
AEMAIL="e2e+$$@audit.local"
OUT=$(api POST /auditors "$TOKEN" "{\"email\":\"$AEMAIL\",\"name\":\"E2E Auditor\",\"wallet_ids\":[$ETH_WALLET_ID],\"scope_start\":\"2026-01-01T00:00:00Z\",\"scope_end\":\"2026-12-31T00:00:00Z\",\"max_count\":5}")
parse_api "$OUT"
if [[ "$STATUS" =~ ^20[01]$ ]]; then
  TEMPPW=$(echo "$RESP" | jget temp_password)
  AUDID=$(echo "$RESP" | jget auditor_id)
  assert_pass "auditor created id=$AUDID"
else
  assert_fail "auditor create" "$RESP ($STATUS)"; exit 1
fi
OUT=$(api POST /auditor/login '' "{\"email\":\"$AEMAIL\",\"password\":\"$TEMPPW\"}")
parse_api "$OUT"
if [[ "$STATUS" == "200" ]]; then
  ATOKEN=$(echo "$RESP" | jget token)
  assert_pass "auditor login → JWT"
else
  assert_fail "auditor login" "$RESP ($STATUS)"; exit 1
fi
OUT=$(api GET /auditor/me "$ATOKEN")
parse_api "$OUT"
[[ "$STATUS" == "200" ]] && assert_pass "auditor /me" || assert_fail "auditor /me" "$RESP ($STATUS)"
OUT=$(api GET /auditor/wallets "$ATOKEN")
parse_api "$OUT"
if [[ "$STATUS" == "200" ]] && [[ "$RESP" == *"wallet_name"* ]] && [[ "$RESP" == *"scope_start"* ]]; then
  assert_pass "auditor /wallets returns 11-field shape"
else
  assert_fail "auditor /wallets shape" "$RESP ($STATUS)"
fi
OUT=$(api GET "/auditor/wallets/$ETH_WALLET_ID/balance" "$ATOKEN")
parse_api "$OUT"
[[ "$STATUS" == "200" ]] && [[ "$RESP" == *"native_balance"* ]] \
  && assert_pass "auditor /balance returns native_balance" \
  || assert_fail "auditor /balance" "$RESP ($STATUS)"
OUT=$(api GET "/auditor/wallets/$ETH_WALLET_ID/transfers?limit=10" "$ATOKEN")
parse_api "$OUT"
[[ "$STATUS" == "200" ]] && [[ "$RESP" == *"scope_start"* ]] && [[ "$RESP" == *"transfers"* ]] \
  && assert_pass "auditor /transfers returns scope+transfers shape" \
  || assert_fail "auditor /transfers" "$RESP ($STATUS)"
# Cross-auth check — admin token must NOT reach /auditor/*
OUT=$(api GET /auditor/me "$TOKEN")
parse_api "$OUT"
[[ "$STATUS" == "401" || "$STATUS" == "403" ]] \
  && assert_pass "admin token rejected from /auditor/* ($STATUS)" \
  || assert_fail "admin token unexpectedly accepted at /auditor/me" "$RESP ($STATUS)"

# ---------------------------------------------------------------------------
# Step: F3.1 PayrollRun (create + execute below threshold)
# ---------------------------------------------------------------------------
step "F3.1 PayrollRun lifecycle"
OUT=$(api POST /payroll/runs "$TOKEN" "{\"pay_period\":\"e2e\",\"source_wallet_id\":$ETH_WALLET_ID,\"items\":[{\"employee_address\":\"0x742d35Cc6634C0532925a3b844Bc454e4438f44e\",\"amount\":\"0.001\",\"memo\":\"e2e\"}]}")
parse_api "$OUT"
if [[ "$STATUS" =~ ^20[01]$ ]] && [[ "$RESP" == *"validation_errors"* ]]; then
  RUN_ID=$(echo "$RESP" | jget run_id)
  assert_pass "run created id=$RUN_ID"
else
  assert_fail "run create" "$RESP ($STATUS)"; exit 1
fi
# Invalid row should return 422 + validation_errors[0]
OUT=$(api POST /payroll/runs "$TOKEN" "{\"pay_period\":\"e2e-invalid\",\"source_wallet_id\":$ETH_WALLET_ID,\"items\":[{\"employee_address\":\"bad_addr\",\"amount\":\"1.0\"}]}")
parse_api "$OUT"
if [[ "$STATUS" == "422" ]] && [[ "$RESP" == *"invalid ethereum address"* ]]; then
  assert_pass "invalid row → 422 with row-level error"
else
  assert_fail "expected 422 invalid row" "$RESP ($STATUS)"
fi
OUT=$(api GET "/payroll/runs/$RUN_ID" "$TOKEN")
parse_api "$OUT"
[[ "$STATUS" == "200" ]] && [[ "$RESP" == *'"run"'* ]] && [[ "$RESP" == *'"items"'* ]] \
  && assert_pass "run detail returns {run, items}" \
  || assert_fail "run detail" "$RESP ($STATUS)"
# Execute — 0 ETH balance ⇒ items fail, but the tagged union should still come back
OUT=$(api POST "/payroll/runs/$RUN_ID/execute" "$TOKEN")
parse_api "$OUT"
if [[ "$STATUS" == "200" ]] && [[ "$RESP" == *'"result"'* ]]; then
  RESULT=$(echo "$RESP" | jget result)
  assert_pass "run execute → ExecuteRunOutcome result=$RESULT"
else
  assert_fail "run execute" "$RESP ($STATUS)"
fi

# ---------------------------------------------------------------------------
# Step: F2.1 auto-pivot — policy threshold matches → execute returns
# ExecuteRunOutcome.AwaitingApproval (covers the approval branch of the
# tagged union the F3.1 step skipped).
# ---------------------------------------------------------------------------
step "F2.1 auto-pivot via PayrollRun (run total ≥ threshold)"
# Tiny policy for ETH so a 0.001 amount is above threshold
OUT=$(api POST /approval-policies "$TOKEN" '{"scope":"global","chain":"ethereum","token":"ETH","amount_threshold":"0.0001","required_count":1,"enabled":true}')
parse_api "$OUT"
if [[ "$STATUS" =~ ^20[01]$ ]]; then
  PIVOT_POL_ID=$(echo "$RESP" | jget id)
  assert_pass "pivot policy created id=$PIVOT_POL_ID threshold=0.0001"
else
  assert_fail "pivot policy" "$RESP ($STATUS)"
fi
OUT=$(api POST /payroll/runs "$TOKEN" "{\"pay_period\":\"e2e-pivot\",\"source_wallet_id\":$ETH_WALLET_ID,\"items\":[{\"employee_address\":\"0x742d35Cc6634C0532925a3b844Bc454e4438f44e\",\"amount\":\"0.001\"}]}")
parse_api "$OUT"
PIVOT_RUN_ID=$(echo "$RESP" | jget run_id 2>/dev/null || echo "")
[[ -n "$PIVOT_RUN_ID" ]] && assert_pass "pivot run created id=$PIVOT_RUN_ID" || assert_fail "pivot run create" "$RESP ($STATUS)"
OUT=$(api POST "/payroll/runs/$PIVOT_RUN_ID/execute" "$TOKEN")
parse_api "$OUT"
if [[ "$STATUS" == "200" ]] && [[ "$(echo "$RESP" | jget result)" == "awaiting_approval" ]]; then
  assert_pass "execute → result=awaiting_approval (run pivoted to maker/checker)"
else
  assert_fail "expected awaiting_approval" "$RESP ($STATUS)"
fi
# Cancel from awaiting_approval (verifies cancel_run extended state per 40d1588)
OUT=$(api POST "/payroll/runs/$PIVOT_RUN_ID/cancel" "$TOKEN")
parse_api "$OUT"
[[ "$STATUS" == "200" ]] && assert_pass "cancel awaiting_approval run" || assert_fail "cancel" "$RESP ($STATUS)"
# Clean up the pivot policy so it doesn't shadow future runs
api DELETE "/approval-policies/$PIVOT_POL_ID" "$TOKEN" >/dev/null || true

# ---------------------------------------------------------------------------
# Step: F1.1 Disclosure async lifecycle — POST 202 + poll → ready + download
# ---------------------------------------------------------------------------
step "F1.1 Disclosure async lifecycle (granularity=address)"
OUT=$(api POST "/wallets/$ZEC_WALLET_ID/payment-disclosures" "$TOKEN" '{"granularity":"address","scope_param":{"address":"u1abc"},"format":"json"}')
parse_api "$OUT"
INIT_STATUS=$(echo "$RESP" | jget status 2>/dev/null || echo "")
if [[ "$STATUS" =~ ^20[0-2]$ ]] && [[ "$INIT_STATUS" == "generating" || "$INIT_STATUS" == "ready" ]]; then
  DID=$(echo "$RESP" | jget disclosure_id)
  assert_pass "disclosure POST → 202 (status=$INIT_STATUS, often races to ready) id=$DID"
else
  assert_fail "disclosure POST" "$RESP ($STATUS)"; exit 1
fi
# Poll up to 15s for status flip — address granularity is in-memory DB scan, usually <1s
DSTATUS=""
for _ in $(seq 1 15); do
  sleep 1
  OUT=$(api GET "/payment-disclosures/$DID" "$TOKEN")
  parse_api "$OUT"
  DSTATUS=$(echo "$RESP" | jget status)
  [[ "$DSTATUS" == "ready" || "$DSTATUS" == "failed" ]] && break
done
[[ "$DSTATUS" == "ready" ]] && assert_pass "disclosure ready after polling" \
  || assert_fail "disclosure did not become ready (final=$DSTATUS)" "$RESP"
# Download body — verify the ZIP-307-enterprise shape
OUT=$(api GET "/payment-disclosures/$DID/download" "$TOKEN")
parse_api "$OUT"
if [[ "$STATUS" == "200" ]] && [[ "$RESP" == *'"zip_version"'* ]] && [[ "$RESP" == *'"actions"'* ]]; then
  assert_pass "disclosure download returns ZIP-307-enterprise body"
else
  assert_fail "disclosure download" "$RESP ($STATUS)"
fi
# Validation guard — scope_param must match granularity
OUT=$(api POST "/wallets/$ZEC_WALLET_ID/payment-disclosures" "$TOKEN" '{"granularity":"tx","scope_param":{"address":"foo"},"format":"json"}')
parse_api "$OUT"
[[ "$STATUS" == "400" ]] && [[ "$RESP" == *"tx_hash"* ]] \
  && assert_pass "scope_param mismatch → 400 with hint" \
  || assert_fail "scope_param mismatch not rejected" "$RESP ($STATUS)"

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
TOTAL=$((PASS+FAIL))
printf '\n%s═══ E2E Summary ═══%s\n' "$DIM" "$RST"
printf '  total : %d\n' "$TOTAL"
printf '  pass  : %s%d%s\n' "$GREEN" "$PASS" "$RST"
printf '  fail  : %s%d%s\n' "${RED}" "$FAIL" "$RST"
if (( FAIL > 0 )); then
  printf '\n%sE2E FAILED%s — see above for the first failing assertion.\n' "$RED" "$RST"
  exit 1
fi
printf '\n%sE2E PASSED%s\n' "$GREEN" "$RST"
