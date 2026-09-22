## Motivation
`mine_structured_feedback_signals` now detects `FailedToolCall` signals (`event_meta.tool_success == Some(false)`), but a failed tool call has no inherent target entity. We must decide, with evidence, what (if anything) it maps to before recording feedback or synthesizing tickets from it.

## Scope
- Investigate the triggers of the 522 previously auto-mined (now bulk-deleted) tickets using the committed `.session/sessions/*` transcripts as evidence — understand what patterns fired and whether any findings are genuinely useful.
- Decide the mapping for a failed tool call: tool identity, an argument-referenced entity (ticket/spec/rule id extracted from `tool_arguments_json`), or no entity (signal-only, not feedback).
- Produce a typed representation + documented policy. Encode error/ambiguity states in the type system (no silent fallbacks).

## Non-goals
- No message-text heuristics.

## Acceptance criteria
- A written policy (in the ticket/spec) for failed-tool-call handling.
- Typed model + unit test covering the confident cases and an explicit "unmapped" outcome.

## Investigation & Decision (2026-07-13)

**Critical grounding finding**: real captured `.session/sessions/*` transcripts have **zero** `SessionTurn`s with `role: tool` (checked all 61 committed sessions, 2670 total turns, 0 tool-role). Tool call/result metadata (`tool_name`, `tool_call_id`, `tool_success`, `tool_arguments_json`) is recorded only on the separate `CopilotHookEvent` list (`events.json`), specifically the normalized `tool.execution_result` event. This means the already-landed turn-based `FailedToolCall` detector (`mine_structured_feedback_signals`) can never fire against real data — it was validated only against synthetic fixtures. Added a new event-based miner (`mine_failed_tool_call_signals`) that reads the events list instead; the turn-based miner is kept for forward compatibility but is not what the stop-hook relies on for real sessions.

**Failure distribution** (115 failed `tool.execution_result` events out of 5,031 captured, across 61 sessions):
- ~46% generic file/dev tools (`read_file` 48, `apply_patch`/`create_file`/`grep_search`/`list_dir`/`run_in_terminal` a few each) — reference a filesystem path, not an entity in any `feedback-api` `EntityUrn` store (only `rule`/`spec`/`ticket` exist, confirmed by grepping every `EntityUrn::new`/`::rule`/`::spec`/`::ticket` call site in the repo).
- `test-mcp`'s `test_record_execution` (8) — its `validation_spec_id` has no corresponding `EntityUrn` store either.
- `ticket-mcp` methods keyed on an existing ticket id (`board_check_out` 23, `get_ticket` 6, `update_ticket` 4, `get_ticket_description` 3, `board_check_in` 2, plus board file/rename ops) — unambiguously reference one ticket.
- `create_ticket` (7) — creates a *new* entity, so there is no existing-entity id to map to.
- `add_edge`/`remove_edge` (1) — reference two candidate tickets (`from`/`to`) with no principled way to prefer one.

**Decision (policy)**: map a failed call to an entity only when the tool is a known entity-domain method with a present single-entity id argument (today: `ticket-mcp`, `ticket_id`- or `id`-keyed per method). Every other case returns an explicit, typed `UnmappedReason` (`UnknownTool` / `NoEntityIdArgument` / `AmbiguousMultipleCandidates` / `NoSupportedEntityStore`) rather than a guess or silent fallback.

**Implementation**: `FailedToolCallMapping`, `UnmappedReason`, `map_failed_tool_call_to_entity`, and `mine_failed_tool_call_signals` in [crates/session-api/src/transcript_feedback.rs](../../../crates/session-api/src/transcript_feedback.rs). 9 new unit tests (7 mapping-policy cases + 2 event-mining integration tests), all passing. Validation: `vt-feedback-failed-tool-call-mapping` / `exec-vt-feedback-failed-tool-call-mapping-20260713`.