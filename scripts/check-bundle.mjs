#!/usr/bin/env node
// Fails when the startup script of the desktop app grows past its budget
// (run after `npx vite build` in apps/desktop). Views and the code editor
// are loaded on demand; an eager import of the editor shows up here.
import { readdirSync, statSync } from "node:fs";

const BUDGET_KB = 400;
const dir = new URL("../apps/desktop/dist/assets/", import.meta.url);
let entry;
try {
  entry = readdirSync(dir).find((f) => /^index-.*\.js$/.test(f));
} catch {
  entry = undefined;
}
if (!entry) {
  console.error("No built bundle: run `npx vite build` in apps/desktop first.");
  process.exit(1);
}
const kb = Math.round(statSync(new URL(entry, dir)).size / 1000);
if (kb > BUDGET_KB) {
  console.error(`The startup script is ${kb} kB (budget ${BUDGET_KB} kB): something large is imported by the shell.`);
  process.exit(1);
}
console.log(`Startup script: ${kb} kB (budget ${BUDGET_KB} kB).`);
