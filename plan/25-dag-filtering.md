---
whoami: amos
name: "@github:tatolab/amos#25"
description: Large DAG navigation — filtering and scoping the output
adapters:
  github: builtin
---

@github:tatolab/amos#25

Start with `amos --ready` flag that only outputs nodes with ready/in-progress status. This is the 80% case — agents want actionable work. Don't build a full query language. If more filtering is needed later, separate plan directories (e.g. `plan/backend/`, `plan/frontend/`) are the KISS approach.
