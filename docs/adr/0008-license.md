# ADR-0008: Open-source license selection

* Status: **Proposed — awaiting the project owner's decision**
* Date: 2026-10-21

## Context
The project must be open source. The choice affects who can embed, fork or
host it, and how plugins interact with it.

## Options

| | MIT | Apache-2.0 | GPLv3 | AGPLv3 |
|---|---|---|---|---|
| Type | Permissive | Permissive | Strong copyleft | Network copyleft |
| Patent grant | No explicit | **Explicit** + retaliation | Explicit | Explicit |
| Proprietary forks | Allowed | Allowed | Must release source when distributed | Also when offered as a network service |
| Plugin implications | None | None | Plugins linking in-process may be derivative works → GPL | Same as GPL |
| Rust ecosystem fit | Very common (often dual MIT/Apache) | Very common | Less common | Rare |
| Corporate adoption | Easiest | Easy (lawyers like patent clause) | Harder | Hardest |
| Protects against closed SaaS clones | No | No | No | **Yes** |

## Recommendation
**Apache-2.0** (or the Rust-customary dual `MIT OR Apache-2.0`): maximizes
adoption by security teams and plugin authors, gives an explicit patent
grant, and all current dependencies are compatible. Choose **GPLv3** if
preventing proprietary forks matters more than adoption; **AGPLv3** only if
a hosted/collaborative ChosProxy service is a real concern.

## Status
No `LICENSE` file is committed and crates carry no `license` field until the
owner confirms. Dependencies are checked for compatibility with all four
options (none are GPL-only at this time).
