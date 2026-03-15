---
whoami: amos
name: rewrite-output
description: New output formatter — structured DAG dump, not a prompt
dependencies:
  - up:gut-claude-pipeline
  - down:update-spec-docs

---

New `output.rs` prints the DAG state to stdout. Compact for done/blocked nodes, expanded with full body for ready/in-progress nodes.

Format:
- DAG summary section: each node on one line with status and deps
- Execution order for remaining work
- Validation issues (cycles, missing deps)
- Expanded section with full bodies for ready/in-progress nodes

The output is designed to be readable by both humans and LLMs. No prompt engineering, no instructions to Claude — just the data.
