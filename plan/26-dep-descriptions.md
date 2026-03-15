---
whoami: amos
name: "@github:tatolab/amos#26"
description: DAG readability — inline descriptions on dependency lines
adapters:
  github: builtin
---

@github:tatolab/amos#26

One-line fix in `output.rs` — in the `format_dag` function where dependency lines are built, look up the node's description from the DAG and append it after the status. Same for the `blocks:` lines. Don't resolve from GitHub — use the local description field only (it's a routing hint, always available).
