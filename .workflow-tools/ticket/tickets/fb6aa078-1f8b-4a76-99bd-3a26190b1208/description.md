## Goal
Define and enforce feedback-api crate boundaries, a single versioned FeedbackEntry schema, AND the correct topology of the feedback ring: the ring is NOT a module — it is an emergent distributed loop where every domain writes feedback into the feedback-api hub and reacts to outcomes.

## Decision (grounded 2026-07-12 review)
`rule-api::ring` is architecturally wrong and must be deleted as a module. rule-api (a rules crate) currently depends on spec-api + test-api + session-api + ticket-api solely to host ring edges that are not rule concerns. This is a dependency-magnet god-module. Redistribute each edge to its owning domain; feedback-api is the shared hub every edge writes into.

## Edge ownership (redistribution target)
- execution→spec `verified` recompute (`recompute_spec_verified_state`): move to **spec-api** (spec×test seam). Emits `FeedbackSource::System` entry via feedback-api. Zero rule content today.
- transcript mining (`mine_transcript_for_rule_confusion`): move to **session-api / transcript analyzer**. Emits `TranscriptMined` feedback. Replace `contains("rule violation")` string heuristics.
- missing-rule auto-ticketing (`handle_missing_rule_match`): rule-api owns ONLY the "did any rule match?" signal (it currently receives `has_matching_rule: bool` as a parameter and does no matching). Ticket creation + feedback emission is orchestration over ticket-api + feedback-api, not rule-api.
- frontend feedback ingest (`ingest_frontend_feedback`): move into **feedback-api** itself — it is a pure `store.ingest_rating` path.

## Scope
- crates/feedback-api is the first-class store-backed hub (DONE: crate exists, schema, store, transports build; feedback-api 12 / rule-api 70 tests pass).
- Delete crates/rule-api/src/ring.rs; redistribute its edges per ownership table.
- After redistribution, rule-api Cargo.toml drops spec-api, test-api, session-api (keep feedback-api; keep ticket-api only if the no-match seam stays).
- FeedbackEntry v1: id, schema_version, source, target(EntityUrn), rating/note/note_kind, provenance, status(new|triaged|actioned|dismissed) — DONE in feedback-api.
- Enforce dependency direction: domains -> feedback-api only; feedback-api depends on none of them (no cycle).

## Acceptance
- crates/feedback-api builds and hosts schema + store + domain-agnostic edges.
- No `ring` module exists in rule-api; edges live in spec-api / session-api / feedback-api; rule-api retains only the rule-match signal.
- rule-api Cargo.toml no longer depends on spec-api/test-api/session-api.
- feedback-api round-trip + per-domain edge tests pass; workspace builds with no dependency cycle.
- feedback-api contract-v2 spec documents the ring as an architectural property, not a module.