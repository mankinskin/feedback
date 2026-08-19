use super::*;

#[test]
fn feedback_input_defaults_note_kind_when_note_text_is_present() {
    let input = RuleFeedbackInput::new(
        FeedbackRating::Mixed,
        Some("  Needs refinement  ".to_string()),
        None,
        None,
        None,
    )
    .unwrap();

    assert_eq!(input.note_text.as_deref(), Some("Needs refinement"));
    assert_eq!(input.note_kind, Some(FeedbackNoteKind::Note));
}

#[test]
fn feedback_input_rejects_partial_session_reference() {
    let err = RuleFeedbackInput::new(
        FeedbackRating::Helpful,
        None,
        None,
        Some("session-1".to_string()),
        None,
    )
    .unwrap_err();

    assert!(err.contains("session_id and agent_or_user_id together"));
}

#[test]
fn parses_ce_urn_and_keeps_supported_store_segments() {
    let rule_urn: EntityUrn = "ce://memory-api/rule/rule-123".parse().unwrap();
    let spec_urn: EntityUrn = "ce://memory-api/spec/spec-123".parse().unwrap();
    let ticket_urn: EntityUrn =
        "ce://memory-api/ticket/ticket-123".parse().unwrap();

    assert_eq!(rule_urn.store(), "rule");
    assert_eq!(spec_urn.store(), "spec");
    assert_eq!(ticket_urn.store(), "ticket");
}

#[test]
fn entity_feedback_core_ranks_usage_and_finds_low_rated_and_unresolved() {
    let mut core = EntityFeedbackCore::default();
    let rule_urn: EntityUrn = "ce://memory-api/rule/rule-123".parse().unwrap();
    let spec_urn: EntityUrn = "ce://memory-api/spec/spec-123".parse().unwrap();

    core.record_usage(rule_urn.clone());
    core.record_usage(rule_urn.clone());
    core.record_usage(spec_urn.clone());

    core.record_rating(
        rule_urn.clone(),
        EntityRatingInput::new(
            FeedbackRating::NotHelpful,
            Some("missing example".to_string()),
            None,
            None,
            None,
        )
        .unwrap(),
    );
    core.record_rating(
        spec_urn.clone(),
        EntityRatingInput::new(FeedbackRating::Helpful, None, None, None, None)
            .unwrap(),
    );

    let by_usage = core.entities_by_usage_frequency(None);
    assert_eq!(by_usage[0].urn, rule_urn);
    assert_eq!(by_usage[0].usage_count, 2);

    let low_rated = core.low_rated_entities();
    assert_eq!(low_rated.len(), 1);
    assert_eq!(low_rated[0].urn, rule_urn);

    let unresolved = core.unresolved_note_entities();
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].urn, rule_urn);
}

#[test]
fn feedback_store_persists_usage_and_rating_for_rule_and_spec_urns() {
    let dir = tempfile::tempdir().unwrap();
    let store = EntityFeedbackStore::new(dir.path(), "memory-api").unwrap();

    let rule_urn = EntityUrn::rule("memory-api", "rule-123").unwrap();
    let spec_urn = EntityUrn::spec("memory-api", "spec-123").unwrap();

    store.record_usage(rule_urn.clone()).unwrap();
    store.record_usage(rule_urn.clone()).unwrap();
    store.record_usage(spec_urn.clone()).unwrap();

    store
        .record_rating(
            rule_urn.clone(),
            EntityRatingInput::new(
                FeedbackRating::NotHelpful,
                Some("too vague".to_string()),
                Some(FeedbackNoteKind::Suggestion),
                None,
                None,
            )
            .unwrap(),
        )
        .unwrap();
    store
        .record_rating(
            spec_urn.clone(),
            EntityRatingInput::new(
                FeedbackRating::Helpful,
                None,
                None,
                None,
                None,
            )
            .unwrap(),
        )
        .unwrap();

    let by_usage = store.entities_by_usage_frequency(None).unwrap();
    assert_eq!(by_usage[0].urn, rule_urn);
    assert_eq!(by_usage[0].usage_count, 2);

    let low_rated = store.low_rated_entities().unwrap();
    assert_eq!(low_rated.len(), 1);
    assert_eq!(low_rated[0].urn, rule_urn);

    let unresolved = store.unresolved_note_entities().unwrap();
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].urn, rule_urn);

    let spec_summary = store.summary_for(&spec_urn).unwrap();
    assert_eq!(spec_summary.usage_count, 1);
    assert_eq!(spec_summary.helpful_count, 1);
}

#[test]
fn feedback_store_accepts_ticket_urn_extension_point() {
    let dir = tempfile::tempdir().unwrap();
    let store = EntityFeedbackStore::new(dir.path(), "memory-api").unwrap();
    let ticket_urn = EntityUrn::ticket("memory-api", "ticket-123").unwrap();

    store.record_usage(ticket_urn.clone()).unwrap();
    let summary = store.summary_for(&ticket_urn).unwrap();

    assert_eq!(summary.usage_count, 1);
}

#[test]
fn ingest_author_normalizes_id_and_requires_privileged_identity() {
    let human = IngestAuthor::human(Some("  ada  ".to_string())).unwrap();
    assert_eq!(human.kind(), FeedbackAuthorKind::Human);
    assert_eq!(human.id(), Some("ada"));

    // Human may omit identity; empty-after-trim is treated as absent.
    let anon = IngestAuthor::human(Some("   ".to_string())).unwrap();
    assert_eq!(anon.id(), None);

    // Privileged agents must supply a non-empty identity.
    let err = IngestAuthor::new(
        FeedbackAuthorKind::PrivilegedAgent,
        Some("  ".to_string()),
    )
    .unwrap_err();
    assert!(err.contains("privileged-agent ingestion requires"));

    let agent = IngestAuthor::privileged_agent("copilot").unwrap();
    assert_eq!(agent.kind(), FeedbackAuthorKind::PrivilegedAgent);
    assert_eq!(agent.id(), Some("copilot"));
}

#[test]
fn author_kind_round_trips_through_persisted_events() {
    let dir = tempfile::tempdir().unwrap();
    let store = EntityFeedbackStore::new(dir.path(), "memory-api").unwrap();
    let rule_urn = EntityUrn::rule("memory-api", "rule-a").unwrap();
    let agent = IngestAuthor::privileged_agent("copilot").unwrap();

    let usage = store.ingest_usage(&agent, rule_urn.clone()).unwrap();
    assert_eq!(usage.author_kind, Some(FeedbackAuthorKind::PrivilegedAgent));
    assert_eq!(usage.author_id.as_deref(), Some("copilot"));

    // Rating input without an explicit actor backfills from the author.
    let rating = store
        .ingest_rating(
            &agent,
            rule_urn.clone(),
            EntityRatingSubmission::new(FeedbackRating::Helpful),
        )
        .unwrap();
    assert_eq!(
        rating.author_kind,
        Some(FeedbackAuthorKind::PrivilegedAgent)
    );
    assert_eq!(rating.agent_or_user_id.as_deref(), Some("copilot"));

    // Reload from disk to confirm the author metadata persisted.
    let reloaded: Vec<EntityUsageEvent> =
        read_ndjson(&store.usage_events_path()).unwrap();
    assert_eq!(reloaded.len(), 1);
    assert_eq!(
        reloaded[0].author_kind,
        Some(FeedbackAuthorKind::PrivilegedAgent)
    );
}

#[test]
fn ingest_rating_backfills_author_for_session_scoped_rating() {
    let dir = tempfile::tempdir().unwrap();
    let store = EntityFeedbackStore::new(dir.path(), "memory-api").unwrap();
    let rule_urn = EntityUrn::rule("memory-api", "rule-session").unwrap();
    let agent = IngestAuthor::privileged_agent("copilot").unwrap();

    // Session-scoped rating with the actor id omitted: the author must
    // backfill `agent_or_user_id` before the paired-session invariant is
    // validated, so this call succeeds without repeating the author id.
    let mut submission = EntityRatingSubmission::new(FeedbackRating::Mixed);
    submission.session_id = Some("session-42".to_string());

    let rating = store
        .ingest_rating(&agent, rule_urn.clone(), submission)
        .unwrap();

    assert_eq!(rating.session_id.as_deref(), Some("session-42"));
    assert_eq!(rating.agent_or_user_id.as_deref(), Some("copilot"));
    assert_eq!(
        rating.author_kind,
        Some(FeedbackAuthorKind::PrivilegedAgent)
    );

    // The persisted event carries the backfilled actor and session.
    let reloaded: Vec<EntityRatingEvent> =
        read_ndjson(&store.rating_events_path()).unwrap();
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].session_id.as_deref(), Some("session-42"));
    assert_eq!(reloaded[0].agent_or_user_id.as_deref(), Some("copilot"));

    // A human author with no identity still cannot attribute a session.
    let anon = IngestAuthor::human(None).unwrap();
    let mut orphan = EntityRatingSubmission::new(FeedbackRating::Mixed);
    orphan.session_id = Some("session-99".to_string());
    let err = store.ingest_rating(&anon, rule_urn, orphan).unwrap_err();
    assert!(err.contains("session_id and agent_or_user_id together"));
}

#[test]
fn retention_prunes_by_age_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let store = EntityFeedbackStore::new(dir.path(), "memory-api").unwrap();
    let urn = EntityUrn::rule("memory-api", "rule-age").unwrap();
    let path = store.usage_events_path();

    // Write events by hand so timestamps are deterministic.
    let old = EntityUsageEvent {
        timestamp: "2000-01-01T00:00:00+00:00".to_string(),
        urn: urn.clone(),
        author_kind: None,
        author_id: None,
    };
    let recent = EntityUsageEvent {
        timestamp: "2025-01-01T00:00:00+00:00".to_string(),
        urn: urn.clone(),
        author_kind: None,
        author_id: None,
    };
    append_ndjson(&path, &old).unwrap();
    append_ndjson(&path, &recent).unwrap();

    let now = chrono::DateTime::parse_from_rfc3339("2025-01-10T00:00:00+00:00")
        .unwrap()
        .with_timezone(&Utc);
    let policy = RetentionPolicy::max_age_days(30);

    let outcome = store.apply_retention_at(&policy, now).unwrap();
    assert_eq!(outcome.usage.retained, 1);
    assert_eq!(outcome.usage.removed, 1);

    let kept: Vec<EntityUsageEvent> = read_ndjson(&path).unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].timestamp, recent.timestamp);

    // Second pass removes nothing.
    let repeat = store.apply_retention_at(&policy, now).unwrap();
    assert_eq!(repeat.total_removed(), 0);
}

#[test]
fn retention_keeps_most_recent_events_by_count() {
    let dir = tempfile::tempdir().unwrap();
    let store = EntityFeedbackStore::new(dir.path(), "memory-api").unwrap();
    let urn = EntityUrn::rule("memory-api", "rule-count").unwrap();
    let path = store.usage_events_path();

    for day in 1..=5 {
        let event = EntityUsageEvent {
            timestamp: format!("2025-01-0{day}T00:00:00+00:00"),
            urn: urn.clone(),
            author_kind: None,
            author_id: None,
        };
        append_ndjson(&path, &event).unwrap();
    }

    let outcome = store
        .apply_retention_at(&RetentionPolicy::max_events(2), Utc::now())
        .unwrap();
    assert_eq!(outcome.usage.retained, 2);
    assert_eq!(outcome.usage.removed, 3);

    let kept: Vec<EntityUsageEvent> = read_ndjson(&path).unwrap();
    assert_eq!(kept.len(), 2);
    // The two most recent (chronologically last) events survive.
    assert_eq!(kept[0].timestamp, "2025-01-04T00:00:00+00:00");
    assert_eq!(kept[1].timestamp, "2025-01-05T00:00:00+00:00");
}

#[test]
fn feedback_entry_round_trip_records_schema_and_status() {
    let dir = tempfile::tempdir().unwrap();
    let store = EntityFeedbackStore::new(dir.path(), "memory-api").unwrap();
    let urn = EntityUrn::rule("memory-api", "rule-entry").unwrap();

    let entry = FeedbackEntry::new(
        FeedbackSource::System,
        urn.clone(),
        Some(FeedbackRating::Mixed),
        Some("Needs stronger examples".to_string()),
        Some(FeedbackNoteKind::Suggestion),
        FeedbackProvenance::new(
            Some("session-1".to_string()),
            Some("copilot".to_string()),
            None,
        )
        .unwrap(),
    )
    .unwrap();

    store.record_entry(entry).unwrap();
    let loaded = store.entries_for(&urn).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].schema_version, FEEDBACK_SCHEMA_VERSION);
    assert_eq!(loaded[0].status, FeedbackStatus::New);
    assert_eq!(loaded[0].source, FeedbackSource::System);
}

#[test]
fn feedback_provenance_round_trips_turn_and_tool_call_refs() {
    let provenance = FeedbackProvenance::from_session_turn(
        Some("session-42".to_string()),
        Some("session-api/structured-miner".to_string()),
        None,
        Some(7),
        Some("call-7".to_string()),
    )
    .unwrap();

    assert_eq!(provenance.session_id.as_deref(), Some("session-42"));
    assert_eq!(provenance.turn_sequence, Some(7));
    assert_eq!(provenance.tool_call_id.as_deref(), Some("call-7"));

    let json = serde_json::to_string(&provenance).unwrap();
    let round_tripped: FeedbackProvenance =
        serde_json::from_str(&json).unwrap();
    assert_eq!(round_tripped, provenance);
}

#[test]
fn feedback_provenance_deserializes_pre_v2_records_without_turn_refs() {
    // Pre-existing on-disk records predate `turn_sequence`/`tool_call_id`;
    // both fields must default to `None` rather than fail deserialization.
    let legacy_json = r#"{"session_id":"session-1","author":"copilot","executed_at":"2025-01-01T00:00:00Z"}"#;
    let provenance: FeedbackProvenance =
        serde_json::from_str(legacy_json).unwrap();

    assert_eq!(provenance.session_id.as_deref(), Some("session-1"));
    assert_eq!(provenance.turn_sequence, None);
    assert_eq!(provenance.tool_call_id, None);
}

#[test]
fn mined_entry_asserts_populated_backtrace_refs() {
    let dir = tempfile::tempdir().unwrap();
    let store = EntityFeedbackStore::new(dir.path(), "memory-api").unwrap();
    let urn = EntityUrn::rule("memory-api", "rule-entry").unwrap();

    let provenance = FeedbackProvenance::from_session_turn(
        Some("session-mined-1".to_string()),
        Some("session-api/structured-miner".to_string()),
        None,
        Some(3),
        Some("call-3".to_string()),
    )
    .unwrap();

    let entry = FeedbackEntry::new(
        FeedbackSource::TranscriptMined,
        urn.clone(),
        None,
        Some("failed tool call detected".to_string()),
        Some(FeedbackNoteKind::Note),
        provenance,
    )
    .unwrap();

    store.record_entry(entry).unwrap();
    let loaded = store.entries_for(&urn).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(
        loaded[0].provenance.session_id.as_deref(),
        Some("session-mined-1")
    );
    assert_eq!(loaded[0].provenance.turn_sequence, Some(3));
    assert_eq!(loaded[0].provenance.tool_call_id.as_deref(), Some("call-3"));
}
