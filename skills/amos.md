---
name: amos
description: >
  Activate when you see markdown files with amos frontmatter blocks
  (--- with whoami: amos), when the user mentions work streams, DAG
  status, dependency tracking, or wants to work on a planned task.
---

# Amos Work Stream Skill

Amos scans `.md` files for `whoami: amos` blocks, builds the dependency
graph, and dumps the state. He's a mechanic — reads the ship, reports
what he sees.

## Invoking Amos

```bash
amos                    # scan cwd, print DAG state
amos --dir /path        # scan specific directory
amos done <node>        # mark node done
amos start <node>       # mark node in-progress
amos reset <node>       # clear status
amos sync               # sync status from GitHub issues / external adapters
```

Output goes to stdout — structured markdown with the DAG summary,
execution order, and expanded detail for ready/in-progress nodes.

## Amos Block Format

```yaml
---
whoami: amos
name: "@github:org/repo#42"
description: Short routing hint — when is this node relevant?
dependencies:
  - "up:@github:org/repo#41"
  - "down:@github:org/repo#43"
---

@github:org/repo#42

Agent instructions that supplement the issue content.
```

- `whoami: amos` — required discriminator
- `name` — plain name or `@scheme:reference` URI (self-documenting, portable)
- `description` — routing hint, not content. Like a skill description: tells the agent *when* to look, not *what's inside*
- Body — mix of `@` references (lazily resolved) and local agent instructions

## `@` References

Lines starting with `@scheme:reference` in the body resolve lazily through adapters:

- `@github:org/repo#42` — pulls GitHub issue body
- `@file:path/to/file.rs` — inlines file content
- `@url:https://example.com/image.png` — downloads to local cache
- `@ffmpeg:recordings/demo.mp4` — extracts keyframes as images

Only expanded for ready/in-progress nodes. Done/blocked nodes stay compact.

## Status

Stored in `.amos-status` (not in frontmatter). Node files are immutable specs.

- **done** = `[x]` in `.amos-status`
- **in-progress** = `[~]` in `.amos-status`
- **ready** = all upstream deps are done
- **blocked** = any upstream dep is not done

`amos sync` pulls status from external systems (GitHub issue open/closed state).

## Using the Output

The output is structured markdown — readable by both humans and LLMs.
Pipe it into other tools, feed it to claude, or just read it.

When work is complete, run `amos done <node>` to update `.amos-status`.
The DAG recomputes and downstream nodes become ready.
