## Motivation
Structured feedback mining must iterate discovered entities deterministically. Per the agreed design (decision #2): iterate breadth-first, and only queue newly-discovered entities for detection after each unit is processed, so entities discovered later are appended to the queue.

## Scope
- Implement a BFS work-queue in the structured miner: process signals/turns in order, enqueue newly-discovered entity URNs, dedupe by URN, and mine only queued entities.
- Acceptable alternative (document the choice if taken): mine only the entities detected at the beginning.
- Deterministic, reproducible ordering.

## Dependencies
- Requires entity discovery from explicit-ingestion mining and the failed-tool-call mapping policy.

## Non-goals
- No message-text heuristics.

## Acceptance criteria
- A test asserting deterministic BFS ordering and correct dedupe for a multi-entity session.