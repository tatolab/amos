---
name: amos
description: >
  Activate when you see markdown with whoami: amos frontmatter,
  @github: or @file: or @ffmpeg: references, .amos-status files,
  or when the user mentions work streams, dependency graphs,
  or plan nodes. Amos is a CLI tool that resolves these references
  and builds dependency graphs from markdown files.
---

# Amos

Amos is a lightweight CLI that scans markdown files for `whoami: amos` frontmatter blocks, builds a dependency DAG, and resolves `@` references through adapters. It returns fully formed content — you don't need to fetch anything yourself.

## Current DAG State

!`amos 2>/dev/null || echo "amos not installed or no amos blocks found in cwd"`

## What You're Looking At

When you see a block like this in a markdown file:

```yaml
---
whoami: amos
name: "@github:tatolab/streamlib#143"
description: Package system — making streamlib.yaml self-describing
adapters:
  github: builtin
---

@github:tatolab/streamlib#143

Agent-specific notes here.
```

This is an amos node. The parts:
- **name** — node identity, often a `@github:owner/repo#N` URI
- **description** — routing hint (when is this relevant, not what it contains)
- **dependencies** — `up:` (must complete first) and `down:` (waits for this)
- **adapters** — where to find resolvers (`builtin` or `@github:org/repo#path`)
- **body** — `@scheme:reference` lines resolve lazily through adapters, local text passes through

## Commands

Use these to interact with amos:

- `amos` — dump the full DAG with resolved bodies for ready/in-progress nodes
- `amos show <name>` — resolve a single node fully (pass the name field value)
- `amos graph` — ASCII dependency tree
- `amos sync` — pull status from external systems (GitHub issue open/closed state)
- `amos notify <node> <message>` — post a message to the node's source (e.g. GitHub comment)
- `amos prune` — remove done nodes not needed by active work

## @ References

Lines starting with `@scheme:reference` in node bodies resolve through adapters:

| Scheme | Example | What it returns |
|--------|---------|----------------|
| `@github:` | `@github:tatolab/amos#22` | GitHub issue title + body |
| `@file:` | `@file:src/main.rs` | File contents inline |
| `@url:` | `@url:https://example.com/doc` | Downloaded content |
| `@ffmpeg:` | `@ffmpeg:recording.mp4` | Keyframe images from video |
| `@exec:` | `@exec:git log --oneline -5` | Command output |

These are already resolved in the amos output — you read the result directly.

## Status

Node status is stored in `.amos-status` (not in frontmatter). Computed status:
- **done** — marked in `.amos-status` or synced from adapter (e.g. closed GitHub issue)
- **in-progress** — marked in `.amos-status`
- **ready** — all upstream dependencies are done
- **blocked** — upstream work is not done yet

## Creating Nodes

To create a new amos node, write a markdown file with this frontmatter:

```yaml
---
whoami: amos
name: "<identifier or @github:owner/repo#N>"
description: "<routing hint — when is this relevant>"
dependencies:
  - "up:<upstream-node-name>"
  - "down:<downstream-node-name>"
adapters:
  github: builtin
---

@github:owner/repo#N

Local agent instructions below the reference.
```

The name can be a plain identifier or a `@github:` URI. If it's a URI, `amos sync` can pull status from the external system.
