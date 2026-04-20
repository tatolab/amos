---
name: amos-show
description: >
  Resolve a single amos node to its fully expanded content — frontmatter summary plus the
  remote body fetched via the adapter (e.g. GitHub issue body). Use when the user asks about
  a specific ticket or node ("tell me about #322", "what's #319 about", "show me the MoQ
  plan"). Use amos-graph for structural views across many nodes, and Read for just the local
  markdown without a remote fetch.
allowed-tools: Bash
---

```bash
"$HOME/.local/bin/amos" show "<node-name>"
```

`<node-name>` must be the node's canonical `name:` value:

```bash
"$HOME/.local/bin/amos" show "@github:tatolab/streamlib#319"
```

## Output

The node's frontmatter as a summary, followed by the resolved body — which, for a
`@github:...` node, is the fetched GitHub issue body plus any local agent instructions
kept below the adapter reference in the plan file.

## When to use

- "Tell me about issue #N" / "what's #N about"
- Need the remote issue body inline alongside local plan instructions
- Reviewing a single node's full context before starting work on it

## When NOT to use

- Want the DAG / dependency structure → use **amos-graph**.
- Want just the raw local markdown (no network fetch) → use **Read** on the plan file
  directly. Much faster, and the local agent instructions are usually enough.
- Want to post a comment back to the tracker → use **amos-notify**.

## If the name doesn't resolve

`amos show` matches by the literal `name:` field in the file's frontmatter. If a node still
has a free-text name (drift from canonical form — see the **amos** skill), pass that literal
string. Prefer fixing the node to canonical `@github:<owner>/<repo>#<N>` form first — broken
identifiers silently break dependency edges elsewhere in the DAG.
