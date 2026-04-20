---
name: amos-notify
description: >
  Post a message to a node's source system through its adapter. For a `@github:...` node,
  this creates a comment on the referenced issue. Use when the user wants to share status,
  results, or context with everyone watching the upstream tracker — not for local scratch
  notes. The message is visible to anyone with access to the tracker.
disable-model-invocation: true
argument-hint: "<node-name> <message>"
allowed-tools: Bash
---

```bash
"$HOME/.local/bin/amos" notify "<node-name>" "<message>"
```

`<node-name>` must be the canonical `name:` value of an amos node:

```bash
"$HOME/.local/bin/amos" notify "@github:tatolab/streamlib#326" "Dependency edges updated — #326 now sits behind #369 and #370 per the umbrella ordering."
```

## Adapters

- **github (builtin)** — posts the message as a new comment on the GitHub issue named by the
  node. Supports GitHub-flavored markdown (code fences, links, lists, checkboxes).

## Examples

Short status update:

```bash
"$HOME/.local/bin/amos" notify "@github:tatolab/streamlib#319" "Phase 1 merged. Starting phase 2 now."
```

Longer comment with markdown — use a HEREDOC to keep the quoting sane:

```bash
MSG="$(cat <<'EOF'
## Progress update

- Landed the canonical-name migration (80 plan files).
- Reattached #322 under #319 (was silently orphaned by a stale rename).
- Added missing #369 / #370 edges under #326.

Next: working on #370 (xtask JTD discriminator).
EOF
)"
"$HOME/.local/bin/amos" notify "@github:tatolab/streamlib#319" "$MSG"
```

## When to use

- Reporting progress to everyone watching the issue
- Closing-out notes when finishing a task
- Flagging a blocker that needs the issue reporter's attention

## When NOT to use

- Notes meant only for yourself or the agent → put them in the plan file body instead.
- Communicating privately with a teammate → use a DM, not an issue comment.
- Anything sensitive → remember the tracker is visible to everyone with repo access.

## Verification

After posting, the comment appears on the issue page. `amos show "<node-name>"` will re-fetch
the issue and include the new comment in the resolved body.
