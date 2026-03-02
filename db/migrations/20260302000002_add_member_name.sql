-- Add name column to portal.org_members
ALTER TABLE portal.org_members ADD COLUMN name VARCHAR(255) NOT NULL DEFAULT '';
