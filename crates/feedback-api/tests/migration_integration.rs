use std::fs;

use feedback_api::{
    EntityUrn, FEEDBACK_SCHEMA_VERSION, FeedbackEntry, FeedbackProvenance, FeedbackRating,
    FeedbackSource, canonical::CanonicalFeedbackStore,
    migration::migrate_and_discard_legacy_ndjson,
};

#[test]
fn destructive_cutover_migrates_valid_records_and_removes_legacy_input() {
    let directory = tempfile::tempdir().unwrap();
    let legacy_path = directory.path().join("entries.ndjson");
    let entry = FeedbackEntry::new(
        FeedbackSource::Agent,
        EntityUrn::rule("default", "rule-a").unwrap(),
        Some(FeedbackRating::Helpful),
        None,
        None,
        FeedbackProvenance::new(
            None,
            Some("copilot".to_string()),
            Some("2026-01-01T00:00:00Z".to_string()),
        )
        .unwrap(),
    )
    .unwrap();
    fs::write(
        &legacy_path,
        format!(
            "{}\n{{malformed}}\n",
            serde_json::to_string(&entry).unwrap()
        ),
    )
    .unwrap();
    let canonical = CanonicalFeedbackStore::new(directory.path().join("canonical"));

    let outcome = migrate_and_discard_legacy_ndjson(&canonical, "default", &legacy_path).unwrap();

    assert_eq!(outcome.migrated_count, 1);
    assert_eq!(outcome.discarded_count, 1);
    assert!(!legacy_path.exists());
    let migrated = canonical.list_entities().unwrap();
    assert_eq!(migrated.len(), 1);
    assert_eq!(migrated[0].entry.schema_version, FEEDBACK_SCHEMA_VERSION);
}
