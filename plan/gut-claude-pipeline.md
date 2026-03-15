---
whoami: amos
name: "@github:tatolab/amos#3"
description: Remove all Claude API calls — amos is a data tool, not an AI orchestrator
dependencies:
  - "down:@github:tatolab/amos#5"
  - "down:@github:tatolab/amos#6"
---

Delete from `output.rs`:
- `call_claude()` — amos doesn't call claude
- `parse_node_selection()` — no phase 1
- `build_summary_prompt()` — replaced by plain DAG formatter
- `build_spec_prompt()` — no phase 2
- `format_context_ref()` — only used by deleted functions

Delete from `main.rs`:
- All phase 1 / phase 2 logic
- The prompt handling

Remove `serde_json` from `Cargo.toml` — only used for parsing claude's response.

Amos becomes: scan → parse → build DAG → print.
