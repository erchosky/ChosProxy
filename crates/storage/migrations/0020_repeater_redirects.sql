-- Schema v20: each Repeater tab keeps its redirect policy
-- (never, onSite, inScope or always).

ALTER TABLE repeater_tabs ADD COLUMN redirect_policy TEXT NOT NULL DEFAULT 'never';
