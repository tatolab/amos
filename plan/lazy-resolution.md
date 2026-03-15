---
whoami: amos
name: "gh:14"
description: Lazy body resolution with @reference syntax
dependencies:
  - "up:gh:13"
  - "down:gh:15"
---

@gh:tatolab/amos#14

Node bodies contain local agent instructions mixed with `@scheme:reference` lines. References resolve lazily — only when a node is expanded (ready/in-progress in the output).

The resolver:
- Scans body lines for `@scheme:reference` pattern (line must start with `@`)
- Looks up the scheme in the adapter registry
- Calls `adapter.resolve(reference)` and inlines the result
- Non-matching lines pass through as-is

This keeps DAG construction instant (no network calls) and only loads external content when needed.
