-- Add invite token support for member invitations
ALTER TABLE portal.org_members
  ADD COLUMN invite_token_hash VARCHAR,
  ADD COLUMN invite_expires_at TIMESTAMPTZ;

CREATE UNIQUE INDEX idx_org_members_invite_token
  ON portal.org_members (invite_token_hash)
  WHERE invite_token_hash IS NOT NULL;
