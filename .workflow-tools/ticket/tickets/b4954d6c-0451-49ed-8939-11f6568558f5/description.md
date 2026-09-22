## Motivation
The highest-fidelity feedback signal is an explicit feedback-ingestion tool call the agent already made during a session. It carries an unambiguous target entity + rating + note in structured arguments — no inference required.

## Scope
- Add an `ExplicitIngestion` variant to the structured signal model in `crates/session-api/src/transcript_feedback.rs`.
- Ground the exact captured tool-name schema against the real `feedback-mcp` / `feedback-cli` / `feedback-http` definitions — DO NOT guess the tool name. Confirm how the Copilot capture pipeline records the tool name in `SessionTurn.tool_name` / `event_meta`.
- Parse `event_meta.tool_arguments_json` for target `EntityUrn` + `FeedbackRating` + note + `FeedbackNoteKind`.
- Record a backtraceable `FeedbackEntry` (source = Agent or User) via `EntityFeedbackStore`, with provenance carrying session/turn/tool-call refs.

## Non-goals
- No free-text / keyword parsing of message content.
- No auto-ticket creation here (that is the follow-up-format ticket).

## Dependencies
- Requires the extended `FeedbackProvenance` backtrace refs.

## Acceptance criteria
- Unit test: a synthetic tool turn with feedback-ingest args produces exactly one recorded `FeedbackEntry` with correct target/rating and populated provenance.
- No code path inspects `SessionTurn.content` text.