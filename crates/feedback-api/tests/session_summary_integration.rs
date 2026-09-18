use feedback_api::{
    EntityFeedbackStore, EntityUrn, FeedbackEntry, FeedbackProvenance, FeedbackRating,
    FeedbackSource,
};

#[test]
fn session_summary_filters_entries_by_session_id_only() {
    let directory = tempfile::tempdir().unwrap();
    let store = EntityFeedbackStore::new(directory.path());

    let session_a_entry = FeedbackEntry::new(
        FeedbackSource::Agent,
        EntityUrn::rule("default", "rule-a").unwrap(),
        Some(FeedbackRating::Mixed),
        Some("unexpected tool behavior".to_string()),
        None,
        FeedbackProvenance::new(
            Some("session-a".to_string()),
            Some("copilot".to_string()),
            None,
        )
        .unwrap(),
    )
    .unwrap();
    let session_b_entry = FeedbackEntry::new(
        FeedbackSource::Agent,
        EntityUrn::rule("default", "rule-b").unwrap(),
        Some(FeedbackRating::Helpful),
        None,
        None,
        FeedbackProvenance::new(
            Some("session-b".to_string()),
            Some("copilot".to_string()),
            None,
        )
        .unwrap(),
    )
    .unwrap();
    store.record_entry(session_a_entry).unwrap();
    store.record_entry(session_b_entry).unwrap();

    let summary = store.session_summary("session-a").unwrap();

    assert_eq!(summary.session_id, "session-a");
    assert_eq!(summary.total_count, 1);
    assert_eq!(summary.mixed_count, 1);
    assert_eq!(summary.helpful_count, 0);
    assert_eq!(summary.note_count, 1);
    assert_eq!(summary.entries.len(), 1);
    assert_eq!(
        summary.entries[0].target,
        EntityUrn::rule("default", "rule-a").unwrap()
    );
}

#[test]
fn session_summary_returns_empty_rollup_for_unknown_session() {
    let directory = tempfile::tempdir().unwrap();
    let store = EntityFeedbackStore::new(directory.path());

    let summary = store.session_summary("no-such-session").unwrap();

    assert_eq!(summary.total_count, 0);
    assert!(summary.entries.is_empty());
}
