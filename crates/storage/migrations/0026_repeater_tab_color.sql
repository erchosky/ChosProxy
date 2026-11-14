-- M31b: a highlight colour per Repeater tab ('' = none; a group's colour
-- still marks the tabs of the group).
ALTER TABLE repeater_tabs ADD COLUMN color TEXT NOT NULL DEFAULT '';
