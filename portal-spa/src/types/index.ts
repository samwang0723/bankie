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
  email: string;
  password: string;
}

export interface AuthResponse {
  token: string;
  user: PortalUser;
  organization: Organization;
}

export interface PortalUser {
  id: string;
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

// Organization Members
export interface OrgMember {
  id: string;
  user_id: string;
  email: string;
  role: OrgRole;
  invited_at: string;
  joined_at: string | null;
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
  total_requests_today: number;
  org_name: string;
  environment: Environment;
}

// Bank Accounts (from bankie-core views)
export interface BankAccountView {
  view_id: string;
  account_number: string;
  kind: string;
  currency: string;
  status: string;
  external_reference_id: string;
  parent_id: string | null;
  created_at?: string;
}

// Transactions
export interface Transaction {
  id: string;
  bank_account_id: string;
  kind: string;
  amount: string;
  currency: string;
  status: string;
  reference_id: string;
  created_at: string;
}

// Ledger
export interface LedgerView {
  view_id: string;
  available: string;
  pending: string;
  current: string;
  currency: string;
}
