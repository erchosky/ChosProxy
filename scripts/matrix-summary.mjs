#!/usr/bin/env node
// Counts the rows of docs/BURP_PARITY_MATRIX.md by area and status and
// rewrites its Summary table (or, with --check, fails when the table does
// not match the rows). Run it whenever a row's status changes.
import { readFileSync, writeFileSync } from "node:fs";

const FILE = new URL("../docs/BURP_PARITY_MATRIX.md", import.meta.url);
const STATUSES = ["🟢", "🟡", "🔵", "➖"];
const text = readFileSync(FILE, "utf8");
const lines = text.split("\n");

const areas = [];
let area = null;
for (const line of lines) {
  const heading = /^## (.+)$/.exec(line);
  if (heading) {
    const name = heading[1].trim();
    area = name === "Summary" || name.startsWith("Superset") ? null : { name, counts: {}, rows: 0 };
    if (area) areas.push(area);
    continue;
  }
  if (!area) continue;
  const row = /^\| ([A-Z]+-\d+[a-z]?) \|/.exec(line);
  if (!row) continue;
  const status = line.split("|")[4]?.trim() ?? "";
  // A superset (⭐) is a complete row.
  const known =
    STATUSES.find((s) => status.startsWith(s)) ??
    (status.startsWith("⭐") ? "🟢" : status.startsWith("⬜") ? "⬜" : null);
  if (!known) throw new Error(`${row[1]}: unknown status "${status}"`);
  area.rows += 1;
  area.counts[known] = (area.counts[known] ?? 0) + 1;
}

const total = { rows: 0, counts: {} };
const out = ["| Area | Rows | 🟢 | 🟡 | 🔵 | ➖ |", "|---|---|---|---|---|---|"];
let unassigned = 0;
for (const a of areas) {
  out.push(`| ${a.name} | ${a.rows} | ${STATUSES.map((s) => a.counts[s] ?? 0).join(" | ")} |`);
  total.rows += a.rows;
  for (const s of STATUSES) total.counts[s] = (total.counts[s] ?? 0) + (a.counts[s] ?? 0);
  unassigned += a.counts["⬜"] ?? 0;
}
out.push(
  `| **Total** | **${total.rows}** | ${STATUSES.map((s) => `**${total.counts[s] ?? 0}**`).join(" | ")} |`,
);

const start = lines.findIndex((l) => l.startsWith("| Area | Rows |"));
let end = start;
while (end < lines.length && lines[end].startsWith("|")) end++;
if (start < 0) throw new Error("no Summary table found");
const current = lines.slice(start, end).join("\n");
const next = out.join("\n");
const summary = `${total.rows} rows: ${STATUSES.map((s) => `${total.counts[s] ?? 0} ${s}`).join(", ")}${unassigned ? `, ${unassigned} ⬜` : ""}`;
if (process.argv.includes("--check")) {
  if (current !== next) {
    console.error("The Summary table of the parity matrix does not match its rows. Run: node scripts/matrix-summary.mjs");
    process.exit(1);
  }
  console.log(`Parity matrix summary is up to date (${summary}).`);
} else {
  lines.splice(start, end - start, ...out);
  writeFileSync(FILE, lines.join("\n"));
  console.log(`Summary rewritten (${summary}).`);
}
