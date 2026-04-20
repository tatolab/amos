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

Use `--json` for machine-readable output; parse with `jq`. Then craft a
natural-language confirmation for the user — don't echo raw JSON.

```bash
"$HOME/.local/bin/amos" focus "<milestone title>" --json
"$HOME/.local/bin/amos" focus --clear --json
"$HOME/.local/bin/amos" focus --json              # read current
```

## JSON shapes

- Set: `{"focus": "<title>", "action": "set"}`
- Clear: `{"focus": null, "action": "cleared"}`
- Read: `{"focus": "<title>" | null}`

Extract with `jq -r '.focus'`.

## When to use

- "Let's work on GPU Capability Rewrite" → set
- "What milestone am I on?" → read (no args)
- "Clear focus, show me everything" → clear

## Discovering milestone titles

If the user doesn't know the exact title:

```bash
"$HOME/.local/bin/amos" milestones --json | jq -r '.milestones[].title'
```

Pick the one that matches, then feed the exact string back to `amos focus`.

## Presenting the result

After running the command, craft a short confirmation in natural language.
Example for a set:

> Focused on **GPU Capability Rewrite**. `/amos:next` will now surface
> only ready tasks from this milestone.

For a read with `focus: null`:

> No milestone currently focused. Run `/amos:next` and I'll rank the
> candidates for you.
