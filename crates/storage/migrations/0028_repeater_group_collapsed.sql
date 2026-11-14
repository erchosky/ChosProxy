-- A Repeater group can be collapsed: the tab strip then shows only its chip
-- (and the selected tab, when it belongs to the group).
ALTER TABLE repeater_groups ADD COLUMN collapsed INTEGER NOT NULL DEFAULT 0;
