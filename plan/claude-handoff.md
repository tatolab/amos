---
whoami: amos
name: claude-handoff
description: Pipe amos output directly into claude code as a system prompt
dependencies:
  - up:run-from-root
---

The end goal: `amos` builds the spec, then passes it to `claude` as context so you can start working immediately. Right now the spec goes to stdout and you have to copy-paste it.

Options to explore:
- `amos | claude -p` — simplest, pipe stdout into claude's prompt mode
- `amos --exec` — amos invokes `claude` itself with the spec as a system message
- `amos --print` — just print the spec (current behavior), for when you want to review before handing off

The default should be the most useful flow: scan, build spec, hand off to claude. The spec becomes claude's working context for the session.
