---
whoami: amos
name: "@github:tatolab/amos#13"
description: GitHub adapter — sync node status from GitHub issues via gh CLI
dependencies:
  - "up:@github:tatolab/amos#12"
---

First adapter implementation. Uses `gh` CLI (assumes installed and authed).

Node naming:
- `@github:tatolab/amos#15` — issue #15 in the current repo
- `@github:tatolab/openclaw#42` — cross-repo reference

Status mapping:
- Issue closed → done
- Issue open with "in-progress" label → in-progress
- Issue open → not started (clear status)

Sync: `gh issue list --json number,state,labels` in one API call, match against node names, write `.amos-status`.

No GitHub token management — `gh` handles auth. If `gh` isn't installed, `amos sync` prints an error and exits.
