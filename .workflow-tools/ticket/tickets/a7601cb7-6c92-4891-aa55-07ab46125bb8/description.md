## Motivation
`FeedbackProvenance` currently carries only `session_id`, `author`, `executed_at` (crates/feedback-api/src/lib.rs). The removed transcript miner even dropped `session_id` entirely, so mined entries were not backtraceable to their origin. Backtraceability is a hard requirement for the robust feedback model.

## Scope
- Add optional `turn_sequence: Option<usize>` and `tool_call_id: Option<String>` to `FeedbackProvenance` (and confirm `session_id` is always populated for mined/ingested entries).
- Bump `FEEDBACK_SCHEMA_VERSION` if the on-disk shape changes; keep deserialization backward compatible.
- Provide a constructor path that threads these refs from a captured `SessionTurn`.

## Non-goals
- No message-text analysis. Structured metadata only.

## Acceptance criteria
- serde round-trip test for the extended provenance.
- A mined/ingested entry asserts populated `session_id` + `turn_sequence` + `tool_call_id`.

## Foundational for
- Explicit feedback-ingestion mining.
- Re-enabling backtraceable follow-up ticket synthesis.