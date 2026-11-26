# Specification Quality Checklist: `propose` reads index globals, collations and datatypes

**Purpose**: Validate specification completeness and quality before proceeding to planning
**Created**: 2026-11-26
**Feature**: [spec.md](../spec.md)

## Content Quality

- [x] No implementation details (languages, frameworks, APIs) beyond the tool contract this repository's specs carry
      (spec 172 does the same: the tool, its actions and its response keys are the user-facing surface)
- [x] Focused on user value and business needs
- [x] Written for non-technical stakeholders (the developer who owns a global; Context explains the trap in plain terms)
- [x] All mandatory sections completed

## Requirement Completeness

- [x] No [NEEDS CLARIFICATION] markers remain
- [x] Requirements are testable and unambiguous
- [x] Success criteria are measurable
- [x] Success criteria are technology-agnostic
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

- Open points for `/speckit.clarify`: six unmeasured behaviours listed in Assumptions (hand-written bitmap map, row ID
  from a node value, `Condition` on an index map, `$$COLLATE` argument forms, privileges to list globals, multi-part
  key drafts); what happens to the whole spec if one of them fails to measure.
