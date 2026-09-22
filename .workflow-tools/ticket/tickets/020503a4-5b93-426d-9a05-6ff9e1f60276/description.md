## Objective
Test the shared feedback API contract across feedback-cli and feedback-mcp.

## Context
Owner decision: “ONE shared feedback API spec; `feedback-cli` and `feedback-mcp` are transports of it.”

## Acceptance criteria
- Derive shared contract tests from the new canonical spec.
- Assert transport parity and contract-specific errors.

## Out of scope
- Feedback API specification authoring.