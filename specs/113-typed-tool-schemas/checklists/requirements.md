# Specification Quality Checklist: Typed input schemas for every MCP tool

**Purpose**: Validate specification completeness and quality before proceeding to planning
**Created**: 2026-09-06
**Feature**: [spec.md](../spec.md)

## Content Quality

- [x] No implementation details (languages, frameworks, APIs)
- [x] Focused on user value and business needs
- [x] Written for non-technical stakeholders
- [x] All mandatory sections completed

## Requirement Completeness

- [x] No [NEEDS CLARIFICATION] markers remain
- [x] Requirements are testable and unambiguous
- [x] Success criteria are measurable
- [x] Success criteria are technology-agnostic (no implementation details)
- [x] All acceptance scenarios are defined
- [x] Edge cases are identified
- [x] Scope is clearly bounded
- [x] Dependencies and assumptions identified

## Feature Readiness

- [x] All functional requirements have clear acceptance criteria
- [x] User scenarios cover primary flows
- [x] Feature meets measurable outcomes defined in Success Criteria
- [x] No implementation details leak into specification

## Notes

All items pass. Two rounds of changes were made during validation.

**Round 1 — Content Quality:**

- FR-013 originally specified the verification mechanism ("starting the server as a
  subprocess"). Rewritten to state the requirement — verify against the listing a running
  server emits, not a definition inspected in isolation — leaving the mechanism to the plan.
- Rust type and macro names were kept out of the spec entirely. Tool names (`iris_admin`,
  `stream_inspect`) are retained because they are the product's own user-facing vocabulary,
  not implementation detail.

**Round 2 — resolved the one open clarification (FR-015).** Unknown parameters are rejected
with the offending name and the accepted names, not silently discarded. This is the feature's
only intended behavior change, so it produced follow-on edits rather than a one-line fix:

- FR-015 rewritten; FR-016 (reject before any work runs, so a destructive tool cannot
  half-apply) and FR-017 (distinguish unknown-name from disallowed-value) added.
- Acceptance scenario 5 added to User Story 1; SC-008 added.
- Two edge cases added: the extra parameter that used to be tolerated, and rejection on a
  destructive tool.
- The "behavior is preserved" assumption now names FR-015 as its single exception, so it no
  longer contradicts the requirements.

**On "non-technical stakeholders":** the stakeholder for this feature is a developer or an
agent integrating against the tool surface. The spec is written for that reader and stays
clear of the codebase's internal vocabulary; it does not attempt to avoid the domain
vocabulary of tools and parameters, which would make it unreadable.

**Terminology note:** the spec deliberately says "advertise" and "parameter contract" rather
than naming the schema mechanism, so the requirements survive a change of transport or
serialization approach.

Ready for `/speckit.plan`. `/speckit.clarify` is not needed — the one ambiguity worth asking
about was resolved in session 2026-09-06.
