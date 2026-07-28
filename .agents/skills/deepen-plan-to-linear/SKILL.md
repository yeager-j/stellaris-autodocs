---
name: deepen-plan-to-linear
description: Deepen one phase of docs/implementation-plan.md into sequenced, agent-ready Linear tickets for the Autodocs MVP project. Use when planning, drafting, reviewing, or filing Phase 0-12 work in Linear, including assigning the correct phase milestone, existing labels, T-shirt size, dependencies, acceptance criteria, and phase exit criteria.
---

# Deepen Plan to Linear

Turn one implementation phase into a coherent ticket sequence without copying the phase plan into
Linear. Keep the plan and technical design authoritative; make each ticket a concise review and
delivery boundary that a new agent can pick up.

## Establish the source of truth

1. Read `AGENTS.md`, the entire requested phase in `docs/implementation-plan.md`, its global
   constraints, sequencing rationale, ticketing guidance, cross-cutting workstreams, and open
   decisions.
2. Read `docs/technical-design.md` completely. Re-read the sections named or implied by every task in
   the phase. Follow links to ADRs, evaluations, acceptance cases, decision-log entries, or existing
   code when they define evidence or a contract the ticket must preserve.
3. Inspect the current implementation and existing Linear issues for the phase. Treat implemented
   work, current code, and accepted decisions as evidence; do not create duplicate tickets from stale
   outline text.
4. Inspect the **Autodocs MVP** Linear project, its description, the requested `Phase 0` through
   `Phase 12` milestone, existing issues, issue relations, estimate configuration, and current label
   vocabulary. Use only labels and T-shirt estimates that are valid in Linear; never invent a label
   or map a size from memory.
5. If no phase was named, identify the next unimplemented phase from the plan and ask the user to
   confirm it before drafting.

## Use the issue-writing workflow

Invoke `$to-issues` and follow its gather, draft, user-review, and publish workflow. Apply its
tracer-bullet rule through all layers that actually belong to the selected phase; do not pull UI,
transport, packaging, or other later-phase work forward merely to make a nominal vertical slice.
Each ticket must still produce a complete, independently verifiable result at the narrowest public or
module interface that observes the phase contract.

Draft the complete breakdown before creating anything in Linear. Present each proposed ticket with:

- title;
- plan tasks and user stories covered;
- blocking tickets;
- proposed T-shirt size;
- complexity or uncertainty that affects the split;
- the evidence or behavior available for review when it is complete.

Ask the `$to-issues` granularity and dependency questions and wait for approval. Do not file an
unapproved breakdown.

## Design a sequence that hands off cleanly

- Begin with prefactoring only when current code makes the intended change unnecessarily difficult.
- Put contract, evidence, fixture, or decision tickets before dependents that consume their result.
- Keep dependency chains necessary and explicit. Avoid serializing tickets that can safely proceed in
  parallel.
- Make the output of each ticket the named input or established contract of the next. State the
  handoff in acceptance criteria when it would otherwise be implicit.
- Split at genuine review gates: a maintainer must be able to reject one ticket while accepting its
  neighbors.
- Align judgment-heavy tickets to one evidence-bearing decision or a small cohesive set of policy
  rows. Group mechanical work when one review can safely assess it.
- Merge adjacent tasks that would each be smaller than `S`.
- Split work larger than `XL`.
- Do not leave a ticket both large (`L` or `XL`) and highly complex or uncertain. Split challenging
  work into `S` or `M` contracts, evidence slices, or supported domain cases before implementation
  fans out.
- Preserve phase entry conditions. Do not schedule a consumer before the plan's prerequisites are
  closed.
- Designate one final ticket for the phase. Prefer a natural integration or acceptance slice; create
  a focused phase-verification ticket only when no implementation slice can honestly prove the full
  exit.

## Write concise, traceable tickets

Use this body shape in addition to any required Linear workspace template:

```markdown
## What to build

<A brief end-to-end summary of the behavior, decision, or evidence this ticket delivers.>

## References

- Implementation Plan: `Phase N — …`, task(s) N[, N]
- Technical Design: `Section name`[, `Section name`]
- <Relevant ADR, evaluation, acceptance case, decision, or existing issue when needed>

## Acceptance criteria

- [ ] <Externally observable behavior or module contract>
- [ ] <Required failure, boundary, or unresolved-evidence behavior>
- [ ] <Focused executable verification and any required negative control>
- [ ] <Artifact or contract needed by the next ticket>

## Blocked by

- <Real Linear issue reference, or "None - can start immediately">
```

For the designated final ticket, append:

```markdown
## Phase exit criteria

- [ ] <Every condition from the phase's Exit paragraph, expressed as observable evidence>
- [ ] <Required golden-case, local-corpus, security, packaging, or cross-cutting evidence>
- [ ] All phase tickets are complete and their accepted outputs integrate through the phase's
      highest practical verification seam.
- [ ] No plan task is omitted, deferred, or left unresolved without a linked decision and explicit
      owner.
```

Keep summaries short and name technical-design sections precisely. Acceptance criteria must describe
outcomes, invariants, boundaries, and evidence—not a layer-by-layer implementation recipe. Avoid
whole implementation code blocks; include only a brief example when it communicates a decision more
precisely than prose.

Do not use acceptance criteria as a hidden backlog. Split criteria that establish independently
reviewable contracts or unrelated evidence. Conversely, do not create separate tickets for routine
tests, documentation, or wiring that belong to the same change.

## Size, label, and publish

1. Re-query Linear immediately before publishing so project, milestone, labels, and estimates are
   current.
2. Create tickets in dependency order in the **Autodocs MVP** project under the exact milestone for
   the selected phase.
3. Assign one valid T-shirt estimate from `S`, `M`, `L`, or `XL`. Never create a ticket estimated
   below `S` or above `XL`.
4. Apply only current, relevant Linear labels. Include the workspace's ready-for-agent/triage label
   when its documented meaning fits; do not guess from a similarly named label.
5. Add real Linear blocking relations after issue identifiers exist. Keep the body references
   readable, but treat Linear's relation field as authoritative.
6. Preserve approved ordering and scope while publishing. Do not close or modify unrelated or parent
   issues.

## Audit the filed phase with a subagent

After every approved ticket has been filed, use a fresh subagent for a read-only coverage audit. Give
it the raw phase plan, the technical design, and the created Linear issue identifiers and bodies.
Do not give it the intended coverage map or conclusions.

Ask the subagent to report:

- a matrix mapping every numbered phase task, entry condition, Exit condition, and applicable
  cross-cutting obligation to one or more tickets;
- missing, duplicated, contradictory, or prematurely scheduled scope;
- dependency gaps or cycles;
- tickets whose size and complexity violate this skill;
- incorrect project, milestone, estimate, or label assignments;
- acceptance criteria that do not let another agent determine success.

Compare the audit with the source documents yourself. Correct unambiguous omissions or metadata
errors within the approved breakdown, then repeat the audit. If the correction requires a material
new ticket, merge, split, or dependency change, return to the `$to-issues` user-review step before
changing Linear. Finish only when every plan point is covered exactly where intended and the final
ticket can determine whether the phase succeeded.
