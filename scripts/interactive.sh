#!/usr/bin/env bash
#
# Bankie Interactive Console
# Menu-driven tool for testing all Bankie API endpoints.
#
# Auto-detects JWT from Docker container, .docker-jwt-token, or .local-jwt-token.
# Remembers last-used IDs across menu actions for convenience.
#
# Usage:
#   ./scripts/interactive.sh                  # auto-detect token
#   ./scripts/interactive.sh <JWT_TOKEN>      # explicit token
#   TOKEN=eyJ... ./scripts/interactive.sh     # via env var
#
set -uo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
BASE_URL="${BASE_URL:-http://localhost:3030}"
OUTBOX_WAIT="${OUTBOX_WAIT:-15}"

# ---------------------------------------------------------------------------
# Colors
# ---------------------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
DIM='\033[2m'
NC='\033[0m'

# ---------------------------------------------------------------------------
# State — remembers last-used values
# ---------------------------------------------------------------------------
LAST_ACCOUNT_ID=""
LAST_LEDGER_ID=""
LAST_USER_ID=""
LAST_ACCOUNT_NUMBER=""
LAST_CURRENCY="USD"

# ---------------------------------------------------------------------------
# JWT Token Resolution
# ---------------------------------------------------------------------------
resolve_token() {
  local token="${1:-${TOKEN:-}}"

  # 1. Explicit argument or env var
  if [[ -n "$token" ]]; then
    echo "$token"
    return
  fi

  # 2. Read from existing token files first (preserves tenant context)
  for f in .docker-jwt-token .local-jwt-token; do
    if [[ -f "$f" && -s "$f" ]]; then
      echo -e "${DIM}  Using token from ${f}${NC}" >&2
      cat "$f"
      return
    fi
  done

  # 3. Generate from Docker container as last resort
  if docker ps --format '{{.Names}}' 2>/dev/null | grep -q '^bankie$'; then
    echo -e "${DIM}  Generating JWT from Docker container...${NC}" >&2
    token=$(docker exec bankie /app/bankie --mode jwt --service demo-service 2>&1 | \
      grep -oE 'eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+' | head -1)
    if [[ -n "$token" ]]; then
      echo "$token" > .docker-jwt-token
      echo -e "${DIM}  JWT saved to .docker-jwt-token${NC}" >&2
      echo "$token"
      return
    fi
  fi

  echo ""
}

TOKEN_VALUE=$(resolve_token "${1:-}")
if [[ -z "$TOKEN_VALUE" ]]; then
  echo -e "${RED}ERROR: No JWT token found.${NC}"
  echo ""
  echo "Options:"
  echo "  1. Start Docker stack:  make docker-up"
  echo "  2. Generate token:      make docker-jwt"
  echo "  3. Pass explicitly:     ./scripts/interactive.sh <JWT_TOKEN>"
  exit 1
fi

AUTH="Authorization: Bearer ${TOKEN_VALUE}"
CT="Content-Type: application/json"

# ---------------------------------------------------------------------------
# HTTP Helpers
# ---------------------------------------------------------------------------
HTTP_BODY=""
HTTP_STATUS=""

http_get() {
  local url="$1"
  local response
  response=$(curl -s -w "\n%{http_code}" "$url" -H "$AUTH")
  HTTP_STATUS=$(echo "$response" | tail -1)
  HTTP_BODY=$(echo "$response" | sed '$d')
}

http_post() {
  local url="$1"
  local body="$2"
  local response
  response=$(curl -s -w "\n%{http_code}" -X POST "$url" -H "$AUTH" -H "$CT" -d "$body")
  HTTP_STATUS=$(echo "$response" | tail -1)
  HTTP_BODY=$(echo "$response" | sed '$d')
}

http_get_raw() {
  local url="$1"
  curl -s -D /dev/stderr "$url" -H "$AUTH" 2>/tmp/bankie_headers
}

pretty_json() {
  echo "$1" | python3 -m json.tool 2>/dev/null || echo "$1"
}

print_status() {
  if [[ "$HTTP_STATUS" =~ ^2 ]]; then
    echo -e "  ${GREEN}HTTP ${HTTP_STATUS}${NC}"
  else
    echo -e "  ${RED}HTTP ${HTTP_STATUS}${NC}"
  fi
}

# ---------------------------------------------------------------------------
# Prompt helpers — use last value as default
# ---------------------------------------------------------------------------
prompt() {
  local label="$1"
  local default="$2"
  local result
  if [[ -n "$default" ]]; then
    read -rp "  ${label} [${default}]: " result
    echo "${result:-$default}"
  else
    read -rp "  ${label}: " result
    echo "$result"
  fi
}

separator() {
  echo -e "${DIM}──────────────────────────────────────────────────────${NC}"
}

# ---------------------------------------------------------------------------
# Menu Actions
# ---------------------------------------------------------------------------

action_health() {
  echo -e "\n${CYAN}Health Check${NC}"
  separator

  echo -e "  ${DIM}/health${NC}"
  local r
  r=$(curl -s -w "\n%{http_code}" "${BASE_URL}/health")
  local status=$(echo "$r" | tail -1)
  local body=$(echo "$r" | sed '$d')
  if [[ "$status" == "200" ]]; then
    echo -e "  ${GREEN}Healthy (${status})${NC}"
  else
    echo -e "  ${RED}Unhealthy (${status})${NC}  ${body}"
  fi

  echo -e "  ${DIM}/ready${NC}"
  r=$(curl -s -w "\n%{http_code}" "${BASE_URL}/ready")
  status=$(echo "$r" | tail -1)
  body=$(echo "$r" | sed '$d')
  if [[ "$status" == "200" ]]; then
    echo -e "  ${GREEN}Ready (${status})${NC}  $(pretty_json "$body")"
  else
    echo -e "  ${RED}Not ready (${status})${NC}  ${body}"
  fi
}

action_list_house_accounts() {
  echo -e "\n${CYAN}List House Accounts${NC}"
  separator
  local currency
  currency=$(prompt "Currency (USD/TWD/BTC/ETH/USDT or blank for all)" "")
  [[ -n "$currency" ]] && LAST_CURRENCY="$currency"

  local url="${BASE_URL}/v1/house_account"
  [[ -n "$currency" ]] && url="${url}?currency=${currency}"

  http_get "$url"
  print_status
  pretty_json "$HTTP_BODY"
}

action_create_house_account() {
  echo -e "\n${CYAN}Create House Account${NC}"
  separator
  local currency name acct_type
  currency=$(prompt "Currency" "$LAST_CURRENCY")
  name=$(prompt "Account name" "Master ${currency} Settlement")
  acct_type=$(prompt "Account type (Settlement/House)" "Settlement")

  local body
  body=$(cat <<ENDJSON
{
  "status": "active",
  "account_name": "${name}",
  "account_type": "${acct_type}",
  "currency": "${currency}"
}
ENDJSON
  )

  http_post "${BASE_URL}/v1/house_account" "$body"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_CURRENCY="$currency"
}

action_open_account() {
  echo -e "\n${CYAN}Open Bank Account${NC}"
  separator
  local acct_type kind currency ext_ref
  acct_type=$(prompt "Account type (Retail/Institution/Tax)" "Retail")
  kind=$(prompt "Kind (Checking/Interest/Yield)" "Checking")
  currency=$(prompt "Currency" "$LAST_CURRENCY")
  ext_ref=$(prompt "External reference ID (user ID, optional)" "$LAST_USER_ID")

  local body
  body=$(cat <<ENDJSON
{
  "OpenAccount": {
    "account_type": "${acct_type}",
    "kind": "${kind}",
    "currency": "${currency}"$([ -n "$ext_ref" ] && echo ",
    \"external_reference_id\": \"${ext_ref}\"")
  }
}
ENDJSON
  )

  http_post "${BASE_URL}/v1/bank_account" "$body"
  print_status
  pretty_json "$HTTP_BODY"

  # Extract and remember IDs
  local new_id
  new_id=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('id',''))" 2>/dev/null)
  if [[ -n "$new_id" ]]; then
    LAST_ACCOUNT_ID="$new_id"
    echo -e "\n  ${GREEN}Account ID saved: ${new_id}${NC}"
  fi
  [[ -n "$ext_ref" ]] && LAST_USER_ID="$ext_ref"
  LAST_CURRENCY="$currency"
}

action_approve_account() {
  echo -e "\n${CYAN}Approve Account (KYC)${NC}"
  separator
  local id
  id=$(prompt "Account ID" "$LAST_ACCOUNT_ID")

  local body="{\"ApproveAccount\":{\"id\":\"${id}\"}}"

  http_post "${BASE_URL}/v1/bank_account" "$body"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"
}

action_deposit() {
  echo -e "\n${CYAN}Deposit${NC}"
  separator
  local id amount currency
  id=$(prompt "Account ID" "$LAST_ACCOUNT_ID")
  amount=$(prompt "Amount" "1000")
  currency=$(prompt "Currency" "$LAST_CURRENCY")

  local body
  body=$(cat <<ENDJSON
{
  "Deposit": {
    "id": "${id}",
    "amount": {
      "amount": "${amount}",
      "currency": "${currency}"
    }
  }
}
ENDJSON
  )

  http_post "${BASE_URL}/v1/bank_account" "$body"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"
  LAST_CURRENCY="$currency"

  echo -e "\n  ${YELLOW}Note: Ledger update is async via outbox (${OUTBOX_WAIT}s).${NC}"
}

action_withdraw() {
  echo -e "\n${CYAN}Withdrawal${NC}"
  separator
  local id amount currency
  id=$(prompt "Account ID" "$LAST_ACCOUNT_ID")
  amount=$(prompt "Amount" "100")
  currency=$(prompt "Currency" "$LAST_CURRENCY")

  local body
  body=$(cat <<ENDJSON
{
  "Withdrawal": {
    "id": "${id}",
    "amount": {
      "amount": "${amount}",
      "currency": "${currency}"
    }
  }
}
ENDJSON
  )

  http_post "${BASE_URL}/v1/bank_account" "$body"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"
  LAST_CURRENCY="$currency"

  echo -e "\n  ${YELLOW}Note: Ledger update is async via outbox (${OUTBOX_WAIT}s).${NC}"
}

action_transfer() {
  echo -e "\n${CYAN}Transfer${NC}"
  separator
  local from_id to_id amount currency
  from_id=$(prompt "From account ID" "$LAST_ACCOUNT_ID")
  to_id=$(prompt "To account ID" "")
  amount=$(prompt "Amount" "100")
  currency=$(prompt "Currency" "$LAST_CURRENCY")

  if [[ -z "$to_id" ]]; then
    echo -e "  ${RED}Destination account ID is required.${NC}"
    return
  fi

  local body
  body=$(cat <<ENDJSON
{
  "Transfer": {
    "id": "${from_id}",
    "to_account_id": "${to_id}",
    "amount": {
      "amount": "${amount}",
      "currency": "${currency}"
    }
  }
}
ENDJSON
  )

  http_post "${BASE_URL}/v1/bank_account" "$body"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$from_id"
  LAST_CURRENCY="$currency"

  echo -e "\n  ${YELLOW}Note: Ledger update is async via outbox (${OUTBOX_WAIT}s).${NC}"
}

action_freeze_unfreeze() {
  echo -e "\n${CYAN}Freeze / Unfreeze Account${NC}"
  separator
  local id choice
  id=$(prompt "Account ID" "$LAST_ACCOUNT_ID")

  echo "  1) Freeze"
  echo "  2) Unfreeze"
  read -rp "  Choice [1]: " choice
  choice="${choice:-1}"

  local cmd
  if [[ "$choice" == "2" ]]; then
    cmd="UnfreezeAccount"
  else
    cmd="FreezeAccount"
  fi

  local body="{\"${cmd}\":{\"id\":\"${id}\"}}"
  http_post "${BASE_URL}/v1/bank_account" "$body"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"
}

action_close_account() {
  echo -e "\n${CYAN}Close Account${NC}"
  separator
  local id
  id=$(prompt "Account ID" "$LAST_ACCOUNT_ID")

  local body="{\"CloseAccount\":{\"id\":\"${id}\"}}"
  http_post "${BASE_URL}/v1/bank_account" "$body"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"
}

action_view_account() {
  echo -e "\n${CYAN}View Account Details${NC}"
  separator
  local id
  id=$(prompt "Account ID" "$LAST_ACCOUNT_ID")

  http_get "${BASE_URL}/v1/bank_account/${id}"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"

  # Extract ledger_id for convenience
  local lid
  lid=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('ledger_id',''))" 2>/dev/null)
  [[ -n "$lid" ]] && LAST_LEDGER_ID="$lid"
}

action_view_ledger() {
  echo -e "\n${CYAN}View Ledger Balance${NC}"
  separator
  local id
  id=$(prompt "Ledger ID" "$LAST_LEDGER_ID")

  if [[ -z "$id" ]]; then
    echo -e "  ${YELLOW}Tip: View an account first (option 3) to auto-fill the ledger ID.${NC}"
    return
  fi

  http_get "${BASE_URL}/v1/ledger/${id}"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_LEDGER_ID="$id"
}

action_list_user_accounts() {
  echo -e "\n${CYAN}List User Accounts${NC}"
  separator
  local user_id
  user_id=$(prompt "User ID (external_reference_id)" "$LAST_USER_ID")

  if [[ -z "$user_id" ]]; then
    echo -e "  ${RED}User ID is required.${NC}"
    return
  fi

  http_get "${BASE_URL}/v1/user/${user_id}"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_USER_ID="$user_id"

  # Extract first account ID if available
  local first_id
  first_id=$(echo "$HTTP_BODY" | python3 -c "
import sys,json
data=json.load(sys.stdin)
entries=data.get('entries',[])
if entries: print(entries[0].get('id',''))
else: print('')
" 2>/dev/null)
  [[ -n "$first_id" ]] && LAST_ACCOUNT_ID="$first_id"
}

action_lookup_by_number() {
  echo -e "\n${CYAN}Lookup Account by Number${NC}"
  separator
  local num
  num=$(prompt "Account number" "$LAST_ACCOUNT_NUMBER")

  if [[ -z "$num" ]]; then
    echo -e "  ${RED}Account number is required.${NC}"
    return
  fi

  http_get "${BASE_URL}/v1/bank_account/by-number/${num}"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_NUMBER="$num"

  local aid lid
  aid=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('id',''))" 2>/dev/null)
  lid=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('ledger_id',''))" 2>/dev/null)
  [[ -n "$aid" ]] && LAST_ACCOUNT_ID="$aid"
  [[ -n "$lid" ]] && LAST_LEDGER_ID="$lid"
}

action_sub_accounts() {
  echo -e "\n${CYAN}List Sub-Accounts${NC}"
  separator
  local id
  id=$(prompt "Master account ID" "$LAST_ACCOUNT_ID")

  http_get "${BASE_URL}/v1/bank_account/${id}/sub-accounts"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"
}

action_transactions() {
  echo -e "\n${CYAN}List Transactions${NC}"
  separator
  local id offset limit start_date end_date tx_type status
  id=$(prompt "Bank account ID" "$LAST_ACCOUNT_ID")
  offset=$(prompt "Offset" "0")
  limit=$(prompt "Limit" "20")
  start_date=$(prompt "Start date (YYYY-MM-DD, blank to skip)" "")
  end_date=$(prompt "End date (YYYY-MM-DD, blank to skip)" "")
  tx_type=$(prompt "Type filter (deposit/withdrawal/transfer, blank to skip)" "")
  status=$(prompt "Status filter (blank to skip)" "")

  local url="${BASE_URL}/v1/transaction?bank_account_id=${id}&offset=${offset}&limit=${limit}"
  [[ -n "$start_date" ]] && url="${url}&start_date=${start_date}"
  [[ -n "$end_date" ]] && url="${url}&end_date=${end_date}"
  [[ -n "$tx_type" ]] && url="${url}&transaction_type=${tx_type}"
  [[ -n "$status" ]] && url="${url}&status=${status}"

  http_get "$url"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"
}

action_balance_history() {
  echo -e "\n${CYAN}Balance History${NC}"
  separator
  local id start_date end_date
  id=$(prompt "Bank account ID" "$LAST_ACCOUNT_ID")
  start_date=$(prompt "Start date (YYYY-MM-DD)" "2026-01-01")
  end_date=$(prompt "End date (YYYY-MM-DD)" "2026-12-31")

  http_get "${BASE_URL}/v1/bank_account/${id}/balance-history?start_date=${start_date}&end_date=${end_date}"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"

  echo -e "\n  ${YELLOW}Note: Snapshots are created by the midnight UTC cron job.${NC}"
}

action_settlement_report() {
  echo -e "\n${CYAN}Settlement Report (CSV)${NC}"
  separator
  local id start_date end_date currency save
  id=$(prompt "Bank account ID (blank for all accounts)" "$LAST_ACCOUNT_ID")
  start_date=$(prompt "Start date (YYYY-MM-DD)" "2026-01-01")
  end_date=$(prompt "End date (YYYY-MM-DD)" "2026-12-31")
  currency=$(prompt "Currency (blank for auto)" "$LAST_CURRENCY")

  local url="${BASE_URL}/v1/report/settlement?start_date=${start_date}&end_date=${end_date}"
  [[ -n "$id" ]] && url="${url}&bank_account_id=${id}"
  [[ -n "$currency" ]] && url="${url}&currency=${currency}"

  read -rp "  Save to file? (y/N): " save
  if [[ "$save" == "y" || "$save" == "Y" ]]; then
    local filename
    if [[ -n "$id" ]]; then
      filename="settlement_${id:0:8}_${start_date}_${end_date}.csv"
    else
      filename="settlement_all_${start_date}_${end_date}.csv"
    fi
    curl -s "$url" -H "$AUTH" -o "$filename"
    echo -e "  ${GREEN}Saved to ${filename}${NC}"
    echo ""
    head -5 "$filename"
    local lines
    lines=$(wc -l < "$filename")
    echo -e "  ${DIM}... (${lines} total lines)${NC}"
  else
    local response
    response=$(curl -s -w "\n%{http_code}" "$url" -H "$AUTH")
    HTTP_STATUS=$(echo "$response" | tail -1)
    HTTP_BODY=$(echo "$response" | sed '$d')
    print_status
    echo "$HTTP_BODY"
  fi

  LAST_ACCOUNT_ID="$id"
  LAST_CURRENCY="$currency"
}

action_quick_flow() {
  echo -e "\n${CYAN}Quick Flow: Open + Approve + Deposit${NC}"
  separator
  echo -e "  ${DIM}This runs a complete account setup in one go.${NC}"
  echo ""
  local currency ext_ref amount
  currency=$(prompt "Currency" "$LAST_CURRENCY")
  ext_ref=$(prompt "External reference ID (user ID)" "$LAST_USER_ID")
  amount=$(prompt "Initial deposit amount" "1000")
  LAST_CURRENCY="$currency"

  # 1. Open account
  echo -e "\n  ${BLUE}[1/3] Opening account...${NC}"
  local open_body
  open_body=$(cat <<ENDJSON
{
  "OpenAccount": {
    "account_type": "Retail",
    "kind": "Checking",
    "currency": "${currency}"$([ -n "$ext_ref" ] && echo ",
    \"external_reference_id\": \"${ext_ref}\"")
  }
}
ENDJSON
  )
  http_post "${BASE_URL}/v1/bank_account" "$open_body"
  print_status

  local acct_id acct_num
  acct_id=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('id',''))" 2>/dev/null)
  acct_num=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('account_number',''))" 2>/dev/null)

  if [[ -z "$acct_id" ]]; then
    echo -e "  ${RED}Failed to open account.${NC}"
    pretty_json "$HTTP_BODY"
    return
  fi
  echo -e "  ${GREEN}Account: ${acct_id}${NC}"
  echo -e "  ${GREEN}Number:  ${acct_num}${NC}"
  LAST_ACCOUNT_ID="$acct_id"
  LAST_ACCOUNT_NUMBER="$acct_num"
  [[ -n "$ext_ref" ]] && LAST_USER_ID="$ext_ref"

  # 2. Approve
  echo -e "\n  ${BLUE}[2/3] Approving (KYC)...${NC}"
  http_post "${BASE_URL}/v1/bank_account" "{\"ApproveAccount\":{\"id\":\"${acct_id}\"}}"
  print_status

  if [[ ! "$HTTP_STATUS" =~ ^2 ]]; then
    echo -e "  ${RED}Approval failed.${NC}"
    pretty_json "$HTTP_BODY"
    return
  fi

  # Extract ledger_id
  local lid
  lid=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('ledger_id',''))" 2>/dev/null)
  [[ -n "$lid" ]] && LAST_LEDGER_ID="$lid"

  # 3. Deposit
  echo -e "\n  ${BLUE}[3/3] Depositing ${amount} ${currency}...${NC}"
  local dep_body
  dep_body=$(cat <<ENDJSON
{
  "Deposit": {
    "id": "${acct_id}",
    "amount": {
      "amount": "${amount}",
      "currency": "${currency}"
    }
  }
}
ENDJSON
  )
  http_post "${BASE_URL}/v1/bank_account" "$dep_body"
  print_status

  echo -e "\n  ${GREEN}Done! Account is ready.${NC}"
  echo -e "  Account ID:  ${BOLD}${acct_id}${NC}"
  echo -e "  Ledger ID:   ${BOLD}${LAST_LEDGER_ID}${NC}"
  echo -e "  Account #:   ${BOLD}${acct_num}${NC}"
  echo -e "\n  ${YELLOW}Ledger balance updates in ~${OUTBOX_WAIT}s (outbox processing).${NC}"
}

action_list_accounts() {
  echo -e "\n${CYAN}List All Accounts${NC}"
  separator

  http_get "${BASE_URL}/v1/accounts?offset=0&limit=50"
  print_status

  if [[ ! "$HTTP_STATUS" =~ ^2 ]]; then
    pretty_json "$HTTP_BODY"
    return
  fi

  # Parse and display as a numbered table
  local count
  count=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
print(len(entries))
" 2>/dev/null)

  if [[ "$count" == "0" || -z "$count" ]]; then
    echo -e "  ${YELLOW}No accounts found. Use option 1 or 18 to create one.${NC}"
    return
  fi

  echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
pagination = data.get('pagination', {})
total = pagination.get('total', len(entries))

print(f'  {\"#\":>3}  {\"Account ID\":36}  {\"Account Number\":14}  {\"Status\":10}  {\"Currency\":8}  {\"Kind\":10}  {\"Available\":>15}')
print(f'  {\"---\":>3}  {\"-\"*36}  {\"-\"*14}  {\"-\"*10}  {\"-\"*8}  {\"-\"*10}  {\"-\"*15}')

for i, e in enumerate(entries):
    aid = e.get('id', '?')
    num = e.get('account_number', '?')
    st = e.get('status', '?')
    cur = e.get('currency', '?')
    kind = e.get('kind', '?')
    avail = e.get('available', '0')
    if avail is None: avail = '-'
    print(f'  {i+1:3}  {aid:36}  {num:14}  {st:10}  {cur:8}  {kind:10}  {str(avail):>15}')

print(f'\n  Total: {total} accounts')
"

  echo ""
  read -rp "  Select account # to set as active (Enter to skip): " pick
  if [[ -n "$pick" && "$pick" =~ ^[0-9]+$ ]]; then
    local selected
    selected=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
idx = int('${pick}') - 1
if 0 <= idx < len(entries):
    e = entries[idx]
    print(e.get('id', ''))
    print(e.get('ledger_id', ''))
    print(e.get('account_number', ''))
    print(e.get('currency', ''))
else:
    print('')
" 2>/dev/null)

    local sel_id sel_lid sel_num sel_cur
    sel_id=$(echo "$selected" | sed -n '1p')
    sel_lid=$(echo "$selected" | sed -n '2p')
    sel_num=$(echo "$selected" | sed -n '3p')
    sel_cur=$(echo "$selected" | sed -n '4p')

    if [[ -n "$sel_id" ]]; then
      LAST_ACCOUNT_ID="$sel_id"
      [[ -n "$sel_lid" ]] && LAST_LEDGER_ID="$sel_lid"
      [[ -n "$sel_num" ]] && LAST_ACCOUNT_NUMBER="$sel_num"
      [[ -n "$sel_cur" ]] && LAST_CURRENCY="$sel_cur"
      echo -e "  ${GREEN}Active account set:${NC}"
      echo -e "    Account ID: ${BOLD}${LAST_ACCOUNT_ID}${NC}"
      [[ -n "$LAST_LEDGER_ID" ]] && echo -e "    Ledger ID:  ${BOLD}${LAST_LEDGER_ID}${NC}"
      [[ -n "$LAST_ACCOUNT_NUMBER" ]] && echo -e "    Account #:  ${BOLD}${LAST_ACCOUNT_NUMBER}${NC}"
    else
      echo -e "  ${RED}Invalid selection.${NC}"
    fi
  fi
}

# ---------------------------------------------------------------------------
# Main Menu
# ---------------------------------------------------------------------------

print_menu() {
  echo ""
  echo -e "${BOLD}╔══════════════════════════════════════════════════════╗${NC}"
  echo -e "${BOLD}║            ${BLUE}Bankie Interactive Console${NC}${BOLD}                ║${NC}"
  echo -e "${BOLD}╚══════════════════════════════════════════════════════╝${NC}"
  echo ""
  echo -e "  ${BOLD}Account Lifecycle${NC}"
  echo -e "    ${GREEN} 1${NC}) Open new account"
  echo -e "    ${GREEN} 2${NC}) Approve account (KYC)"
  echo -e "    ${GREEN} 3${NC}) View account details"
  echo -e "    ${GREEN} 4${NC}) Freeze / Unfreeze"
  echo -e "    ${GREEN} 5${NC}) Close account"
  echo ""
  echo -e "  ${BOLD}Money Operations${NC}"
  echo -e "    ${GREEN} 6${NC}) Deposit"
  echo -e "    ${GREEN} 7${NC}) Withdraw"
  echo -e "    ${GREEN} 8${NC}) Transfer"
  echo ""
  echo -e "  ${BOLD}Queries${NC}"
  echo -e "    ${GREEN} 9${NC}) View ledger balance"
  echo -e "    ${GREEN}10${NC}) List transactions"
  echo -e "    ${GREEN}11${NC}) List user accounts"
  echo -e "    ${GREEN}12${NC}) Lookup by account number"
  echo -e "    ${GREEN}13${NC}) List sub-accounts"
  echo ""
  echo -e "  ${BOLD}Reports${NC}"
  echo -e "    ${GREEN}14${NC}) Settlement report (CSV)"
  echo -e "    ${GREEN}15${NC}) Balance history"
  echo ""
  echo -e "  ${BOLD}House Accounts${NC}"
  echo -e "    ${GREEN}16${NC}) List house accounts"
  echo -e "    ${GREEN}17${NC}) Create house account"
  echo ""
  echo -e "  ${BOLD}Shortcuts${NC}"
  echo -e "    ${GREEN}18${NC}) Quick flow (open + approve + deposit)"
  echo -e "    ${GREEN}19${NC}) List all accounts"
  echo -e "    ${GREEN}20${NC}) Health check"
  echo ""

  # Show remembered state
  if [[ -n "$LAST_ACCOUNT_ID" || -n "$LAST_LEDGER_ID" || -n "$LAST_USER_ID" ]]; then
    echo -e "  ${DIM}Remembered:${NC}"
    [[ -n "$LAST_ACCOUNT_ID" ]] && echo -e "    ${DIM}Account:  ${LAST_ACCOUNT_ID}${NC}"
    [[ -n "$LAST_LEDGER_ID" ]] && echo -e "    ${DIM}Ledger:   ${LAST_LEDGER_ID}${NC}"
    [[ -n "$LAST_USER_ID" ]] && echo -e "    ${DIM}User:     ${LAST_USER_ID}${NC}"
    [[ -n "$LAST_ACCOUNT_NUMBER" ]] && echo -e "    ${DIM}Acct #:   ${LAST_ACCOUNT_NUMBER}${NC}"
    echo ""
  fi

  echo -e "    ${RED} 0${NC}) Exit"
  echo ""
}

# ---------------------------------------------------------------------------
# Main Loop
# ---------------------------------------------------------------------------
echo -e "\n${GREEN}Connected to ${BASE_URL}${NC}"
echo -e "${DIM}Token: ${TOKEN_VALUE:0:20}...${NC}"

while true; do
  print_menu
  read -rp "  Choose [0-20]: " choice

  case "$choice" in
    1)  action_open_account ;;
    2)  action_approve_account ;;
    3)  action_view_account ;;
    4)  action_freeze_unfreeze ;;
    5)  action_close_account ;;
    6)  action_deposit ;;
    7)  action_withdraw ;;
    8)  action_transfer ;;
    9)  action_view_ledger ;;
    10) action_transactions ;;
    11) action_list_user_accounts ;;
    12) action_lookup_by_number ;;
    13) action_sub_accounts ;;
    14) action_settlement_report ;;
    15) action_balance_history ;;
    16) action_list_house_accounts ;;
    17) action_create_house_account ;;
    18) action_quick_flow ;;
    19) action_list_accounts ;;
    20) action_health ;;
    0|q|Q|exit)
      echo -e "\n${GREEN}Bye!${NC}\n"
      exit 0
      ;;
    *)
      echo -e "\n  ${RED}Invalid choice.${NC}"
      ;;
  esac

  echo ""
  read -rp "  Press Enter to continue..."
done
