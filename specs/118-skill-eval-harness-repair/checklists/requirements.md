# Specification Quality Checklist: Repair the skill-eval regression harness

**Purpose**: Validate specification completeness and quality before proceeding to planning
**Created**: 2026-09-08
**Feature**: [spec.md](../spec.md)

## Content Quality

- [x] CHK001 No implementation details (languages, frameworks, APIs)
- [x] CHK002 Focused on user value and business needs
- [x] CHK003 Written for non-technical stakeholders
- [x] CHK004 All mandatory sections completed

## Requirement Completeness

- [x] CHK005 No `[NEEDS CLARIFICATION]` markers remain
- [x] CHK006 Requirements are testable and unambiguous
- [x] CHK007 Success criteria are measurable
- [x] CHK008 Success criteria are technology-agnostic (no implementation details)
- [x] CHK009 All acceptance scenarios are defined
- [x] CHK010 Edge cases are identified
- [x] CHK011 Scope is clearly bounded
- [x] CHK012 Dependencies and assumptions identified

## Feature Readiness

- [x] CHK013 All functional requirements have clear acceptance criteria
- [x] CHK014 User scenarios cover primary flows
- [x] CHK015 Feature meets measurable outcomes defined in Success Criteria
- [x] CHK016 No implementation leakage

## Validation Notes

Two passes were needed. Findings from the first pass and what changed:

- **CHK001 / CHK016 failed initially.** The requirements named the harness's own function and
  module identifiers, which is exactly what constitution clarification prompt 2 forbids in
  `spec.md`. Rewritten to describe behaviour: "the scorer", "the baseline entry", "the aggregate
  gate". File paths survive in the Dependencies section only, to bound the blast radius, and no
  function signature, parameter name, or module path appears anywhere in the document.
- **CHK007 failed initially.** SC-004 read "the gate covers more skills", which is not a number.
  It now states one of nine rising to at least six of nine. SC-010 was added because the cost
  argument needed an outcome, not just a table.
- **CHK012 failed initially.** The scorer diagnosis was written as fact, on the strength of a
  perfect correlation plus a missing credential. It was demoted to an assumption, and then the
  clarification session read the scoring code and confirmed it: the failure handler returns a
  score of zero, the client needs Bedrock or an API key, and the nightly supplies neither. The
  assumption is now the code evidence.
- **CHK003 partial and accepted.** Statistical power cannot be specified without stating a
  minimum detectable effect, a significance level, and an item count. Those three numbers are
  the requirement. The surrounding prose explains what they buy in plain terms ("three items
  versus one", "a coin flip"), which is as far as this can be pushed without dropping the
  requirement.
- **CHK011 checked against the brief's exclusions.** Skill-content edits, GEPA work, and new
  skills are all named in Out of Scope. Replacing a benchmark task is explicitly in scope, since
  Story 3 cannot conclude without that option.

Settled in the 2026-09-08 clarification session, not a checklist failure: constitution
clarification prompt 3 caps a spec at 40 tasks, and twenty-two functional requirements across four
stories would have crowded it. 118 now ships Stories 1 and 2 (make the measurement real, make the
gate cover everything). Stories 3 and 4 (retire the null task sets, size the measurement) stay
written out in the spec and ship as a follow-on, because they depend on Story 1's output and
nothing depends on them. The deferred requirements and success criteria are marked as such in
`spec.md`.
