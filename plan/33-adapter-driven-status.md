---
whoami: amos
name: "@github:tatolab/amos#33"
description: Adapter-driven status — statuses come from external systems, not hardcoded enums
adapters:
  github: builtin
---

Status must come from adapters, not from a hardcoded four-value enum.
The current design has two problems:

1. `ManualStatus` only has `Done` and `InProgress` — but Jira has custom
   workflows per project/org, GitHub has labels, open/closed, and draft
   states. The type is too narrow to represent real-world status.

2. Adapter-resolved status is ignored by the DAG. `ResourceFields.status`
   is returned from `resolve()` but `compute_status()` only reads the
   `.amos-status` overlay file. The adapter's answer gets thrown away.

## What needs to change

### 1. Replace `ManualStatus` enum with `String`

**File:** `src/status.rs`

`ManualStatus { Done, InProgress }` → status is just a `String`.
The `.amos-status` file already uses symbols (`[x]` = done, `[~]` = in-progress).
Change it to store arbitrary strings: `- [closed] node-name`, `- [In Review] node-name`.
Keep `[x]` and `[~]` as shorthand aliases for backwards compat.

### 2. Replace `ResourceFields.status` type

**File:** `src/adapter.rs`

`status: Option<ManualStatus>` → `status: Option<String>`.
Adapters return the raw status string from the external system.

### 3. GitHub adapter returns real status strings

**File:** `src/gh_adapter.rs`

`IssueData::to_status()` currently maps `CLOSED → Done`, `OPEN + label:in-progress → InProgress`.
Instead, return the actual state: `"closed"`, `"open"`, or the label value.
Let the DAG consumer decide what "done" means — amos shouldn't hardcode
GitHub's semantics.

### 4. Feed adapter status into DAG

**File:** `src/dag.rs`

`compute_status()` currently only checks `status_overlay` (from `.amos-status`).
It needs a second status source: adapter-resolved status.
Priority: `.amos-status` override > adapter-resolved > computed from deps.

`ComputedStatus` enum needs rethinking — it's the output of `compute_status()`
but the possible values aren't a fixed set anymore. Options:
- Return `String` directly (simple, adapters set the vocabulary)
- Return an enum with a `Custom(String)` variant for adapter statuses
- Keep `Ready` and `Blocked` as computed states, but `Done`/`InProgress`
  become adapter-provided strings

The readiness logic (`Ready` vs `Blocked`) is still internal to amos —
it depends on whether upstream deps are "done". But what counts as "done"
needs to be configurable per adapter or per project. A GitHub `closed`
issue is done. A Jira ticket in `Deployed` is done. This mapping belongs
in the node's frontmatter or adapter config, not in Rust code.

### 5. External adapter protocol

**File:** `src/external_adapter.rs`

External adapters return JSON from `resolve()`. The `status` field in
that JSON should be passed through as-is (it's already a string in JSON).
Currently it gets mapped through `ManualStatus` — remove that mapping.

### 6. Output formatting

**File:** `src/output.rs`

`format_dag()` calls `compute_status()` and formats as `[done]`, `[ready]`, etc.
With string-based status, it should display whatever the adapter returned:
`[closed]`, `[In Review]`, `[Deployed]`, `[ready]`, `[blocked]`.

## What stays the same

- `.amos-status` file still works as a manual override
- `Ready` and `Blocked` are still computed by the DAG (not from adapters)
- The adapter trait interface stays the same shape
- Dependency edges and DAG structure unchanged
