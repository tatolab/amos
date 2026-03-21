---
name: amos-notify
description: >
  Send a message to a node's source system through its adapter.
disable-model-invocation: true
argument-hint: "<node> <message>"
allowed-tools: Bash(amos:*)
---

Run `amos notify <node> <message>` to send a freeform message
to the node's source system. The adapter decides how to deliver
it (e.g. GitHub issue comment, Slack message).
