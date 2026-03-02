// API response envelope (matches bankie-common AppError format)
export interface ApiError {
  code: number;
  message: string;
}

// Auth
export interface LoginRequest {
  email: string;
  password: string;
}

export interface SignupRequest {
  org_name: string;
  name: string;
  email: string;
  password: string;
}

export interface AuthResponse {
  user: PortalUser;
  organization: Organization;
}

export interface PortalUser {
  id: string;
  name: string;
  email: string;
  role: OrgRole;
  created_at: string;
}

// Organization
export interface Organization {
  id: string;
  name: string;
  slug: string;
  environment: Environment;
  created_at: string;
}

export type Environment = "live" | "test";

export type OrgRole = "owner" | "admin" | "member";

// Organization Members (matches gateway OrgMember)
export interface OrgMember {
  id: string;
  org_id: string;
  name: string;
  email: string;
  role: OrgRole;
  status: MemberStatus;
  created_at: string;
  updated_at: string;
}

export type MemberStatus = "active" | "pending" | "suspended";

export interface InviteMemberRequest {
  email: string;
  role: string;
  name?: string;
}

export interface InviteMemberResponse {
  member: OrgMember;
  invite_link: string;
}

export interface AcceptInviteRequest {
  token: string;
  password: string;
  name?: string;
}

export interface InviteInfo {
  email: string;
  org_name: string;
  role: OrgRole;
}

export interface UpdateRoleRequest {
  role: string;
}

// API Keys (matches gateway KeyListItem)
export interface ApiKey {
  id: string;
  name: string;
  key_prefix: string;
  scopes: string[];
  status: ApiKeyStatus;
  grace_expires_at: string | null;
  created_at: string;
}

export type ApiKeyStatus = "active" | "rotated" | "revoked";

export interface CreateApiKeyRequest {
  name: string;
  scopes: string[];
}

// Matches gateway CreateKeyResponse (flat, not nested)
export interface CreateApiKeyResponse {
  id: string;
  name: string;
  key_prefix: string;
  raw_key: string;
  scopes: string[];
  created_at: string;
}

// Matches gateway RotateKeyResponse
export interface RotateApiKeyResponse {
  new_key: CreateApiKeyResponse;
  old_key_id: string;
  grace_expires_at: string;
}

// Dashboard
export interface DashboardStats {
  total_api_keys: number;
  active_api_keys: number;
  scopes_granted: number;
  total_requests_today: number;
  throttled_today: number;
  org_name: string;
  environment: Environment;
}

// Activity
export interface ActivityEntry {
  id: number;
  org_id: string;
  actor_id: string;
  action: string;
  resource_type: string;
  resource_id: string | null;
  changes: Record<string, unknown> | null;
  created_at: string;
}

// Bank Accounts (matches BankAccountWithLedger from Core /v1/accounts)
export interface BankAccountView {
  id: string;
  account_number: string;
  kind: string;
  currency: string;
  status: string;
  account_type?: string;
  external_reference_id: string | null;
  parent_id: string | null;
  ledger_id?: string;
  available?: string;
  pending?: string;
  book_balance?: string;
  created_at?: string;
  updated_at?: string;
}

// Transactions (matches TransactionWithMoney from Core)
export interface Transaction {
  id: string;
  bank_account_id: string;
  transaction_type: string;
  transaction_reference: string;
  transaction_date: string;
  amount: string;
  currency: string;
  description: string | null;
  metadata: Record<string, unknown>;
  status: string;
  amount_usd: string | null;
  fx_rate_to_usd: string | null;
  fx_rate_source: string | null;
}

// Rate Limits (matches gateway /dashboard/rate-limits response)
export interface RateLimitEntry {
  key_id: string;
  key_name: string;
  key_prefix: string;
  status: ApiKeyStatus;
  remaining: number;
  limit: number;
  sustained_per_min: number;
  throttled_24h: number;
}

// Ledger
export interface LedgerView {
  view_id: string;
  available: string;
  pending: string;
  current: string;
  currency: string;
}
