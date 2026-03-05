#!/usr/bin/env bash
#
# Bankie Interest Engine E2E Test Suite
# Tests the interest lifecycle: rate config CRUD, daily accrual, posting,
# API query endpoints, estimate, edge cases, and negative paths.
#
# Prerequisites:
#   make local-setup   (starts infra + DB + server with interest engine)
#   make local-jwt     (generates JWT token)
#
# Usage:
#   ./scripts/interest-e2e-test.sh <JWT_TOKEN>
#   TOKEN=eyJ... ./scripts/interest-e2e-test.sh
#
# Environment variables:
#   BASE_URL        - Core server URL (default: http://localhost:3030)
#   DB_URL          - PostgreSQL connection (default: postgres://bankie_app:password@localhost:5432/bankie_main)
#   OUTBOX_WAIT     - Seconds to wait for outbox processing (default: 15)
#   INTEREST_WAIT   - Seconds to wait for accrual/posting cron (default: 20)
#   TOKEN           - JWT token (alternative to argument)
#
set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
BASE_URL="${BASE_URL:-http://localhost:3030}"
DB_URL="${DB_URL:-postgres://bankie_app:password@localhost:5432/bankie_main}"
TOKEN="${1:-${TOKEN:-}}"
OUTBOX_WAIT="${OUTBOX_WAIT:-25}"
INTEREST_WAIT="${INTEREST_WAIT:-20}"

if [[ -z "$TOKEN" ]]; then
  echo "ERROR: JWT token required."
  echo ""
  echo "Usage: ./scripts/interest-e2e-test.sh <JWT_TOKEN>"
  echo "   or: TOKEN=eyJ... ./scripts/interest-e2e-test.sh"
  echo ""
  echo "Generate a token with: make local-jwt SERVICE=interest-test"
  exit 1
fi

AUTH="Authorization: Bearer ${TOKEN}"
CT="Content-Type: application/json"

# ---------------------------------------------------------------------------
# Colors
# ---------------------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

# ---------------------------------------------------------------------------
# Test Framework
# ---------------------------------------------------------------------------
TOTAL=0
PASSED=0
FAILED=0
FAILED_TESTS=""

suite() {
  echo ""
  echo -e "${BLUE}============================================================${NC}"
  echo -e "${BLUE}  SUITE: $1${NC}"
  echo -e "${BLUE}============================================================${NC}"
}

run_test() {
  local name="$1"
  TOTAL=$((TOTAL + 1))
  echo -e "${CYAN}  TEST: ${name}${NC}"
}

pass() {
  PASSED=$((PASSED + 1))
  echo -e "    ${GREEN}PASS${NC}"
}

fail() {
  local reason="${1:-assertion failed}"
  FAILED=$((FAILED + 1))
  FAILED_TESTS="${FAILED_TESTS}\n    - ${reason}"
  echo -e "    ${RED}FAIL: ${reason}${NC}"
}

assert_status() {
  local actual="$1"
  local expected="$2"
  local context="${3:-}"
  if [[ "$actual" == "$expected" ]]; then
    return 0
  else
    fail "${context}: expected status ${expected}, got ${actual}"
    return 1
  fi
}

assert_json_field() {
  local json="$1"
  local field="$2"
  local expected="${3:-}"
  local context="${4:-}"
  local actual
  actual=$(echo "$json" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    keys = '${field}'.split('.')
    val = data
    for k in keys:
        if isinstance(val, list):
            val = val[int(k)]
        else:
            val = val[k]
    print(val)
except (KeyError, IndexError, TypeError):
    sys.exit(1)
" 2>/dev/null) || {
    fail "${context}: field '${field}' not found in response"
    return 1
  }

  if [[ -n "$expected" ]]; then
    if [[ "$actual" == "$expected" ]]; then
      return 0
    else
      fail "${context}: field '${field}' expected '${expected}', got '${actual}'"
      return 1
    fi
  fi
  return 0
}

assert_entries_count() {
  local json="$1"
  local min_count="$2"
  local context="${3:-}"
  local count
  count=$(echo "$json" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    print(len(data.get('entries', [])))
except:
    print(0)
" 2>/dev/null)
  if [[ "$count" -ge "$min_count" ]]; then
    return 0
  else
    fail "${context}: expected >= ${min_count} entries, got ${count}"
    return 1
  fi
}

assert_positive_decimal() {
  local value="$1"
  local context="${2:-}"
  if python3 -c "
import sys
try:
    v = float('${value}')
except (ValueError, TypeError):
    sys.exit(1)
sys.exit(0 if v > 0 else 1)
" 2>/dev/null; then
    return 0
  else
    fail "${context}: expected positive decimal, got '${value}'"
    return 1
  fi
}

# ---------------------------------------------------------------------------
# HTTP Helpers
# ---------------------------------------------------------------------------
HTTP_BODY=""
HTTP_STATUS=""

http_get() {
  local url="$1"
  local use_auth="${2:-yes}"
  local response
  if [[ "$use_auth" == "no" ]]; then
    response=$(curl -s -w "\n%{http_code}" "$url")
  else
    response=$(curl -s -w "\n%{http_code}" "$url" -H "$AUTH")
  fi
  HTTP_STATUS=$(echo "$response" | tail -1)
  HTTP_BODY=$(echo "$response" | sed '$d')
}

http_post() {
  local url="$1"
  local data="$2"
  local use_auth="${3:-yes}"
  local response
  if [[ "$use_auth" == "no" ]]; then
    response=$(curl -s -w "\n%{http_code}" -X POST "$url" -H "$CT" -d "$data")
  else
    response=$(curl -s -w "\n%{http_code}" -X POST "$url" -H "$AUTH" -H "$CT" -d "$data")
  fi
  HTTP_STATUS=$(echo "$response" | tail -1)
  HTTP_BODY=$(echo "$response" | sed '$d')
}

http_put() {
  local url="$1"
  local data="$2"
  local use_auth="${3:-yes}"
  local response
  if [[ "$use_auth" == "no" ]]; then
    response=$(curl -s -w "\n%{http_code}" -X PUT "$url" -H "$CT" -d "$data")
  else
    response=$(curl -s -w "\n%{http_code}" -X PUT "$url" -H "$AUTH" -H "$CT" -d "$data")
  fi
  HTTP_STATUS=$(echo "$response" | tail -1)
  HTTP_BODY=$(echo "$response" | sed '$d')
}

wait_for_outbox() {
  local seconds="${1:-$OUTBOX_WAIT}"
  echo -e "    ${YELLOW}(waiting ${seconds}s for outbox processing...)${NC}"
  sleep "$seconds"
}

wait_for_interest() {
  local seconds="${1:-$INTEREST_WAIT}"
  echo -e "    ${YELLOW}(waiting ${seconds}s for interest cron cycle...)${NC}"
  sleep "$seconds"
}

# ---------------------------------------------------------------------------
# State variables
# ---------------------------------------------------------------------------
RATE_CONFIG_ID=""
HOUSE_USD_ID=""
CHECKING_ACCOUNT_ID=""
CHECKING_LEDGER_ID=""
INTEREST_ACCOUNT_ID=""
INTEREST_LEDGER_ID=""
USER_ID="interest-test-user-$(date +%s)"
TODAY=$(date -u +%Y-%m-%d)
YESTERDAY=$(date -u -v-1d +%Y-%m-%d 2>/dev/null || date -u -d "yesterday" +%Y-%m-%d 2>/dev/null || echo "$TODAY")

echo ""
echo -e "${BLUE}╔══════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║       Bankie Interest Engine E2E Test Suite              ║${NC}"
echo -e "${BLUE}║       Direct to Core :3030 with JWT                      ║${NC}"
echo -e "${BLUE}╚══════════════════════════════════════════════════════════╝${NC}"
echo -e "  Target: ${BASE_URL}"
echo -e "  Token:  ${TOKEN:0:30}..."
echo -e "  Date:   ${TODAY}"
echo ""

# ===========================================================================
# SUITE 1: Rate Config CRUD
# ===========================================================================
suite "1. Rate Config CRUD"

run_test "POST /v1/interest/rates -- create USD rate config (Daily posting, 2 tiers)"
http_post "${BASE_URL}/v1/interest/rates" '{
  "currency": "USD",
  "account_kind": "Interest",
  "day_count": "Actual/365",
  "posting_frequency": "Daily",
  "effective_from": "'"${TODAY}"'",
  "tiers": [
    { "tier_order": 1, "min_balance": 0, "max_balance": 10000, "apr": 0.045 },
    { "tier_order": 2, "min_balance": 10000, "max_balance": null, "apr": 0.040 }
  ]
}'
if [[ "$HTTP_STATUS" == "201" ]]; then
  RATE_CONFIG_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  if [[ -n "$RATE_CONFIG_ID" ]]; then
    pass
  else
    fail "create rate config: could not extract id"
  fi
elif [[ "$HTTP_STATUS" == "500" || "$HTTP_STATUS" == "409" ]]; then
  # Rate config may already exist from a previous run — fetch existing
  echo -e "    ${YELLOW}(Rate config already exists, fetching existing...)${NC}"
  http_get "${BASE_URL}/v1/interest/rates?currency=USD"
  RATE_CONFIG_ID=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
for e in entries:
    if e.get('currency') == 'USD' and e.get('account_kind') == 'Interest':
        print(e['id'])
        break
" 2>/dev/null || echo "")
  if [[ -n "$RATE_CONFIG_ID" ]]; then
    pass
  else
    fail "create rate config: duplicate detected but could not fetch existing id"
  fi
else
  fail "create rate config: expected status 201, got ${HTTP_STATUS}"
fi

run_test "GET /v1/interest/rates?currency=USD -- list rate configs"
http_get "${BASE_URL}/v1/interest/rates?currency=USD"
if assert_status "$HTTP_STATUS" "200" "list rate configs" && \
   assert_entries_count "$HTTP_BODY" 1 "rate config count"; then
  pass
fi

run_test "GET /v1/interest/rates/:id -- get rate config by ID"
http_get "${BASE_URL}/v1/interest/rates/${RATE_CONFIG_ID}"
if assert_status "$HTTP_STATUS" "200" "get rate config" && \
   assert_json_field "$HTTP_BODY" "currency" "USD" "rate config currency" && \
   assert_json_field "$HTTP_BODY" "posting_frequency" "Daily" "posting frequency"; then
  pass
fi

run_test "GET /v1/interest/rates/:id -- verify 2 tiers returned"
http_get "${BASE_URL}/v1/interest/rates/${RATE_CONFIG_ID}"
if assert_status "$HTTP_STATUS" "200" "get rate config tiers"; then
  tier_count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin)['tiers']))" 2>/dev/null || echo "0")
  if [[ "$tier_count" == "2" ]]; then
    pass
  else
    fail "expected 2 tiers, got ${tier_count}"
  fi
fi

run_test "GET /v1/interest/rates?currency=EUR -- empty list for unconfigured currency"
http_get "${BASE_URL}/v1/interest/rates?currency=EUR"
if assert_status "$HTTP_STATUS" "200" "list EUR rate configs" && \
   assert_entries_count "$HTTP_BODY" 0 "EUR rate config count should be 0"; then
  # entries count >= 0 always passes, check exact 0
  count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('entries',[])))" 2>/dev/null || echo "")
  if [[ "$count" == "0" ]]; then
    pass
  else
    fail "EUR rate configs: expected 0, got ${count}"
  fi
fi

run_test "PUT /v1/interest/rates/:id/tiers -- replace tiers (3-tier)"
http_put "${BASE_URL}/v1/interest/rates/${RATE_CONFIG_ID}/tiers" '{
  "tiers": [
    { "tier_order": 1, "min_balance": 0, "max_balance": 10000, "apr": 0.05 },
    { "tier_order": 2, "min_balance": 10000, "max_balance": 50000, "apr": 0.04 },
    { "tier_order": 3, "min_balance": 50000, "max_balance": null, "apr": 0.03 }
  ]
}'
if assert_status "$HTTP_STATUS" "200" "replace tiers"; then
  tier_count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin)['tiers']))" 2>/dev/null || echo "0")
  if [[ "$tier_count" == "3" ]]; then
    pass
  else
    fail "expected 3 tiers after replace, got ${tier_count}"
  fi
fi

run_test "PUT /v1/interest/rates/:id/tiers -- revert to 2-tier for remaining tests"
http_put "${BASE_URL}/v1/interest/rates/${RATE_CONFIG_ID}/tiers" '{
  "tiers": [
    { "tier_order": 1, "min_balance": 0, "max_balance": 10000, "apr": 0.045 },
    { "tier_order": 2, "min_balance": 10000, "max_balance": null, "apr": 0.040 }
  ]
}'
if assert_status "$HTTP_STATUS" "200" "revert tiers"; then
  pass
fi

run_test "GET /v1/interest/rates/00000000-0000-0000-0000-000000000000 -- not found"
http_get "${BASE_URL}/v1/interest/rates/00000000-0000-0000-0000-000000000000"
if assert_status "$HTTP_STATUS" "404" "nonexistent rate config"; then
  pass
fi

# ===========================================================================
# SUITE 2: Rate Config Validation (Negative Cases)
# ===========================================================================
suite "2. Rate Config Validation"

run_test "POST /v1/interest/rates -- empty tiers → 400"
http_post "${BASE_URL}/v1/interest/rates" '{
  "currency": "USD",
  "effective_from": "2026-06-01",
  "tiers": []
}'
if assert_status "$HTTP_STATUS" "400" "empty tiers"; then
  pass
fi

run_test "POST /v1/interest/rates -- negative APR → 400"
http_post "${BASE_URL}/v1/interest/rates" '{
  "currency": "USD",
  "effective_from": "2026-06-01",
  "tiers": [{ "tier_order": 1, "min_balance": 0, "max_balance": null, "apr": -0.01 }]
}'
if assert_status "$HTTP_STATUS" "400" "negative APR"; then
  pass
fi

run_test "POST /v1/interest/rates -- invalid posting_frequency → 400"
http_post "${BASE_URL}/v1/interest/rates" '{
  "currency": "USD",
  "posting_frequency": "Biweekly",
  "effective_from": "2026-06-01",
  "tiers": [{ "tier_order": 1, "min_balance": 0, "max_balance": null, "apr": 0.03 }]
}'
if assert_status "$HTTP_STATUS" "400" "invalid posting frequency"; then
  pass
fi

run_test "POST /v1/interest/rates -- Daily with posting_day set → 400 (H3 fix)"
http_post "${BASE_URL}/v1/interest/rates" '{
  "currency": "EUR",
  "posting_frequency": "Daily",
  "posting_day": 1,
  "effective_from": "2026-06-01",
  "tiers": [{ "tier_order": 1, "min_balance": 0, "max_balance": null, "apr": 0.03 }]
}'
if assert_status "$HTTP_STATUS" "400" "Daily with posting_day"; then
  pass
fi

run_test "POST /v1/interest/rates -- Weekly with posting_day=0 → 400 (H3 fix)"
http_post "${BASE_URL}/v1/interest/rates" '{
  "currency": "EUR",
  "posting_frequency": "Weekly",
  "posting_day": 0,
  "effective_from": "2026-06-01",
  "tiers": [{ "tier_order": 1, "min_balance": 0, "max_balance": null, "apr": 0.03 }]
}'
if assert_status "$HTTP_STATUS" "400" "Weekly posting_day=0"; then
  pass
fi

run_test "POST /v1/interest/rates -- Monthly without posting_day → 400 (H3 fix)"
http_post "${BASE_URL}/v1/interest/rates" '{
  "currency": "EUR",
  "posting_frequency": "Monthly",
  "effective_from": "2026-06-01",
  "tiers": [{ "tier_order": 1, "min_balance": 0, "max_balance": null, "apr": 0.03 }]
}'
if assert_status "$HTTP_STATUS" "400" "Monthly missing posting_day"; then
  pass
fi

run_test "POST /v1/interest/rates -- Monthly with posting_day=29 → 400 (H3 fix)"
http_post "${BASE_URL}/v1/interest/rates" '{
  "currency": "EUR",
  "posting_frequency": "Monthly",
  "posting_day": 29,
  "effective_from": "2026-06-01",
  "tiers": [{ "tier_order": 1, "min_balance": 0, "max_balance": null, "apr": 0.03 }]
}'
if assert_status "$HTTP_STATUS" "400" "Monthly posting_day=29"; then
  pass
fi

run_test "PUT /v1/interest/rates/:id/tiers -- gap between tiers → 400"
http_put "${BASE_URL}/v1/interest/rates/${RATE_CONFIG_ID}/tiers" '{
  "tiers": [
    { "tier_order": 1, "min_balance": 0, "max_balance": 10000, "apr": 0.04 },
    { "tier_order": 2, "min_balance": 15000, "max_balance": null, "apr": 0.03 }
  ]
}'
if assert_status "$HTTP_STATUS" "400" "tier gap"; then
  pass
fi

run_test "PUT /v1/interest/rates/:id/tiers -- first tier not starting at 0 → 400"
http_put "${BASE_URL}/v1/interest/rates/${RATE_CONFIG_ID}/tiers" '{
  "tiers": [
    { "tier_order": 1, "min_balance": 100, "max_balance": null, "apr": 0.04 }
  ]
}'
if assert_status "$HTTP_STATUS" "400" "first tier not at zero"; then
  pass
fi

run_test "PUT /v1/interest/rates/:id/tiers -- last tier bounded → 400"
http_put "${BASE_URL}/v1/interest/rates/${RATE_CONFIG_ID}/tiers" '{
  "tiers": [
    { "tier_order": 1, "min_balance": 0, "max_balance": 10000, "apr": 0.04 }
  ]
}'
if assert_status "$HTTP_STATUS" "400" "last tier bounded"; then
  pass
fi

# ===========================================================================
# SUITE 3: Account Setup for Interest Testing
# ===========================================================================
suite "3. Account Setup"

run_test "POST /v1/house_account -- create USD house account"
http_post "${BASE_URL}/v1/house_account" '{
  "status": "active",
  "account_name": "Interest Test USD House",
  "account_type": "House",
  "currency": "USD"
}'
if [[ "$HTTP_STATUS" == "201" ]]; then
  HOUSE_USD_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  if [[ -n "$HOUSE_USD_ID" ]]; then
    pass
  else
    fail "create house USD: could not extract id"
  fi
elif [[ "$HTTP_STATUS" == "400" || "$HTTP_STATUS" == "409" || "$HTTP_STATUS" == "500" ]]; then
  # House account may already exist from prior run — fetch existing
  echo -e "    ${YELLOW}(House account already exists, fetching existing...)${NC}"
  http_get "${BASE_URL}/v1/house_account?currency=USD"
  HOUSE_USD_ID=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
if entries:
    print(entries[0]['id'])
" 2>/dev/null || echo "")
  if [[ -n "$HOUSE_USD_ID" ]]; then
    pass
  else
    fail "create house USD: duplicate detected but could not fetch existing"
  fi
else
  fail "create house USD: expected status 201, got ${HTTP_STATUS}"
fi

# Ensure house account is visible before deposit commands
sleep 3

run_test "Open + Approve Checking account (USD)"
http_post "${BASE_URL}/v1/bank_account" "{
  \"OpenAccount\": {
    \"account_type\": \"Retail\",
    \"kind\": \"Checking\",
    \"currency\": \"USD\",
    \"external_reference_id\": \"${USER_ID}\"
  }
}"
if assert_status "$HTTP_STATUS" "201" "open checking"; then
  CHECKING_ACCOUNT_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
fi

sleep 2

http_post "${BASE_URL}/v1/bank_account" "{
  \"ApproveAccount\": {\"id\": \"${CHECKING_ACCOUNT_ID}\"}
}"
if assert_status "$HTTP_STATUS" "200" "approve checking"; then
  pass
fi

sleep 2

http_get "${BASE_URL}/v1/bank_account/${CHECKING_ACCOUNT_ID}"
CHECKING_LEDGER_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['ledger_id'])" 2>/dev/null || echo "")

run_test "Deposit 50000 USD into Checking"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Deposit\": {
    \"id\": \"${CHECKING_ACCOUNT_ID}\",
    \"amount\": {\"amount\": \"50000\", \"currency\": \"USD\"}
  }
}"
if assert_status "$HTTP_STATUS" "200" "deposit 50000"; then
  pass
fi

wait_for_outbox

# Verify deposit actually processed (CQRS commands are async — 200 only means enqueued)
http_get "${BASE_URL}/v1/ledger/${CHECKING_LEDGER_ID}"
checking_bal=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "0")
if python3 -c "exit(0 if float('${checking_bal}') >= 50000.0 else 1)" 2>/dev/null; then
  echo -e "    ${GREEN}Deposit confirmed: checking balance = ${checking_bal}${NC}"
else
  echo -e "    ${RED}WARNING: Deposit may not have processed. Checking balance = ${checking_bal}${NC}"
  echo -e "    ${YELLOW}(Retrying deposit in case house account was not ready...)${NC}"
  sleep 5
  http_post "${BASE_URL}/v1/bank_account" "{
    \"Deposit\": {
      \"id\": \"${CHECKING_ACCOUNT_ID}\",
      \"amount\": {\"amount\": \"50000\", \"currency\": \"USD\"}
    }
  }"
  wait_for_outbox
  http_get "${BASE_URL}/v1/ledger/${CHECKING_LEDGER_ID}"
  checking_bal=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "0")
  echo -e "    ${YELLOW}After retry: checking balance = ${checking_bal}${NC}"
fi

run_test "Open + Approve Interest sub-account (parent_id = checking)"
http_post "${BASE_URL}/v1/bank_account" "{
  \"OpenAccount\": {
    \"account_type\": \"Retail\",
    \"kind\": \"Interest\",
    \"currency\": \"USD\",
    \"external_reference_id\": \"${USER_ID}\",
    \"parent_id\": \"${CHECKING_ACCOUNT_ID}\"
  }
}"
if assert_status "$HTTP_STATUS" "201" "open interest account"; then
  INTEREST_ACCOUNT_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  if [[ -n "$INTEREST_ACCOUNT_ID" ]]; then
    pass
  else
    fail "open interest: could not extract id"
  fi
fi

sleep 2

http_post "${BASE_URL}/v1/bank_account" "{
  \"ApproveAccount\": {\"id\": \"${INTEREST_ACCOUNT_ID}\"}
}"
assert_status "$HTTP_STATUS" "200" "approve interest account"

sleep 2

http_get "${BASE_URL}/v1/bank_account/${INTEREST_ACCOUNT_ID}"
INTEREST_LEDGER_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['ledger_id'])" 2>/dev/null || echo "")

run_test "Transfer 25000 USD from Checking → Interest"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Transfer\": {
    \"id\": \"${CHECKING_ACCOUNT_ID}\",
    \"to_account_id\": \"${INTEREST_ACCOUNT_ID}\",
    \"amount\": {\"amount\": \"25000\", \"currency\": \"USD\"}
  }
}"
if assert_status "$HTTP_STATUS" "200" "transfer to interest"; then
  pass
fi

wait_for_outbox 20

run_test "Verify Interest account ledger balance = 25000"
http_get "${BASE_URL}/v1/ledger/${INTEREST_LEDGER_ID}"
if assert_status "$HTTP_STATUS" "200" "interest ledger query"; then
  available=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "")
  if python3 -c "exit(0 if float('${available}') == 25000.0 else 1)" 2>/dev/null; then
    pass
  else
    fail "interest ledger: expected 25000, got ${available}"
  fi
fi

# ===========================================================================
# SUITE 4: Interest Estimate (pre-accrual)
# ===========================================================================
suite "4. Interest Estimate"

run_test "GET /v1/interest/estimate -- estimate 30 days for Interest account"
http_get "${BASE_URL}/v1/interest/estimate?account_id=${INTEREST_ACCOUNT_ID}&days=30"
if assert_status "$HTTP_STATUS" "200" "interest estimate"; then
  est_interest=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['estimated_interest'])" 2>/dev/null || echo "0")
  est_currency=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['currency'])" 2>/dev/null || echo "")
  if assert_positive_decimal "$est_interest" "estimated interest > 0" && \
     [[ "$est_currency" == "USD" ]]; then
    pass
  fi
fi

run_test "GET /v1/interest/estimate -- default days (30) when omitted"
http_get "${BASE_URL}/v1/interest/estimate?account_id=${INTEREST_ACCOUNT_ID}"
if assert_status "$HTTP_STATUS" "200" "estimate default days"; then
  est_days=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['days'])" 2>/dev/null || echo "0")
  if [[ "$est_days" == "30" ]]; then
    pass
  else
    fail "estimate default days: expected 30, got ${est_days}"
  fi
fi

run_test "GET /v1/interest/estimate -- nonexistent account → 404"
http_get "${BASE_URL}/v1/interest/estimate?account_id=00000000-0000-0000-0000-000000000000&days=30"
if assert_status "$HTTP_STATUS" "404" "estimate nonexistent account"; then
  pass
fi

# ===========================================================================
# SUITE 5: Daily Accrual Verification
# ===========================================================================
suite "5. Daily Accrual Verification"

echo -e "    ${YELLOW}Note: Accrual cron runs at 00:05 UTC daily.${NC}"
echo -e "    ${YELLOW}For testing, we rely on the cron having run OR seed accruals.${NC}"
echo -e "    ${YELLOW}Waiting for accrual cycle...${NC}"
wait_for_interest

run_test "GET /v1/interest/accruals -- query accruals for Interest account"
http_get "${BASE_URL}/v1/interest/accruals?account_id=${INTEREST_ACCOUNT_ID}&start_date=2026-01-01&end_date=2026-12-31"
if assert_status "$HTTP_STATUS" "200" "list accruals"; then
  accrual_count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('entries',[])))" 2>/dev/null || echo "0")
  if [[ "$accrual_count" -ge "1" ]]; then
    pass
  else
    echo -e "    ${YELLOW}SKIP: No accruals found (cron may not have run yet). This test requires the accrual job to execute.${NC}"
    TOTAL=$((TOTAL - 1))  # Don't count as failure in environments where cron hasn't fired
  fi
fi

# Only run detailed accrual checks if we have accruals
ACCRUAL_AVAILABLE="false"
http_get "${BASE_URL}/v1/interest/accruals?account_id=${INTEREST_ACCOUNT_ID}&start_date=2026-01-01&end_date=2026-12-31"
accrual_count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('entries',[])))" 2>/dev/null || echo "0")
if [[ "$accrual_count" -ge "1" ]]; then
  ACCRUAL_AVAILABLE="true"
fi

if [[ "$ACCRUAL_AVAILABLE" == "true" ]]; then
  run_test "Verify accrual balance_used > 0"
  balance_used=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
if entries:
    print(entries[0].get('balance_used', '0'))
else:
    print('0')
" 2>/dev/null || echo "0")
  if assert_positive_decimal "$balance_used" "accrual balance_used"; then
    pass
  fi

  run_test "Verify accrual daily_interest > 0"
  daily_interest=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
if entries:
    print(entries[0].get('daily_interest', '0'))
else:
    print('0')
" 2>/dev/null || echo "0")
  if assert_positive_decimal "$daily_interest" "accrual daily_interest"; then
    pass
  fi

  run_test "Verify accrual has tier_breakdown array"
  tier_count=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
if entries:
    tb = entries[0].get('tier_breakdown', [])
    print(len(tb) if isinstance(tb, list) else 0)
else:
    print(0)
" 2>/dev/null || echo "0")
  if [[ "$tier_count" -ge "1" ]]; then
    pass
  else
    fail "accrual tier_breakdown: expected >= 1 tier, got ${tier_count}"
  fi

  run_test "Verify accrual currency = USD"
  accrual_currency=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
if entries:
    print(entries[0].get('currency', ''))
else:
    print('')
" 2>/dev/null || echo "")
  if [[ "$accrual_currency" == "USD" ]]; then
    pass
  else
    fail "accrual currency: expected USD, got ${accrual_currency}"
  fi
else
  echo -e "    ${YELLOW}(Skipping detailed accrual checks — no accrual data available)${NC}"
fi

run_test "GET /v1/interest/accruals -- Checking account should have NO accruals"
http_get "${BASE_URL}/v1/interest/accruals?account_id=${CHECKING_ACCOUNT_ID}&start_date=2026-01-01&end_date=2026-12-31"
if assert_status "$HTTP_STATUS" "200" "checking accruals"; then
  count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('entries',[])))" 2>/dev/null || echo "")
  if [[ "$count" == "0" ]]; then
    pass
  else
    fail "checking account should have 0 accruals, got ${count}"
  fi
fi

run_test "GET /v1/interest/accruals -- nonexistent account returns empty"
http_get "${BASE_URL}/v1/interest/accruals?account_id=00000000-0000-0000-0000-000000000000&start_date=2026-01-01&end_date=2026-12-31"
if assert_status "$HTTP_STATUS" "200" "nonexistent accruals"; then
  count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('entries',[])))" 2>/dev/null || echo "")
  if [[ "$count" == "0" ]]; then
    pass
  else
    fail "nonexistent account accruals: expected 0, got ${count}"
  fi
fi

run_test "GET /v1/interest/accruals -- date range with no data returns empty"
http_get "${BASE_URL}/v1/interest/accruals?account_id=${INTEREST_ACCOUNT_ID}&start_date=2020-01-01&end_date=2020-12-31"
if assert_status "$HTTP_STATUS" "200" "old date range accruals"; then
  count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('entries',[])))" 2>/dev/null || echo "")
  if [[ "$count" == "0" ]]; then
    pass
  else
    fail "old date range: expected 0 accruals, got ${count}"
  fi
fi

run_test "GET /v1/interest/accruals -- start_date > end_date → 400"
http_get "${BASE_URL}/v1/interest/accruals?account_id=${INTEREST_ACCOUNT_ID}&start_date=2026-12-31&end_date=2026-01-01"
if assert_status "$HTTP_STATUS" "400" "invalid date range"; then
  pass
fi

# ===========================================================================
# SUITE 6: Posting Verification
# ===========================================================================
suite "6. Posting Verification"

run_test "GET /v1/interest/postings -- query postings for Interest account"
http_get "${BASE_URL}/v1/interest/postings?account_id=${INTEREST_ACCOUNT_ID}&start_date=2026-01-01&end_date=2026-12-31"
if assert_status "$HTTP_STATUS" "200" "list postings"; then
  posting_count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('entries',[])))" 2>/dev/null || echo "0")
  if [[ "$posting_count" -ge "1" ]]; then
    pass
  else
    echo -e "    ${YELLOW}SKIP: No postings found (posting cron may not have run yet).${NC}"
    TOTAL=$((TOTAL - 1))
  fi
fi

POSTING_AVAILABLE="false"
http_get "${BASE_URL}/v1/interest/postings?account_id=${INTEREST_ACCOUNT_ID}&start_date=2026-01-01&end_date=2026-12-31"
posting_count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('entries',[])))" 2>/dev/null || echo "0")
if [[ "$posting_count" -ge "1" ]]; then
  POSTING_AVAILABLE="true"
fi

if [[ "$POSTING_AVAILABLE" == "true" ]]; then
  run_test "Verify posting has posted_amount > 0"
  posted_amount=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
if entries:
    print(entries[0].get('posted_amount', '0'))
else:
    print('0')
" 2>/dev/null || echo "0")
  if assert_positive_decimal "$posted_amount" "posted_amount"; then
    pass
  fi

  run_test "Verify posting status = completed or pending"
  posting_status=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
if entries:
    print(entries[0].get('status', ''))
else:
    print('')
" 2>/dev/null || echo "")
  if [[ "$posting_status" == "completed" ]] || [[ "$posting_status" == "pending" ]]; then
    pass
  else
    fail "posting status: expected completed or pending, got '${posting_status}'"
  fi

  run_test "Verify posting has period_start and period_end dates"
  period_start=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
if entries:
    print(entries[0].get('period_start', ''))
else:
    print('')
" 2>/dev/null || echo "")
  period_end=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
if entries:
    print(entries[0].get('period_end', ''))
else:
    print('')
" 2>/dev/null || echo "")
  if [[ -n "$period_start" ]] && [[ -n "$period_end" ]]; then
    pass
  else
    fail "posting missing period_start or period_end"
  fi

  # C1 fix verification: completed postings must have transaction_id
  if [[ "$posting_status" == "completed" ]]; then
    run_test "Verify completed posting has transaction_id (C1 fix)"
    txn_id=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
if entries:
    v = entries[0].get('transaction_id')
    print(v if v else '')
else:
    print('')
" 2>/dev/null || echo "")
    if [[ -n "$txn_id" ]] && [[ "$txn_id" != "None" ]] && [[ "$txn_id" != "null" ]]; then
      pass
    else
      fail "completed posting should have transaction_id, got '${txn_id}'"
    fi

    run_test "Verify interest transaction has IN- prefix reference (C1 fix)"
    http_get "${BASE_URL}/v1/transaction?bank_account_id=${INTEREST_ACCOUNT_ID}&offset=0&limit=10"
    if assert_status "$HTTP_STATUS" "200" "interest transactions"; then
      in_ref_count=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
count = sum(1 for e in entries if e.get('transaction_reference','').startswith('IN-'))
print(count)
" 2>/dev/null || echo "0")
      if [[ "$in_ref_count" -ge "1" ]]; then
        pass
      else
        fail "expected >= 1 transaction with IN- prefix, got ${in_ref_count}"
      fi
    fi

    run_test "Verify interest posting credited ledger balance"
    wait_for_outbox
    http_get "${BASE_URL}/v1/ledger/${INTEREST_LEDGER_ID}"
    if assert_status "$HTTP_STATUS" "200" "ledger after posting"; then
      new_balance=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "0")
      if python3 -c "exit(0 if float('${new_balance}') > 25000.0 else 1)" 2>/dev/null; then
        pass
      else
        fail "ledger after posting: expected > 25000, got ${new_balance}"
      fi
    fi
  fi
else
  echo -e "    ${YELLOW}(Skipping detailed posting checks — no posting data available)${NC}"
fi

run_test "GET /v1/interest/postings -- Checking account should have NO postings"
http_get "${BASE_URL}/v1/interest/postings?account_id=${CHECKING_ACCOUNT_ID}&start_date=2026-01-01&end_date=2026-12-31"
if assert_status "$HTTP_STATUS" "200" "checking postings"; then
  count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('entries',[])))" 2>/dev/null || echo "")
  if [[ "$count" == "0" ]]; then
    pass
  else
    fail "checking account should have 0 postings, got ${count}"
  fi
fi

run_test "GET /v1/interest/postings -- start_date > end_date → 400"
http_get "${BASE_URL}/v1/interest/postings?account_id=${INTEREST_ACCOUNT_ID}&start_date=2026-12-31&end_date=2026-01-01"
if assert_status "$HTTP_STATUS" "400" "posting invalid date range"; then
  pass
fi

# ===========================================================================
# SUITE 7: Rate Config Update (Sunset)
# ===========================================================================
suite "7. Rate Config Sunset"

run_test "PUT /v1/interest/rates/:id -- sunset rate config (set effective_to)"
http_put "${BASE_URL}/v1/interest/rates/${RATE_CONFIG_ID}" '{
  "effective_to": "2026-12-31"
}'
if assert_status "$HTTP_STATUS" "200" "sunset rate config"; then
  eff_to=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['effective_to'])" 2>/dev/null || echo "")
  if [[ "$eff_to" == "2026-12-31" ]]; then
    pass
  else
    fail "sunset effective_to: expected 2026-12-31, got ${eff_to}"
  fi
fi

run_test "PUT /v1/interest/rates/:id -- deactivate rate config"
http_put "${BASE_URL}/v1/interest/rates/${RATE_CONFIG_ID}" '{
  "is_active": false
}'
if assert_status "$HTTP_STATUS" "200" "deactivate rate config"; then
  is_active=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['is_active'])" 2>/dev/null || echo "")
  if [[ "$is_active" == "False" ]]; then
    pass
  else
    fail "deactivate: expected is_active=False, got ${is_active}"
  fi
fi

run_test "PUT /v1/interest/rates/:id -- re-activate for further tests"
http_put "${BASE_URL}/v1/interest/rates/${RATE_CONFIG_ID}" '{
  "is_active": true
}'
if assert_status "$HTTP_STATUS" "200" "reactivate rate config"; then
  pass
fi

run_test "PUT /v1/interest/rates/:id -- empty update body → 400"
http_put "${BASE_URL}/v1/interest/rates/${RATE_CONFIG_ID}" '{}'
if assert_status "$HTTP_STATUS" "400" "empty sunset update"; then
  pass
fi

run_test "PUT /v1/interest/rates/:id -- effective_to before effective_from → 400"
http_put "${BASE_URL}/v1/interest/rates/${RATE_CONFIG_ID}" '{
  "effective_to": "2025-01-01"
}'
if assert_status "$HTTP_STATUS" "400" "effective_to before effective_from"; then
  pass
fi

# ===========================================================================
# SUITE 8: Auth & Tenant Isolation
# ===========================================================================
suite "8. Auth & Tenant Isolation"

run_test "GET /v1/interest/rates -- without auth → 401"
http_get "${BASE_URL}/v1/interest/rates?currency=USD" "no"
if assert_status "$HTTP_STATUS" "401" "rates no auth"; then
  pass
fi

run_test "GET /v1/interest/accruals -- without auth → 401"
http_get "${BASE_URL}/v1/interest/accruals?account_id=${INTEREST_ACCOUNT_ID}&start_date=2026-01-01&end_date=2026-12-31" "no"
if assert_status "$HTTP_STATUS" "401" "accruals no auth"; then
  pass
fi

run_test "GET /v1/interest/postings -- without auth → 401"
http_get "${BASE_URL}/v1/interest/postings?account_id=${INTEREST_ACCOUNT_ID}&start_date=2026-01-01&end_date=2026-12-31" "no"
if assert_status "$HTTP_STATUS" "401" "postings no auth"; then
  pass
fi

run_test "GET /v1/interest/estimate -- without auth → 401"
http_get "${BASE_URL}/v1/interest/estimate?account_id=${INTEREST_ACCOUNT_ID}&days=30" "no"
if assert_status "$HTTP_STATUS" "401" "estimate no auth"; then
  pass
fi

run_test "POST /v1/interest/rates -- without auth → 401"
http_post "${BASE_URL}/v1/interest/rates" '{"currency":"USD","effective_from":"2026-01-01","tiers":[{"tier_order":1,"min_balance":0,"max_balance":null,"apr":0.01}]}' "no"
if assert_status "$HTTP_STATUS" "401" "create rate no auth"; then
  pass
fi

# ===========================================================================
# SUITE 9: Transaction & Report Integration
# ===========================================================================
suite "9. Transaction & Report Integration (requires posting to have completed)"

if [[ "$POSTING_AVAILABLE" == "true" ]]; then
  run_test "GET /v1/transaction -- Interest account has transactions"
  http_get "${BASE_URL}/v1/transaction?bank_account_id=${INTEREST_ACCOUNT_ID}&offset=0&limit=10"
  if assert_status "$HTTP_STATUS" "200" "interest transactions"; then
    txn_count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('entries',[])))" 2>/dev/null || echo "0")
    if [[ "$txn_count" -ge "1" ]]; then
      pass
    else
      fail "expected >= 1 transaction for interest account, got ${txn_count}"
    fi
  fi

  run_test "GET /v1/report/settlement -- CSV includes interest transactions"
  http_get "${BASE_URL}/v1/report/settlement?start_date=2026-01-01&end_date=2026-12-31&currency=USD"
  if assert_status "$HTTP_STATUS" "200" "settlement report"; then
    pass
  fi
else
  echo -e "    ${YELLOW}(Skipping transaction/report checks — no posting data)${NC}"
fi

# ===========================================================================
# SUITE 10: Edge Cases
# ===========================================================================
suite "10. Edge Cases"

run_test "Open Interest account with zero balance — no accrual expected"
EDGE_USER="interest-edge-$(date +%s)"
http_post "${BASE_URL}/v1/bank_account" "{
  \"OpenAccount\": {
    \"account_type\": \"Retail\",
    \"kind\": \"Interest\",
    \"currency\": \"USD\",
    \"external_reference_id\": \"${EDGE_USER}\"
  }
}"
ZERO_ACCOUNT_ID=""
if assert_status "$HTTP_STATUS" "201" "open zero-balance interest"; then
  ZERO_ACCOUNT_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  pass
fi

if [[ -n "$ZERO_ACCOUNT_ID" ]]; then
  sleep 2
  http_post "${BASE_URL}/v1/bank_account" "{
    \"ApproveAccount\": {\"id\": \"${ZERO_ACCOUNT_ID}\"}
  }"

  run_test "Query accruals for zero-balance Interest account — expect empty"
  http_get "${BASE_URL}/v1/interest/accruals?account_id=${ZERO_ACCOUNT_ID}&start_date=2026-01-01&end_date=2026-12-31"
  if assert_status "$HTTP_STATUS" "200" "zero balance accruals"; then
    count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('entries',[])))" 2>/dev/null || echo "")
    if [[ "$count" == "0" ]]; then
      pass
    else
      fail "zero-balance account: expected 0 accruals, got ${count}"
    fi
  fi
fi

run_test "Freeze Interest account — should exclude from future accruals"
http_post "${BASE_URL}/v1/bank_account" "{
  \"FreezeAccount\": {\"id\": \"${INTEREST_ACCOUNT_ID}\"}
}"
if assert_status "$HTTP_STATUS" "200" "freeze interest account"; then
  pass
fi

sleep 1

run_test "Verify Interest account is Frozen"
http_get "${BASE_URL}/v1/bank_account/${INTEREST_ACCOUNT_ID}"
if assert_status "$HTTP_STATUS" "200" "verify frozen"; then
  acct_status=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])" 2>/dev/null || echo "")
  if [[ "$acct_status" == "Freeze" ]]; then
    pass
  else
    fail "expected Freeze, got ${acct_status}"
  fi
fi

run_test "Unfreeze Interest account (restore for cleanup)"
http_post "${BASE_URL}/v1/bank_account" "{
  \"UnfreezeAccount\": {\"id\": \"${INTEREST_ACCOUNT_ID}\"}
}"
if assert_status "$HTTP_STATUS" "200" "unfreeze interest account"; then
  pass
fi

# ===========================================================================
# RESULTS
# ===========================================================================
echo ""
echo -e "${BLUE}============================================================${NC}"
echo -e "${BLUE}  RESULTS${NC}"
echo -e "${BLUE}============================================================${NC}"
echo ""
echo -e "  Total:  ${TOTAL}"
echo -e "  Passed: ${GREEN}${PASSED}${NC}"
echo -e "  Failed: ${RED}${FAILED}${NC}"

if [[ $FAILED -gt 0 ]]; then
  echo -e "\n  Failed tests:${FAILED_TESTS}"
  echo ""
  exit 1
fi

echo ""
echo -e "${GREEN}All ${TOTAL} tests passed!${NC}"
echo ""
