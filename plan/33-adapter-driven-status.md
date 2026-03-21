---
whoami: amos
name: "@github:tatolab/amos#33"
description: Adapter-driven status — statuses come from external systems, not hardcoded enums
adapters:
  github: builtin
---

Amos is a preprocessor — it returns raw facts from adapters and lets
the consuming agent (Claude) interpret what they mean. Amos does not
decide what "done", "ready", or "blocked" means.

## What changed

- `ManualStatus` enum removed entirely
- `ComputedStatus` enum removed entirely — no `is_done()`, no `is_actionable()`
- `ResourceFields.status` replaced with `facts: HashMap<String, String>` —
  adapters return whatever key-value facts they have (state, labels, etc.)
- GitHub adapter returns raw facts: `{"state": "CLOSED", "labels": "bug, priority-high"}`
- External adapters pass through all JSON fields as facts
- `.amos-status` stores raw strings: `[x]`→"done", `[~]`→"in-progress",
  `[closed]`→"closed", `[In Review]`→"In Review"
- Output shows raw overlay status without interpretation — no `[ready]`/`[blocked]`
- Prune only removes nodes explicitly marked `[x]` (done) in `.amos-status`
- The DAG is a pure data structure: graph + overlay. No status computation.
