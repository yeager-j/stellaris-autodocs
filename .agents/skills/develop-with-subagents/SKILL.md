---
name: develop-with-subagents
description: User-invoked development workflow in which a frontier coordinator writes the implementation plan, delegates repository exploration and implementation to smaller-model subagents, and personally reviews the completed changes. Use only when the user explicitly invokes `$develop-with-subagents`; never select this skill implicitly.
---

# Develop With Subagents

Keep the frontier model responsible for intent, design, and acceptance. Give execution to one
smaller implementation agent, then keep that same agent through every repair cycle so it retains the
codebase and implementation context it already earned.

## Non-Negotiable Roles

- Act as the frontier coordinator. Write the plan and make every design decision that changes scope,
  architecture, or acceptance.
- Use a smaller model for exploration and implementation. Select a model named by the user first;
  otherwise select the strongest available coding model below the coordinator's tier. Never invent a
  model identifier or silently delegate to a coordinator-equivalent model.
- Create one implementation agent and record its handle immediately. Resume that agent for every
  implementation follow-up and fault repair; do not replace it while it remains available.
- Review the finished work yourself. Never delegate final review or accept the implementation
  agent's self-review as the final judgment.
- Follow repository instructions, approval boundaries, and user scope at every role boundary. This
  skill changes who performs work, not what work is authorized.

If no smaller model or resumable subagent mechanism is available, explain the missing capability and
ask the user whether to continue without this workflow.

## Workflow

### 1. Establish the Contract

Before delegating:

1. Read the user request and repository instructions.
2. Inspect current branch, worktree state, and relevant issue or project context.
3. State the intended outcome, scope boundaries, and executable acceptance evidence.
4. Preserve unrelated user changes. Branch before editing when repository instructions require it.

Do not delegate an ambiguous contract. Resolve ambiguity from local evidence; ask the user only when
the answer changes product behavior, scope, or external authority.

### 2. Decide Whether to Delegate Exploration

Use this decision tree before writing the plan:

```text
Can the coordinator already name the implementation seam, governing source of truth,
affected callers, and verification command from current evidence?
├── Yes → Inspect the decision-driving files directly and write the plan.
└── No
    ├── Is the missing context discoverable read-only in the repository?
    │   ├── Yes → Launch one smaller-model explorer, then write the plan.
    │   └── No → Obtain the missing external context or ask the user.
```

Require the explorer to remain read-only and report:

- relevant paths and symbols;
- current control and data flow;
- repository instructions and invariants;
- tests and exact verification commands;
- conflicting evidence and unresolved questions.

Give the explorer the user request and repository location, not a proposed solution. This prevents
the coordinator's early hypothesis from shaping the evidence. After the report, inspect every source
whose contents materially determine the plan; the explorer maps the territory but does not own the
design.

### 3. Write the Implementation Plan

Write the plan before launching the implementation agent. Include:

- the user-visible and technical outcome;
- files or modules expected to change and why;
- ordered implementation steps;
- invariants and scope exclusions;
- tests, checks, and user-visible verification;
- decisions the implementation agent must return to the coordinator instead of making alone.

Make the plan specific enough that the implementation agent executes rather than redesigns. Update
the plan yourself when new evidence invalidates it; never outsource plan repair.

### 4. Delegate Implementation Once

Launch one smaller-model implementation agent. Give it:

- the implementation plan;
- repository path and branch;
- relevant exploration findings and source paths;
- ownership of the named files or responsibility;
- repository constraints and unrelated changes to preserve;
- exact verification commands;
- an instruction to stop and report if the plan conflicts with source evidence or requires broader
  scope.

Tell the agent it is not alone in the worktree and must not revert edits it did not create. Require it
to implement, run the assigned checks, inspect its diff, and report changed files, verification
results, and remaining concerns. Require it to commit its work on the feature branch before
finishing — never push or open a pull request. An uncommitted worktree is the only copy of the work,
and the review phase that follows deliberately mutates and restores files; a commit makes every
probe reversible and every accident recoverable.

Keep the agent handle. Do not launch a fresh implementation agent for convenience, token savings, or
a second opinion; continuity is the point of this workflow.

### 5. Review as the Frontier Coordinator

After the implementation agent finishes:

1. Inspect the complete diff and every changed file.
2. Compare the result against the user contract, repository instructions, and the written plan.
3. Hunt for correctness faults, missed edge cases, accidental scope, weak tests, and misleading
   names or comments.
4. Run the checks that observe each changed contract. Treat the agent's reported results as context,
   not proof.
5. If the changed behavior has an executable user interface in the current environment, verify it
   through that interface.

The coordinator diagnoses; the context-bearing implementation agent repairs. One narrow exception:
the coordinator may directly fix a change that is behavior-preserving and embeds no decision —
comment wording, doc typos, a misleading name, formatting — because the existing gates already
review such a fix and there is nothing to get subtly wrong. Any change to observable behavior,
types, or tests goes to the implementation agent regardless of size; one-line behavioral fixes are
exactly where wrong quick fixes hide, and routing them through the agent preserves the
failing-test-first discipline. Report every direct coordinator edit to the implementing agent at its
next resume, so its model of the code never silently diverges from the code.

### 6. Return Faults to the Same Agent

For every review cycle that finds a fault, resume the recorded implementation agent with:

- the concrete failure and its evidence;
- the expected behavior or invariant;
- the relevant path, test, or reproduction command;
- the instruction to preserve correct parts of the existing implementation;
- the checks it must rerun.

Require the agent to fix, verify, and report again. Then repeat the full frontier review. Continue
until no actionable fault remains or progress requires new user authority.

If the original agent becomes unavailable, report the lost context before substituting another
agent. Reconstruct the handoff from the plan, diff, test output, and prior reports; never pretend the
replacement retained the original context.

Codex automatically reviews opened pull requests; the harness notifies the coordinator of its
comments, so do not poll or listen for them. Treat each Codex finding like any other review lead:
confirm it against the source, investigate whether it is a real fault, and — when it is — handle it
under section 5's division of labor: behavior-preserving, decision-free fixes may be made directly
and reported to the implementing agent; everything else goes to that agent through this section's
fault-return loop.

### 7. Close the Loop

Report:

- the implemented outcome;
- the frontier review result;
- verification commands and outcomes;
- assumptions and deliberately deferred concerns;
- any context loss or deviation from the planned delegation model.

Do not claim completion until the frontier review and its verification pass.
