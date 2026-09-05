use std::fs;

use chrono::{TimeZone, Utc};
use feedback_api::{
    EntityFeedbackStore, EntityUrn, FeedbackEntry, FeedbackProvenance, FeedbackRating,
    FeedbackSource, FeedbackStatus, FEEDBACK_SCHEMA_VERSION,
};

fn timestamp(day: u32) -> String {
    Utc.with_ymd_and_hms(2026, 1, day, 0, 0, 0)
        .unwrap()
        .to_rfc3339()
}

#[test]
fn analytics_at_reports_current_schema_statistics_without_rewriting_input() {
    let directory = tempfile::tempdir().unwrap();
    let store = EntityFeedbackStore::new(directory.path(), "default").unwrap();
    let entry = FeedbackEntry {
        id: "analytics-entry".to_string(),
        schema_version: FEEDBACK_SCHEMA_VERSION,
        source: FeedbackSource::Agent,
        target: EntityUrn::rule("default", "rule-a").unwrap(),
        rating: Some(FeedbackRating::NotHelpful),
        note_text: None,
        note_kind: None,
        provenance: FeedbackProvenance::new(
            Some("session-a".to_string()),
            Some("copilot".to_string()),
            Some(timestamp(1)),
        )
        .unwrap(),
        status: FeedbackStatus::New,
    };
    store.record_entry(entry).unwrap();

    let path = directory
        .path()
        .join("default")
        .join("feedback-core")
        .join("entries.ndjson");
    fs::write(
        &path,
        format!("{}\n{{malformed}}\n", fs::read_to_string(&path).unwrap()),
    )
    .unwrap();
    let before = fs::read_to_string(&path).unwrap();

    let report = store
        .analytics_at(Utc.with_ymd_and_hms(2026, 1, 12, 0, 0, 0).unwrap())
        .unwrap();

    assert_eq!(report.valid_event_count, 1);
    assert_eq!(report.malformed_line_count, 2);
    assert_eq!(report.source_distribution.get("agent"), Some(&1));
    assert_eq!(report.rating_distribution.get("not-helpful"), Some(&1));
    assert_eq!(report.incidents.len(), 1);
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
}
