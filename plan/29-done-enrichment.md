---
whoami: amos
name: "@github:tatolab/amos#29"
description: Completion context — PRs, commits, deployments for done nodes
adapters:
  github: builtin
---

@github:tatolab/amos#29

Extend the gh adapter's resolve for closed issues to also fetch linked PRs via GitHub API. Use `gh api` to query the timeline events for the issue and find linked pull requests.
