---
name: amos-sync
description: >
  Pull current state from external systems into the local DAG.
disable-model-invocation: true
allowed-tools: Bash(amos:*)
---

Run `amos sync` to pull status from external systems (e.g.
GitHub issue open/closed state) and update `.amos-status`.
