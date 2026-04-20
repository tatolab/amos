---
name: amos-focus
description: >
  Set the milestone the user is currently working on. `amos next`, `amos
  blocked`, and `amos graph` scope their output to this milestone once set.
  Use when the user says "let's work on <milestone>", "focus on <milestone>",
  "switch to <milestone>", or wants to check which milestone is currently
  active. Accepts free-form milestone text — the tool resolves it against
  the adapter's known titles.
argument-hint: "<milestone-title> | --clear | (no args → print current)"
allowed-tools: Bash
---

```bash
"$HOME/.local/bin/amos" focus "<milestone title>"
"$HOME/.local/bin/amos" focus --clear
"$HOME/.local/bin/amos" focus              # print current
```

Focus is stored in `.amosrc.toml` at the scan root — persistent across
sessions. Scopes `amos next`, `amos blocked`, `amos orphans`, and (future)
`amos graph --focused` to items in that milestone.

## When to use

- "Let's work on GPU Capability Rewrite" → `amos focus "GPU Capability Rewrite"`
- "What milestone am I on?" → `amos focus` (no args)
- "Clear focus, show me everything" → `amos focus --clear`

## Discovering milestone titles

If the user doesn't know the exact title, list them first:

```bash
"$HOME/.local/bin/amos" milestones
```

Prints every milestone the adapter knows about with open/closed counts and a
`*` marker on the currently focused one. Then feed the exact string back to
`amos focus "<title>"`.

## Interaction with `amos next`

Once focused, `amos next` only returns ready-to-start nodes **in that
milestone**. If the milestone has nothing ready (e.g. all remaining items are
blocked by each other or by closed adapter nodes), `amos next` exits with a
helpful message naming the focused milestone.
