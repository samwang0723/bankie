-- Portal schema for Developer Portal
-- Provides organization management, API key management, webhook delivery, and audit logging

CREATE SCHEMA IF NOT EXISTS portal;

-- Sequence for auto-assigning tenant IDs to new organizations
CREATE SEQUENCE portal.tenant_id_seq START 100;

-- Organizations: top-level entity for multi-tenant portal access
CREATE TABLE portal.organizations (
    id UUID PRIMARY KEY,
    tenant_id INT UNIQUE NOT NULL DEFAULT nextval('portal.tenant_id_seq'),
    name VARCHAR NOT NULL,
    slug VARCHAR NOT NULL UNIQUE,
    status VARCHAR NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_organizations_tenant_id ON portal.organizations (tenant_id);
CREATE INDEX idx_organizations_status ON portal.organizations (status);

-- Organization members: users within an organization
CREATE TABLE portal.org_members (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES portal.organizations(id),
    email VARCHAR NOT NULL UNIQUE,
    password_hash VARCHAR NOT NULL,
    role VARCHAR NOT NULL DEFAULT 'member',
    status VARCHAR NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_org_members_org_id ON portal.org_members (org_id);
CREATE INDEX idx_org_members_email ON portal.org_members (email);

-- API keys: credentials issued to organizations for accessing the banking API
CREATE TABLE portal.api_keys (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES portal.organizations(id),
    tenant_id INT NOT NULL,
    name VARCHAR NOT NULL,
    key_prefix VARCHAR NOT NULL,
    key_hash VARCHAR NOT NULL UNIQUE,
    scopes JSONB NOT NULL DEFAULT '[]'::jsonb,
    environment VARCHAR NOT NULL DEFAULT 'live',
    status VARCHAR NOT NULL DEFAULT 'active',
    grace_expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_api_keys_org_id ON portal.api_keys (org_id);
CREATE INDEX idx_api_keys_key_hash ON portal.api_keys (key_hash);
CREATE INDEX idx_api_keys_status ON portal.api_keys (status);
CREATE INDEX idx_api_keys_created_at ON portal.api_keys (created_at);

-- Webhook endpoints: URLs registered by organizations for event delivery
CREATE TABLE portal.webhook_endpoints (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES portal.organizations(id),
    url VARCHAR NOT NULL,
    signing_secret VARCHAR NOT NULL,
    event_types JSONB NOT NULL DEFAULT '[]'::jsonb,
    status VARCHAR NOT NULL DEFAULT 'active',
    failure_count INT NOT NULL DEFAULT 0,
    disabled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_webhook_endpoints_org_id ON portal.webhook_endpoints (org_id);

-- Webhook deliveries: individual delivery attempts for webhook events
CREATE TABLE portal.webhook_deliveries (
    id UUID PRIMARY KEY,
    endpoint_id UUID NOT NULL REFERENCES portal.webhook_endpoints(id),
    event_type VARCHAR NOT NULL,
    payload JSONB NOT NULL,
    http_status INT,
    attempt_number INT NOT NULL DEFAULT 1,
    status VARCHAR NOT NULL DEFAULT 'pending',
    response_body TEXT,
    latency_ms INT,
    next_retry_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_webhook_deliveries_endpoint_id ON portal.webhook_deliveries (endpoint_id);
CREATE INDEX idx_webhook_deliveries_status ON portal.webhook_deliveries (status);
CREATE INDEX idx_webhook_deliveries_created_at ON portal.webhook_deliveries (created_at);

-- API logs: high-volume request logging with monthly range partitioning
CREATE TABLE portal.api_logs (
    id BIGINT NOT NULL,
    api_key_id UUID,
    tenant_id INT,
    method VARCHAR NOT NULL,
    path VARCHAR NOT NULL,
    status_code INT NOT NULL,
    latency_ms INT,
    client_ip VARCHAR,
    request_summary JSONB,
    response_summary JSONB,
    created_at TIMESTAMPTZ NOT NULL
) PARTITION BY RANGE (created_at);

CREATE INDEX idx_api_logs_tenant_id ON portal.api_logs (tenant_id);
CREATE INDEX idx_api_logs_api_key_id ON portal.api_logs (api_key_id);
CREATE INDEX idx_api_logs_created_at ON portal.api_logs (created_at);

-- Create partitions: current month + next 2 months
CREATE TABLE portal.api_logs_2026_02 PARTITION OF portal.api_logs
    FOR VALUES FROM ('2026-02-01') TO ('2026-03-01');

CREATE TABLE portal.api_logs_2026_03 PARTITION OF portal.api_logs
    FOR VALUES FROM ('2026-03-01') TO ('2026-04-01');

CREATE TABLE portal.api_logs_2026_04 PARTITION OF portal.api_logs
    FOR VALUES FROM ('2026-04-01') TO ('2026-05-01');

-- Audit logs: track all portal administrative actions
CREATE TABLE portal.audit_logs (
    id BIGINT PRIMARY KEY,
    org_id UUID,
    actor_id UUID,
    action VARCHAR NOT NULL,
    resource_type VARCHAR NOT NULL,
    resource_id VARCHAR,
    changes JSONB,
    client_ip VARCHAR,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_audit_logs_org_id ON portal.audit_logs (org_id);
CREATE INDEX idx_audit_logs_created_at ON portal.audit_logs (created_at);
