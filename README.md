Back to [workflow-tools](..).

# feedback

Feedback system: normalized feedback events, summaries, and file-backed persistence for recording ratings and notes against any entity (ticket, spec, session, tool).

## Primary Use Case

Record feedback (a rating, a note, a mined transcript observation) against an entity URN, then query it back as a summary or an inbox instead of scattering feedback across chat transcripts.

## Usage

Build the desired transport from the `workflow-tools` workspace root:

```bash
cargo run -p feedback-cli --bin feedback -- --help
cargo run -p feedback-mcp --bin feedback-mcp
cargo run -p feedback-http --bin feedback-http
```

## Examples

```bash
# Ingest a feedback entry
cargo run -p feedback-cli --bin feedback -- ingest --target <entity-urn> --source cli --rating good

# Read the inbox for an entity
cargo run -p feedback-cli --bin feedback -- inbox --target <entity-urn>
```

## Related Crates

- [crates/feedback-api](crates/feedback-api): core library (events, summaries, persistence).
- [crates/feedback-cli](crates/feedback-cli): CLI transport (binary `feedback`).
- [crates/feedback-http](crates/feedback-http): HTTP REST API (binary `feedback-http`).
- [crates/feedback-mcp](crates/feedback-mcp): MCP server transport (binary `feedback-mcp`).
