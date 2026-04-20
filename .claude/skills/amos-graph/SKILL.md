---
name: amos-graph
description: >
  Print the amos dependency tree as a nested ASCII graph. Use when the user asks about open
  tasks, the roadmap, what's next, what's blocked, what depends on X, or wants a structural
  view of the plan. Returns every node with status and a short description; does NOT resolve
  `@github:...` references (use amos-show for full content). Trigger on phrases like "what's
  on my plate", "what's open", "show me the DAG", "what's blocking X", "what depends on Y",
  "where are we on <umbrella>".
allowed-tools: Bash
---

Run from the project root (the directory containing amos-scanned markdown files):

```bash
"$HOME/.local/bin/amos" graph
```

## Output format

Nested tree, indent encodes depth. Each line looks like:

```
[state=OPEN] #319 GPU capability-based access (sandbox + escalate) — Umbrella — replace runtime-phase checks ...
    ├── [state=CLOSED] #320 Design doc: ... — Write the design doc gating all downstream work ...
    ├── [state=CLOSED] #321 Introduce GpuContext... — Add the two capability types as thin newtype wrappers ...
    └── [state=OPEN] #326 Learning doc: ... — Capture the "why" of the sandbox/escalate pattern ...
        ├── [state=CLOSED] #325 (→ see above)
        ├── [state=OPEN] #369 Polyglot escalate: ... — Follow-up to #325. Extend the polyglot escalate IPC ...
        └── [state=OPEN] #370 xtask: JTD discriminator schemas — Update xtask/src/generate_schemas.rs ...
```

Legend:
- `[state=OPEN]` / `[state=CLOSED]` — lifecycle state (from the adapter, e.g. GitHub issue state).
- `[labels=...]` — tracker labels, when present.
- `#N` — GitHub issue number, derived from the canonical `@github:<owner>/<repo>#N` node name.
- Text after the em-dash — the node's `description:` field (a routing hint).
- `(→ see above)` — back-reference; the subtree was already printed earlier.

## When to use

- "What's next" / "what's on my plate" / "what's open"
- "What's blocking X" / "what does Y depend on"
- Finding the next unblocked task under an umbrella
- Orientation in a new or unfamiliar amos-tracked project
- Spotting broken edges (orphan nodes appear at the top level instead of nested)

## When NOT to use

- Need the full resolved body of a single node (GitHub issue text, etc.) → use **amos-show**.
- Just want the raw markdown of one plan file → use **Read** on the file directly (no
  network, much faster).
- Need to post a comment to the tracker → use **amos-notify**.

## Filtering large trees

Output can be long (hundreds of lines on mature projects). To zoom in, pipe through grep with
context:

```bash
# Show a specific umbrella and its children
"$HOME/.local/bin/amos" graph | grep -A 30 "#319 "

# Find all open tasks
"$HOME/.local/bin/amos" graph | grep "state=OPEN"

# Find orphan nodes (no parent, broken edges)
"$HOME/.local/bin/amos" graph | grep -B 0 "^[A-Z\[]"
```
