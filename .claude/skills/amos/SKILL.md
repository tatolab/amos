---
name: amos
description: >
  Load when working with amos nodes — markdown files carrying `whoami: amos` frontmatter — or when
  the user refers to "the plan", "the DAG", "the roadmap", "the tickets", "what's next", or
  similar work-tracking concepts. Installs the amos conventions (canonical naming, node format,
  when to reach for which sub-skill) into context so amos-graph, amos-show, amos-create, and
  amos-notify can be used correctly. Does not run the CLI on its own.
---

# Amos

Amos is a CLI preprocessor over markdown files. Each file with `whoami: amos` frontmatter is a
**node** in a dependency graph. Typical use: project plans, tickets, tasks, or work items that
span external trackers (GitHub issues, etc.) with `down:` / `up:` edges between them.

## The canonical name rule — NON-NEGOTIABLE

Every node has an identifier in its `name:` field. Use the adapter-qualified form:

```yaml
name: "@github:<owner>/<repo>#<issue-number>"
```

e.g. `name: "@github:tatolab/streamlib#326"`.

**Never put human-readable text in `name:`.** `name:` is a stable identifier used for edge
resolution. Every `down:` and `up:` edge references another node by this string, so edits to it
silently break the DAG. Put titles, summaries, and routing hints in `description:` (free-form).

If you see a node with a free-text name (e.g. `name: "Learning doc: ..."`), that's drift and
should be fixed — rename to the canonical form and move the free text into `description:`.

## Node file format

```markdown
---
whoami: amos
name: "@github:<owner>/<repo>#<N>"
status: pending | in-progress | completed
description: "<short routing hint — free-form>"
github_issue: <N>
dependencies:
  - "down:@github:<owner>/<repo>#<M>"   # this node blocks M
  - "up:@github:<owner>/<repo>#<K>"     # this node waits on K
adapters:
  github: builtin
---

@github:<owner>/<repo>#<N>

Local agent instructions below the adapter reference. These stay local even when
the remote issue body is resolved by the adapter.
```

Filename convention: `<N>-<short-kebab-slug>.md` under `plan/` (or wherever amos scans).

## Sub-skill routing

- **amos-graph** — print the full dependency tree. Use for "what's open", "what's next",
  "what's blocked", "show me the roadmap", orientation queries.
- **amos-show** — resolve a single node to full content including `@`-references. Use for
  "tell me about #N".
- **amos-create** — scaffold a new node (user-invoked via `/amos-create`).
- **amos-notify** — post a message to a node's source system, e.g. GitHub issue comment
  (user-invoked via `/amos-notify`).

## The CLI

Installed binary: `$HOME/.local/bin/amos` (placed there by `tatolab/amos`'s `install.sh`).

Sub-skills invoke it as `"$HOME/.local/bin/amos" <subcommand>`. Don't prefix with `bash` — it's a
compiled binary, not a script.
