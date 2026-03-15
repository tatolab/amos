---
whoami: amos
name: "gh:12"
description: Adapter trait and config system for syncing status from external systems
dependencies:
  - "up:gh:8"
  - "up:gh:9"
  - "down:gh:13"
---

Add `.amosrc.toml` support at the scan root. Minimal config:
```toml
[adapters.gh]
# repo defaults to current repo via `gh repo view`
```

Adapter trait:
- `resolve_status(node_name) -> Option<Status>` — query external system
- `sync(nodes) -> HashMap<String, Status>` — batch sync

Node name prefix determines adapter: `gh:15` → gh adapter, `jira:PROJ-42` → jira adapter, plain name → local only.

`amos sync` iterates all nodes with adapter prefixes, calls the adapter, writes `.amos-status`.
