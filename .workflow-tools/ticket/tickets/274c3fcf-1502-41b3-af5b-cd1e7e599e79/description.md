## Objective
Author the repository’s missing shared feedback API specification.

## Context
Owner decision: “ONE shared feedback API spec; `feedback-cli` and `feedback-mcp` are transports of it. Currently the only fully UNSPECIFIED tool.”
Audit: `tmp/test-coverage-audit/03-requirements.md`.

## Acceptance criteria
- Create one canonical feedback API spec.
- Define CLI and MCP as transports, not independent contracts.
- Link transport behavior and validation obligations.

## Out of scope
- Feedback implementation or transport tests.