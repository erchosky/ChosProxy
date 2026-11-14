# Contributing

Thanks for helping! Read [ARCHITECTURE.md](ARCHITECTURE.md) and
[DEVELOPMENT.md](DEVELOPMENT.md) first.

## Ground rules

* Work on the current milestone in [ROADMAP.md](ROADMAP.md). Features from
  later milestones need an issue/ADR first.
* **No fake UI.** A visible control must work, be clearly labelled
  experimental, or be absent.
* Every bug fix comes with a regression test.
* Hard-to-reverse decisions get an ADR in `docs/adr/`.
* Security-relevant changes update [THREAT_MODEL.md](THREAT_MODEL.md).
* No offensive payload collections, credential lists, evasion or
  destructive automation (see [SECURITY.md](SECURITY.md)).

## Code quality

Rust: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, typed errors
(`thiserror`), no `unwrap()` on runtime data, doc comments on public items.

TypeScript: strict mode, no `any`, components small and reusable, IPC only
through `src/services/`, never `dangerouslySetInnerHTML`.

## Commits

Small, focused, Conventional-Commit style:

```
feat(proxy): add HTTP listener
fix(http): handle chunked trailers
test(storage): cover migration from v1
docs(adr): record storage decision
```

Run `pnpm check` (lint + typecheck + tests, Rust and TS) before pushing.
