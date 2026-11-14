-- Schema v19: a note per Repeater tab (Markdown, shown as text).

ALTER TABLE repeater_tabs ADD COLUMN note TEXT NOT NULL DEFAULT '';
