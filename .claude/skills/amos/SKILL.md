---
name: amos
description: >
  Activate when you see whoami: amos in markdown frontmatter.
  Amos is a CLI preprocessor that resolves content in markdown
  files and builds dependency graphs.
allowed-tools: Bash(amos:*)
---

Amos preprocesses markdown files containing `whoami: amos` frontmatter.
It resolves references in the content through adapters, caches results
locally, and returns fully formed output.

Run `amos` to preprocess all nodes and see the dependency graph with
resolved content for actionable nodes.

!`amos 2>/dev/null || true`
