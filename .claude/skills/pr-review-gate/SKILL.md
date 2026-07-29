---
name: pr-review-gate
description: >
  Run an independent code review against the current branch before opening a
  PR and return a structured verdict (PASS / FIX / DISCUSS) that decides
  whether to auto-open the PR. Replaces the "open PR? y/n" question with an
  agent-made decision on the narrow open-PR gate only — follow-ups, merge
  decisions, and broader discussion still stay with the user. Invoke
  automatically in every PR-opening workflow (amos-next, etc.) just before
  the `gh pr create` step. Can also be invoked manually via
  `/pr-review-gate` for ad-hoc reviews before a push.
allowed-tools: Bash, Read, Grep, Glob, Agent
---

# PR review gate

This skill spawns an independent code-review subagent that **flags
concerns for the calling agent to investigate**. The reviewer never
makes decisions or changes things — it surfaces items the calling
agent (you) must independently verify against the actual code before
deciding what to do.

**The session agent — not the reviewer — owns the final verdict.**
Treat the reviewer's output as a checklist of things to look at, not
a binding decision. After the reviewer returns, you read the actual
code, verify each flagged concern, decide what (if anything) to
change, and form your own final judgment. You should leave this
process with high confidence that the body of work is correct, that
you have a complete understanding of the issue and current code, and
that any decisions are deliberate.

The skill replaces the "open PR? y/n" question with a structured
flag-and-verify loop, but the verification work is yours.

## When this fires

**Automatically:** Any workflow that would otherwise ask the user "open PR?
[y/n]" should invoke this skill first and use the verdict to decide.
`amos-next` has this wired in; other PR-opening workflows should too.

**Manually:** `/pr-review-gate` runs the same review on the current branch's
diff vs. the base, even when no PR is queued. Useful before a push to catch
things early.

## What the reviewer checks

The subagent prompt (Step 2 below) tells the reviewer to look for a specific
set of common failure modes. Don't edit the prompt lightly — each check was
added because a real PR was almost shipped without it:

1. **Test lock-in.** For every test added alongside a fix, does the test
   actually exercise the bug? Mentally revert the fix — does the test still
   pass? If yes, it's a feel-good test, flag it. (See
   `feedback_regression_tests_must_exercise_bug.md`.)
2. **Layering / reach-through.** Does any new example or utility reach
   through `_lib` / `_handle_ptr` / `_internal_*` / similar private
   attributes? If yes, there's likely a missing public accessor. (See
   `feedback_examples_no_underscore_reachthrough.md`.)
3. **Scope discipline.** Does the PR touch files outside the task's stated
   scope? If yes, is there a clear reason (ripple fix from the same root
   cause), or is it drift?
4. **RHI / boundary violations.** For Rust changes: does anything outside
   `vulkan/rhi/` import `ash` or `vulkanalia` directly? Does any processor
   code allocate GPU memory or create Vulkan objects outside the RHI? (See
   CLAUDE.md "Vulkan RHI Boundary — ABSOLUTE RULE.")
5. **Naming.** Do new public names follow the explicit-naming rule
   (`LinkOutputDataWriter`, not `Writer`)? Flag shortening.
6. **Polyglot sync.** For `polyglot`-labeled work: are Python AND Deno
   both covered, or is one deferred with a tracked follow-up? (See
   `feedback_milestones_issues_vocabulary.md`,
   `.claude/workflows/polyglot.md`.)
7. **Docs.** If the change adds public API, does it have the minimal
   autocomplete-useful docstring/doc-comment, and nothing more?
8. **Anything else that would embarrass the PR in six months.**

## Verdicts (advisory, not binding)

The reviewer returns one of three labels — these are the reviewer's
**recommendation**, not the final call. The calling agent (you)
independently verifies each flagged item before acting.

- **PASS** — reviewer didn't find concerns. You still spot-check the
  load-bearing claims (issue-body fidelity, behavior preservation,
  zero-duplication exit criterion if applicable) against the actual
  code before opening the PR.
- **FIX** — reviewer flagged specific items it thinks are
  clearly-actionable bugs. **Do not auto-apply these.** Read each
  flagged location in the actual code, decide whether it's a real
  issue, a false positive, or a stylistic note. Make any actual
  changes deliberately, with your own judgment.
- **DISCUSS** — reviewer flagged an ambiguous trade-off. Read the
  flagged items, form your own view, and decide whether to surface
  to the user (if it's a genuine architecture call) or address it
  yourself (if your reading of the code resolves the ambiguity).

The reviewer must NOT modify code, edit files, or run mutating
commands. It only reads and reports. The skill prompt enforces this
explicitly.

## Protocol

### Step 1 — Gather the diff

The reviewer needs the full diff and the issue/task context. Assemble:

```bash
BASE_BRANCH="main"  # or whatever the target is
BRANCH=$(git branch --show-current)
git fetch origin "$BASE_BRANCH" >/dev/null 2>&1

# Full diff vs. base
git diff "origin/${BASE_BRANCH}...HEAD" > /tmp/pr-review-diff.txt

# Commit list for context
git log --oneline "origin/${BASE_BRANCH}..HEAD" > /tmp/pr-review-commits.txt

# Changed files
git diff --name-only "origin/${BASE_BRANCH}...HEAD" > /tmp/pr-review-files.txt
```

Tell the user in one line what you're doing: "Running pr-review-gate on the
current branch."

### Step 2 — Spawn the reviewer

Invoke a `general-purpose` subagent **with `model: "opus"`** — code
review involves nuanced trade-off reasoning across the diff, the issue
body, and CLAUDE.md rules. Sonnet applies rules mechanically and
generates low-value findings (flagging names that came verbatim from
the issue body, missing real architectural drift). Opus is mandatory
for this step. Do not omit the `model` parameter.

**Pass the full issue body, verbatim**, to the reviewer alongside the
diff. The reviewer must know which decisions came from the user
(issue body, AI Agent Notes) and which came from the implementer, so
it doesn't second-guess names / shapes the user already chose. If the
calling context doesn't have the body in hand, fetch it via
`gh issue view <N> --json body -q .body > /tmp/pr-review-issue.txt`.

Prompt shape (fill the bracketed bits from the calling context):

```
Independent code review of branch <BRANCH> vs. <BASE_BRANCH> in <repo>.

Commits:
<paste /tmp/pr-review-commits.txt>

Issue body (user-authored — names and shapes here were chosen by the
user, not the implementer; do NOT flag them as naming concerns):
<paste /tmp/pr-review-issue.txt, or the issue body the caller has>

Scope context: <one-paragraph summary of what the PR is supposed to
do. Include the issue number.>

Full diff is at /tmp/pr-review-diff.txt — read it with the Read tool.
Changed files are at /tmp/pr-review-files.txt.

Use max effort. Read every changed file end-to-end if the diff hides
context (collapsed regions, renamed-without-shown-old-content, etc.).
This is a high-value gate — surface real concerns, skip the cosmetic
ones.

**You are a flagging tool, not a decision-maker.** Do NOT modify any
files, do NOT run mutating git commands, do NOT propose to "auto-fix"
anything. Your output goes back to a session agent who will read the
actual code, verify each concern you raise, decide what to do, and
make any code changes. If you flag something, you are saying "the
session agent should look at this" — not "this needs to change."
Avoid prescribing fixes; describe the concern and the location, and
let the session agent reach its own conclusion. Skip nitpicks the
session agent will obviously dismiss; surface the genuinely
worth-investigating items.

Check the following (this list is non-negotiable; check each one even
if it doesn't seem to apply):

1. Tests: for every test added alongside a fix, mentally revert the
   fix and ask whether the test would still pass. If yes, flag it —
   it's a feel-good test.
2. Layering: does any new example or utility reach through private
   underscore-prefixed attributes (`_lib`, `_handle_ptr`, etc.)? If
   yes, flag it — there's probably a missing public accessor.
3. Scope: does the PR touch files outside the stated scope? If yes,
   is the out-of-scope touch justified by the same root cause?
4. Rust RHI boundary (if Rust files are touched): anything outside
   `vulkan/rhi/` importing `ash` / `vulkanalia`, or allocating GPU
   memory / creating Vulkan objects outside the RHI? Flag if yes.
5. Naming: do new public names follow the explicit-naming convention
   in CLAUDE.md? Flag shortening like `Writer`, `Handler`, `Manager`
   ONLY when the name was chosen by the implementer. Names that
   match the issue body verbatim are user-chosen — do NOT flag them.
6. Polyglot sync (if `polyglot` is in the labels/scope): are Python
   AND Deno both covered, or does one lag without a tracked follow-up?
7. Docs on new public API — minimal autocomplete-useful only, not
   verbose.
8. Issue-body fidelity: does the diff implement every load-bearing
   item from the issue body's Description / Exit criteria / AI Agent
   Notes? Anything cut without justification is a FIX. Items
   deferred with explicit "out of scope" rationale in the PR body
   are fine.
9. Anything that would embarrass the PR in six months.

Return your verdict in this exact format, nothing else:

VERDICT: <PASS | FIX | DISCUSS>

ISSUES:
- [FIX|DISCUSS] <one-line summary with file:line where useful>
- ...
(Use "- none" if no issues.)

RATIONALE: <one paragraph explaining the verdict in plain English.
If PASS, one sentence is fine. If FIX, describe what needs fixing.
If DISCUSS, describe the trade-off for the human to weigh.>

Do not add preamble, headers, or a "summary" section. The caller
parses the three-section format verbatim. If you have no issues to
report, use "- none" in the ISSUES section and keep going — do not
omit the section or rename it.

Be direct. Don't manufacture concerns to seem thorough. Don't pull
punches on real ones. Report length: under 700 words total — Opus
can afford the extra room for genuine nuance.
```

Set `description: "PR review gate: verdict on <BRANCH>"` and
`model: "opus"` for the Agent call. Do **not** use
`run_in_background: true` — the caller must wait on the verdict to
decide the next step.

### Step 3 — Parse the verdict

The subagent's result is markdown text. Parse:

- `VERDICT:` line → `PASS` / `FIX` / `DISCUSS`. Anything else → treat
  as DISCUSS (reviewer misbehaved; don't auto-open).
- `ISSUES:` lines → list of items, each tagged `[FIX]` or `[DISCUSS]`.
- `RATIONALE:` paragraph → surface back to the caller / user.

### Step 4 — Independently verify each flagged item

This is the load-bearing step. The reviewer flagged things; **you
verify each one against the actual code before doing anything**.

For each item in the `ISSUES:` list:

1. **Read the relevant code.** Open the file the reviewer named (or
   the area its concern points to) with the Read tool. Verify the
   reviewer's claim is true at face value.
2. **Decide on the merits.** Is it a real bug? A false positive
   (reviewer misread the code)? A stylistic note that doesn't
   warrant action? A genuine architectural concern that needs the
   user to weigh? Form your own opinion.
3. **Cross-check against the issue body.** If the reviewer flagged a
   name or shape, confirm whether it came from the issue body
   (user-chosen, do not change) or from the implementer (your
   call).
4. **Take action.** If a real concern in this PR's scope, fix it
   yourself with Edit / Write — don't delegate, your judgment is
   what's load-bearing. If a false positive, note in the report why
   you disagreed. If architectural, surface to the user with your
   own framing of the trade-off.

**A confirmed finding that's out of this PR's scope is NOT
automatically an issue to file.** Run the priority gate in
`amos-file` ("Priority gate — only P0 files without being asked").
File it only when the focused milestone's design or functionality
does not land without it. Otherwise write it into the PR body /
report with its real cost and your recommended action, and let the
user decide. A reviewer flagging something is not the user asking
for a ticket — the reviewer is a tool you invoked, so anything it
surfaces is agent-initiated by definition. Verifying a finding is
real settles whether it's true, not whether it's worth tracking.

After verification, also independently spot-check the load-bearing
claims even if the verdict was PASS:

- Issue-body fidelity — every load-bearing item in
  Description / Exit criteria / AI Agent Notes implemented?
- Behavior preservation in non-trivial migrations — did
  multi-stage lock patterns, error paths, cleanup-on-failure paths
  survive the refactor with the same shape?
- Test quality — do the new tests genuinely lock in behavior
  (mentally revert the impl and check)?

**Outcome:**

- All your independent checks clear → tell the user "Review
  complete, no concerns" + a one-line summary of what you verified;
  proceed to the caller's `gh pr create` step.
- You found genuine issues during verification → fix them, re-run
  the test gate, commit, and re-invoke this skill (cap at 3
  iterations).
- You found a genuine architectural call → surface to the user
  with your own framing: "Reviewer flagged X, I read the code and
  agree it's a trade-off because Y. Want me to do A or B?" Do NOT
  auto-open in this case.

The reviewer's verdict is advisory. Your independent verification
is what actually gates the PR.

### Step 5 — Cleanup

Delete the scratch files: `rm -f /tmp/pr-review-diff.txt
/tmp/pr-review-commits.txt /tmp/pr-review-files.txt
/tmp/pr-review-issue.txt`. They contain the full diff + issue body
and grow big on large PRs.

## Hard rules

1. **Only the open-PR gate.** This skill does not decide whether to merge,
   whether to rebase, whether to rename a branch, etc. Merge decisions
   stay with the user unless they explicitly say otherwise. If the PR is
   already open and the reviewer returns FIX or DISCUSS, the existing PR
   stays open — fixes go into new commits on the same branch, not a new
   PR.
2. **Don't loop forever.** Cap FIX iterations at 3. On the fourth
   iteration, unconditionally escalate to the user as DISCUSS.
3. **Don't suppress concerns.** If the reviewer says DISCUSS, surface
   the rationale verbatim — don't summarize away nuance the reviewer
   thought worth flagging.
4. **Don't shadow real CI.** This is a code-review gate, not a test
   runner. The caller is still responsible for running the test gate
   (workspace baseline, E2E scenarios, etc.) before invoking this skill.
   If the reviewer flags a "you didn't run tests" item as FIX, that's
   a caller bug — the test gate should have run already.
5. **The "no human involvement" of PASS means no 'open PR? y/n'.**
   You still print the PR URL and a short summary after `gh pr create`
   succeeds; you just don't pause to ask for permission to open.
6. **Don't spawn tickets from review output.** The autonomy this skill
   grants is over the open-PR decision only — it does not extend to
   filing issues. Out-of-scope findings go through the P0 priority
   gate in `amos-file`; sub-P0 ones are written up with a recommended
   action and wait for the user.

## Cost / cadence note

Spinning up a subagent for every PR-open is ~1 minute and some tokens.
For workflows that open many small PRs in a row (rare), batch is not an
option — each PR gets its own review because the diff is what's being
reviewed. If the user asks to skip the gate (e.g. "just open the PR,
I'm in a hurry"), do it and note in the PR body that the pre-open
review was bypassed.
