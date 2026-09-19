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

    let summary = store.session_summary("session-a", None).unwrap();

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

    let summary = store.session_summary("no-such-session", None).unwrap();

    assert_eq!(summary.total_count, 0);
    assert!(summary.entries.is_empty());
}

#[test]
fn session_summary_narrows_to_turn_sequence_when_requested() {
    let directory = tempfile::tempdir().unwrap();
    let store = EntityFeedbackStore::new(directory.path());

    let turn_one_entry = FeedbackEntry::new(
        FeedbackSource::Agent,
        EntityUrn::rule("default", "rule-a").unwrap(),
        Some(FeedbackRating::Mixed),
        Some("turn one finding".to_string()),
        None,
        FeedbackProvenance::from_session_turn(
            Some("session-a".to_string()),
            Some("copilot".to_string()),
            None,
            Some(1),
            None,
        )
        .unwrap(),
    )
    .unwrap();
    let turn_two_entry = FeedbackEntry::new(
        FeedbackSource::Agent,
        EntityUrn::rule("default", "rule-b").unwrap(),
        Some(FeedbackRating::Helpful),
        None,
        None,
        FeedbackProvenance::from_session_turn(
            Some("session-a".to_string()),
            Some("copilot".to_string()),
            None,
            Some(2),
            None,
        )
        .unwrap(),
    )
    .unwrap();
    store.record_entry(turn_one_entry).unwrap();
    store.record_entry(turn_two_entry).unwrap();

    let turn_one_summary = store.session_summary("session-a", Some(1)).unwrap();
    assert_eq!(turn_one_summary.total_count, 1);
    assert_eq!(turn_one_summary.mixed_count, 1);

    let whole_session_summary = store.session_summary("session-a", None).unwrap();
    assert_eq!(whole_session_summary.total_count, 2);
}
