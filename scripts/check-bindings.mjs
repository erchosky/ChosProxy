// Fails if the TypeScript bindings generated from Rust (ts-rs, written by
// `cargo test`) differ from what is committed. Run after `cargo test`.
import { execFileSync } from "node:child_process";

const dir = "apps/desktop/src/bindings";
const status = execFileSync("git", ["status", "--porcelain", "--", dir], { encoding: "utf8" });
if (status.trim() !== "") {
  console.error(`Generated bindings in ${dir} are out of date. Run \`cargo test\` and commit:\n${status}`);
  process.exit(1);
}
console.log("TypeScript bindings are up to date.");
