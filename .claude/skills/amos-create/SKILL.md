---
name: amos-create
description: >
  Scaffold a new amos node with proper frontmatter. Use when the user wants to add a new
  ticket, task, or plan entry tracked by amos. Produces a markdown file with the canonical
  `@github:<owner>/<repo>#<N>` name, a description, dependencies, and the adapter block.
disable-model-invocation: true
argument-hint: "<issue-number> <one-line-description>"
allowed-tools: Bash
---

When the user invokes `/amos-create`, scaffold a new node using the template below. See also
`references/node-format.md` for the bare template.

## Inputs

- `<issue-number>` — the GitHub (or other adapter) issue number this node represents.
- `<one-line-description>` — short routing hint (free-form). This is not the identifier.
- Determine `<owner>/<repo>` from the git remote of the current project, or ask the user if
  ambiguous.

## Output: `plan/<N>-<kebab-slug>.md`

```markdown
---
whoami: amos
name: "@github:<owner>/<repo>#<N>"
status: pending
description: "<one-line routing hint>"
github_issue: <N>
dependencies:
  - "down:@github:<owner>/<repo>#<M>"   # this node blocks M
  - "up:@github:<owner>/<repo>#<K>"     # this node waits on K
adapters:
  github: builtin
---

@github:<owner>/<repo>#<N>

Local agent instructions below the adapter reference. These persist locally
even when the remote issue body changes.
```

## Naming rules — strict

- `name:` is an **identifier**, not a title. Always `@github:<owner>/<repo>#<N>`.
- Put the human-readable title in `description:`.
- `github_issue:` should match the `#N` in the name.
- Filename: `<N>-<short-kebab-slug>.md` under the `plan/` directory.

**Why this matters:** every `down:` / `up:` edge references another node by its literal `name:`
string. If you put "Refactor auth module" in `name:` and later rename it to "Refactor auth", every
edge pointing at the node breaks silently — the DAG edge resolver just finds no match and drops
it. Canonical `@github:...` names never need renaming; they're stable as long as the issue exists.

## Dependencies

- `down:<target>` — this node blocks `<target>`; `<target>` is waiting on this.
- `up:<target>` — this node is waiting on `<target>`; `<target>` blocks this.

Prefer writing a single `down:` on the upstream node over an `up:` on the downstream. That gives
each edge exactly one source of truth and matches how the CLI is typically used.

## Adapters

`adapters: { github: builtin }` is the default and enables the built-in GitHub adapter. The
adapter is what lets `@github:<owner>/<repo>#<N>` references in the body resolve to the issue's
content. Leave as `builtin` unless wiring a custom adapter.

## After scaffolding

Run **amos-graph** to confirm the new node appears and its `down:` / `up:` edges resolve:

```bash
"$HOME/.local/bin/amos" graph | grep "#<N>"
```

If an edge target doesn't exist, the new node will appear at the top level (orphaned) instead
of nested under its parent.
