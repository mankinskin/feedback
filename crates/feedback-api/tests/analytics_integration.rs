use chrono::{TimeZone, Utc};
use feedback_api::{
    EntityFeedbackStore, EntityUrn, FeedbackEntry, FeedbackProvenance, FeedbackRating,
    FeedbackSource,
};

#[test]
fn analytics_at_reports_canonical_feedback_entry_statistics() {
    let directory = tempfile::tempdir().unwrap();
    let store = EntityFeedbackStore::new(directory.path());
    let entry = FeedbackEntry::new(
        FeedbackSource::Agent,
        EntityUrn::rule("default", "rule-a").unwrap(),
        Some(FeedbackRating::NotHelpful),
        None,
        None,
        FeedbackProvenance::new(
            Some("session-a".to_string()),
            Some("copilot".to_string()),
            Some("2026-01-01T00:00:00Z".to_string()),
        )
        .unwrap(),
    )
    .unwrap();
    store.record_entry(entry).unwrap();

    let report = store
        .analytics_at(Utc.with_ymd_and_hms(2026, 1, 12, 0, 0, 0).unwrap())
        .unwrap();

    assert_eq!(report.valid_event_count, 1);
    assert_eq!(report.malformed_line_count, 0);
    assert_eq!(report.source_distribution.get("agent"), Some(&1));
    assert_eq!(report.rating_distribution.get("not-helpful"), Some(&1));
    assert_eq!(report.incidents.len(), 1);
}
