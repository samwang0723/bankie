-- Add auto-increment sequences for api_logs and audit_logs tables

CREATE SEQUENCE IF NOT EXISTS portal.api_logs_id_seq;
ALTER TABLE portal.api_logs ALTER COLUMN id SET DEFAULT nextval('portal.api_logs_id_seq');

CREATE SEQUENCE IF NOT EXISTS portal.audit_logs_id_seq;
ALTER TABLE portal.audit_logs ALTER COLUMN id SET DEFAULT nextval('portal.audit_logs_id_seq');
