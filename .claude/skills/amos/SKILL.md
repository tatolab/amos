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

Amos is a CLI preprocessor that overlays a local markdown cache on the adapter's source of truth
(GitHub issues today, other trackers via adapter). The adapter is authoritative for what issues
exist, their state, milestone, and dependency edges. Local plan files are optional overlays that
hold AI-specific notes or pre-GA edges not yet pushed upstream.

Read: the DAG is fetched fresh from GitHub on every `amos` call. Issues appear whether or not
they have a local plan file.

## The canonical name rule — NON-NEGOTIABLE

Every node — whether materialized from GitHub directly or from a local plan file — is identified
by an adapter-qualified name:

```
@github:<owner>/<repo>#<issue-number>
```

e.g. `@github:tatolab/streamlib#326`. This is the string amos uses for every edge lookup; every
`blocked_by:` / `blocks:` / `related_to:` reference uses it.

## Dependency edges: GitHub native > plan file

GitHub's issue UI now exposes **five typed relationships** (blocked-by, blocks, parent,
sub-issue, duplicate). Amos reads these natively via GraphQL (`blockedBy`, `blocking`,
`parent`, `subIssues`). Any edge declared through the GitHub UI or REST/GraphQL API is
visible to `amos graph` / `next` / `blocked` with zero local state.

**Plan files are no longer required to declare edges.** The old `blocked_by:` / `blocks:`
frontmatter fields still work — they're merged with GitHub's native edges — but adding new
edges should go through GitHub's UI or `gh api graphql` so the dependency graph stays in one
place.

To migrate a project's existing plan-file edges to GitHub native:

```bash
"$HOME/.local/bin/amos" sync-edges --dry-run --dir <project-root>   # preview
"$HOME/.local/bin/amos" sync-edges --dir <project-root>             # apply
```

The operation is idempotent; re-running is safe.

## Optional plan file format

Only create a plan file when you have AI-specific notes that shouldn't live in the public GitHub
issue body. The file format is:

```markdown
---
whoami: amos
name: "@github:<owner>/<repo>#<N>"
description: "<short routing hint — free-form>"
# blocked_by / blocks are optional — prefer setting these in GitHub's UI instead.
adapters:
  github: builtin
---

Local agent instructions here. Kept local so they don't clutter the public issue.
```

Filename convention: `<N>-<short-kebab-slug>.md` under `plan/`.

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
