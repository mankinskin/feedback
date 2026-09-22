## Motivation
The stop-hook (`crates/session-api/src/bin/copilot-capture-hook.rs`) currently only logs structured signals — auto-ticket synthesis was paused after the naive miner created 522 false-positive tickets (bulk-deleted). Before re-enabling, we must define a follow-up ticket format that is backtraceable, forward-actionable, and verifiable.

## Scope
- Define the follow-up ticket template so feedback-derived tickets are:
  - **backtraceable** — link the originating session/turn/tool-call and the `FeedbackEntry`;
  - **forward-actionable** — explicit next step + owning component;
  - **verifiable** — linked validation spec / acceptance criteria.
- Re-enable synthesis in the stop-hook, gated on confident structured signals only (explicit ingestion and/or the agreed failed-tool policy).
- Replace title-string-equality dedupe with robust idempotent dedupe (stable key from signal + entity + session).

## Dependencies
- Explicit feedback-ingestion mining, failed-tool-call mapping policy, and extended provenance.

## Non-goals
- No free-text mining. No unbounded ticket creation.

## Acceptance criteria
- A controlled session produces exactly one well-formed, backtraceable follow-up ticket.
- Re-running the same session produces no duplicate.
- The generated ticket links session/turn/tool-call + `FeedbackEntry` + a validation reference.

## Implementation (2026-07-13)

**Gating decision**: synthesis fires only for `ExplicitIngestion` signals whose live `feedback_ingest` call *succeeded* and whose rating is `not-helpful` or `mixed`. Bare `FailedToolCall` signals (even mapped to a known entity, per `MAP`'s policy) are deliberately excluded — most observed failures are transient dev-tool errors, not feedback, and synthesizing from every one would reintroduce the over-triggering failure mode this hardening effort exists to eliminate. A failed `feedback_ingest` call is handled by `INGEST`'s `recover_feedback_entry_from_signal` (records the lost `FeedbackEntry`) but does not synthesize a ticket, since a call that never completed live has not been confirmed as reviewed feedback in the same way a successful call's arguments have.

**Idempotent dedupe**: the ticket's id *is* the dedupe key — `follow_up_ticket_id` derives a deterministic UUIDv5 from `session_id` + `tool_call_id`, so re-running the same session always resolves to the same ticket id. `synthesize_follow_up_ticket` checks `TicketStore::get_indexed` for that id before creating, returning a typed `FollowUpSynthesisOutcome::{Created, AlreadyExists}` — no title-string-equality comparison, no duplicate on re-run.

**Backtrace/verification content**: the synthesized ticket's description links the target `EntityUrn`, `session_id`, `tool_call_id`, and captured `event_id`, plus a note that the live-persisted `FeedbackEntry` should be cross-referenced via `feedback_inbox`/`entries_for(target)` (today's `feedback_ingest` transport does not echo back the created entry's id for a direct link — a follow-up would be threading that id back through the MCP response), and a verification reminder to record a `test-api` validation execution before advancing past `in-review`.

**Implementation**: new `crates/session-api/src/follow_up.rs` (`FollowUpTicketDraft`, `build_follow_up_ticket_draft`, `follow_up_ticket_id`, `synthesize_follow_up_ticket`, `FollowUpSynthesisOutcome`); wired into [copilot-capture-hook.rs](../../../crates/session-api/src/bin/copilot-capture-hook.rs)'s `run()` via `synthesize_follow_up_tickets`, which opens/creates the `.ticket` store and is resilient to store-open failures (logs and skips rather than failing the hook). 5 new unit tests including an idempotent-rerun integration test against a real `TicketStore`. Validation: `vt-feedback-followup-synthesis` / `exec-vt-feedback-followup-synthesis-20260713`.