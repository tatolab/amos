# Amos Frontmatter Specification v0.6

## Overview

Amos scans markdown files, finds `whoami: amos` blocks, builds a typed-edge
dependency graph, and reports what he sees. He reads the ship and reports
what he finds — he does not own the underlying work items; GitHub (or other
adapters) do.

## Block format

```yaml
---
whoami: amos
name: "@github:tatolab/myproject#42"
description: Short routing hint — enough to know when this node is relevant
blocked_by:
  - "@github:tatolab/myproject#41"
blocks:
  - "@github:tatolab/myproject#43"
related_to:
  - "@github:tatolab/otherproject#7"
duplicates: "@github:tatolab/myproject#40"
superseded_by: "@github:tatolab/myproject#99"
labels: [refactor, gpu]
priority: p2
context:
  - path/to/relevant/file.rs
  - "@github:org/repo#path/to/file"
adapters:
  github: builtin
---

@github:tatolab/myproject#42

Agent-specific instructions, implementation notes, constraints. The
`@reference` above lazily pulls the issue body at expansion time. Local
content here supplements what the issue says.
```

Each `.md` file contains at most one amos block, which must be the first
frontmatter block in the file. Leading blank lines are allowed.

## Fields

### `whoami` (required)

Must be `amos`. The discriminator — blocks without it are ignored.

### `name` (required)

The stable identifier for this node. Use the adapter-qualified URI form
whenever possible: `@github:owner/repo#N`. This:

- Is self-documenting.
- Survives external edits — GitHub can't rename an issue number.
- Matches the adapter scheme used to resolve the body.

**Do not put free-form descriptive text in `name`.** `name` is the edge-resolution
key across the DAG. If another node has `blocked_by: [<this-name>]`, the
match is string-equality; a renamed `name` silently breaks every referring
edge. Use `amos rename <old> <new>` to change a name safely — it rewrites
every reference in the scan tree.

### `description` (optional)

A routing hint — tells agents and humans *when this node is relevant*, not
what it contains. Think of it like a skill description: enough to decide
"should I look at this?" without loading the full body.

Keep it short (one line). The actual content lives in the body, resolved
lazily from `@` references.

### Relationships

Each relationship field is a distinct edge type in the DAG. Queries
(`amos next`, `amos blocked`, `amos validate`) filter by kind.

| Field             | Shape              | Meaning                                           | Mirror on target      |
| ----------------- | ------------------ | ------------------------------------------------- | --------------------- |
| `blocked_by`      | list of names      | These must complete before this one can start.    | `blocks`              |
| `blocks`          | list of names      | These can't start until this one is done.         | `blocked_by`          |
| `related_to`      | list of names      | Soft association — no ordering or gating.         | `related_to` (sym.)   |
| `duplicates`      | single name        | This node duplicates another (target is canon.).  | —                     |
| `superseded_by`   | single name        | This node has been replaced by the target.        | —                     |

Declaring an edge from either side is sufficient — amos deduplicates. If
both sides declare the same edge, it's still one edge.

### Grouping

There is intentionally no `parent:` / `children:` field. Grouping is
delegated to **GitHub milestones**, resolved through the adapter. Amos
displays milestone membership as a visual grouping in the tree and provides
milestone-scoped queries, but the milestones themselves live in GitHub.

### Attributes

| Field       | Shape           | Example                                  |
| ----------- | --------------- | ---------------------------------------- |
| `labels`    | list of strings | `[refactor, gpu]`                        |
| `priority`  | enum            | `p0` / `p1` / `p2` / `p3` (p0 = highest) |

Status does **not** live in frontmatter — it belongs in `.amos-status` at
the scan root. Rationale: keeping status out of the plan files avoids
rewriting spec documents every time a ticket moves; diffs show status
changes as a single line in one place; sync from external adapters is
unambiguous about who owns the value.

### `context` (optional)

Pointers to files or references relevant to understanding this node:

- Bare path: local file relative to the scan root
- `@github:<owner/repo>[#<path>][@<ref>]`: GitHub file reference
- `@url:<url>`: arbitrary URL

## Node body

The body (everything after the closing `---`) contains two kinds of content:

1. **`@` references** — lines starting with `@scheme:reference` are lazily
   resolved through adapters when the node is expanded. They pull in
   external content (issue descriptions, file contents, images, video
   frames).

2. **Local content** — everything else. Agent-specific instructions,
   implementation constraints, notes that shouldn't live in the external
   system.

This separation means: the issue has the *what*, the local markdown has
the *how-for-AI*.

### Built-in `@` reference schemes

| Scheme     | Example                        | Resolves to                                   |
| ---------- | ------------------------------ | --------------------------------------------- |
| `@github:` | `@github:tatolab/amos#11`      | GitHub issue title + body                     |
| `@file:`   | `@file:src/main.rs`            | File contents (text inline, images as paths)  |
| `@url:`    | `@url:https://example.com/doc` | Downloaded content (images cached locally)    |
| `@ffmpeg:` | `@ffmpeg:recordings/bug.mp4`   | Keyframes extracted as images                 |

External adapters can be registered in `.amosrc.toml` for any scheme.

## Status

Status is stored in `.amos-status` at the scan root. Node files are
immutable specs — they describe what should happen, not what has happened.

```
- [x] @github:tatolab/amos#1
- [~] @github:tatolab/amos#11
- [ ] @github:tatolab/amos#16
```

`[x]` = done, `[~]` = in-progress, `[ ]` or absent = pending.

Status resolution:

1. If `.amos-status` says done → **done**.
2. If `.amos-status` says in-progress → **in-progress**.
3. Else: **pending**.

Future: `amos sync` will reconcile `.amos-status` with adapter state
(closed GitHub issue → done, open + "in-progress" label → in-progress).

## CLI

```
amos                       # scan cwd, print the DAG summary
amos --dir <path>          # override scan root
amos graph                 # print the dependency tree as ASCII
amos show <node>           # print a single node with its resolved body
amos validate              # run DAG integrity checks (non-zero exit on errors)

amos next                  # ready-to-start: all blockers done, not yet done
amos blocked               # blocked: has at least one non-done blocker
amos orphans               # no relationships of any kind

amos done <node>           # mark node done in .amos-status
amos start <node>          # mark node in-progress
amos reset <node>          # clear node's .amos-status entry

amos notify <node> <msg>   # send a message through the node's adapter
amos rename <old> <new>    # rename a node, rewriting every reference
amos migrate [--dry-run]   # convert legacy frontmatter to the typed model
```

Output goes to stdout. Status messages (informational) go to stderr.
`--dry-run` is available on commands that mutate files (`migrate`, `rename`).

## Discovery

Scans all `.md` files under scan root, respecting `.gitignore`. `.git` is
skipped.

## Output

The default `amos` output is structured markdown:

- **Issues** — validation problems (cycles, missing dependencies).
- **DAG summary** — one line per node: name, adapter facts, description.
- **Topological Order** — `node-a → node-b → node-c`.
- **Critical Path** — longest blocker chain.
- **Detail** — for each node with a body, the resolved content with
  `@` references expanded through adapters.

The `amos graph` output is the DAG as an ASCII tree. Children are ordered
by the parent's declared `blocks:` list (authoring order), with any extras
appended sorted numerically so `#99` comes before `#100`. `(→ see above)`
back-references prevent re-printing a subtree twice.

## Adapters

Adapters resolve URI schemes. Built-in adapters are always available.
External adapters are registered in `.amosrc.toml`:

```toml
[adapters.jira]
command = "npx @openclaw/amos-adapter-jira"
```

External adapters are any executable that speaks the protocol:

```
<command> resolve <reference>  → JSON to stdout
<command> batch <json-array>   → JSON object keyed by reference
```

JSON shape:

```json
{
  "name": "optional string",
  "description": "optional string",
  "status": "done | in-progress | null",
  "body": "optional string",
  "facts": { "state": "open", "labels": "..." }
}
```

## Migration from legacy format

Amos previously used a single `dependencies:` list with `up:`/`down:`
prefixes and an in-frontmatter `status:` field. Both are gone. The parser
errors on either, pointing to `amos migrate`.

`amos migrate` does literal conversion:

- `up:X` → `blocked_by: [X]`
- `down:X` → `blocks: [X]`
- `status: <value>` → removed from frontmatter; non-pending values move to
  `.amos-status`.

No heuristics, no interpretive logic. Grouping stays in GitHub milestones.
