use std::process::Command;

use tempfile::tempdir;
use uuid::Uuid;

use crate::{
    EntityUrn,
    FeedbackEntry,
    FeedbackProvenance,
    FeedbackRating,
    FeedbackSource,
    canonical::{
        CanonicalFeedbackEntity,
        CanonicalFeedbackStore,
        canonical_entity_id,
        compute_digest,
    },
    migration::{
        migrate_legacy_ndjson,
        parse_legacy_records,
        resume_migration,
        rollback_migration,
    },
};

fn run_git(
    repo_root: &std::path::Path,
    args: &[&str],
) {
    let status = Command::new("git")
        .current_dir(repo_root)
        .args(args)
        .status()
        .expect("git command");
    assert!(status.success(), "git {args:?} failed: {status}");
}

fn sample_entry(
    id: &str,
    executed_at: &str,
) -> FeedbackEntry {
    let target = EntityUrn::ticket("demo-workspace", "some-ticket").unwrap();
    let provenance = FeedbackProvenance::new(
        None,
        Some("author".to_string()),
        Some(executed_at.to_string()),
    )
    .unwrap();
    let mut entry = FeedbackEntry::new(
        FeedbackSource::Agent,
        target,
        Some(FeedbackRating::Helpful),
        None,
        None,
        provenance,
    )
    .unwrap();
    entry.id = id.to_string();
    entry
}

fn write_legacy_ndjson(
    path: &std::path::Path,
    lines: &[String],
) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, lines.join("\n") + "\n").unwrap();
}

#[test]
fn move_domain_digest_covers_alias_and_ordinal() {
    let entry = sample_entry(
        "11111111-1111-4111-8111-111111111111",
        "2026-01-01T00:00:00Z",
    );
    let digest_a = compute_digest(&entry, Some("legacy-1"), 0);
    let digest_b = compute_digest(&entry, Some("legacy-1"), 1);
    let digest_c = compute_digest(&entry, None, 0);
    assert_ne!(digest_a, digest_b, "ordinal must change the digest");
    assert_ne!(digest_a, digest_c, "alias must change the digest");
}

#[test]
fn move_domain_canonical_entity_id_retains_existing_uuid() {
    let uuid_id = "22222222-2222-4222-8222-222222222222";
    let resolved = canonical_entity_id("demo-workspace", uuid_id);
    assert_eq!(resolved.to_string(), uuid_id);
}

#[test]
fn move_domain_canonical_entity_id_is_deterministic_for_legacy_ids() {
    let a = canonical_entity_id("demo-workspace", "legacy-42");
    let b = canonical_entity_id("demo-workspace", "legacy-42");
    let c = canonical_entity_id("other-workspace", "legacy-42");
    assert_eq!(a, b, "same workspace+legacy id must derive the same uuid");
    assert_ne!(a, c, "different workspace slug must derive a different uuid");
}

#[test]
fn move_domain_migration_rejects_malformed_legacy_record_before_mutation() {
    let temp = tempdir().unwrap();
    let store = CanonicalFeedbackStore::new(temp.path().join(".feedback"));
    let legacy_path = temp.path().join("legacy").join("entries.ndjson");
    write_legacy_ndjson(
        &legacy_path,
        &["{not valid json".to_string()],
    );

    let result = migrate_legacy_ndjson(&store, "demo-workspace", &legacy_path);
    assert!(result.is_err(), "malformed record must block migration");
    assert!(
        store.list_entities().unwrap().is_empty(),
        "no entity may be staged when validation fails"
    );
}

#[test]
fn move_domain_migration_rejects_duplicate_canonical_ids_before_mutation() {
    let temp = tempdir().unwrap();
    let store = CanonicalFeedbackStore::new(temp.path().join(".feedback"));
    let legacy_path = temp.path().join("legacy").join("entries.ndjson");
    let entry_a = sample_entry("dup-id", "2026-01-01T00:00:00Z");
    let entry_b = sample_entry("dup-id", "2026-01-02T00:00:00Z");
    write_legacy_ndjson(
        &legacy_path,
        &[
            serde_json::to_string(&entry_a).unwrap(),
            serde_json::to_string(&entry_b).unwrap(),
        ],
    );

    let result = migrate_legacy_ndjson(&store, "demo-workspace", &legacy_path);
    assert!(result.is_err(), "duplicate canonical id must block migration");
    assert!(store.list_entities().unwrap().is_empty());
}

#[test]
fn move_domain_migration_preserves_physical_order_and_uuid_alias() {
    let temp = tempdir().unwrap();
    let store = CanonicalFeedbackStore::new(temp.path().join(".feedback"));
    let legacy_path = temp.path().join("legacy").join("entries.ndjson");
    let uuid_entry = sample_entry(
        "33333333-3333-4333-8333-333333333333",
        "2026-01-01T00:00:00Z",
    );
    let legacy_entry = sample_entry("legacy-7", "2026-01-01T00:00:00Z");
    write_legacy_ndjson(
        &legacy_path,
        &[
            String::new(),
            serde_json::to_string(&uuid_entry).unwrap(),
            serde_json::to_string(&legacy_entry).unwrap(),
        ],
    );

    let outcome =
        migrate_legacy_ndjson(&store, "demo-workspace", &legacy_path).unwrap();
    assert!(outcome.completed);
    assert_eq!(outcome.migrated_ids.len(), 2);

    let entities = store.list_entities().unwrap();
    assert_eq!(entities.len(), 2);
    assert_eq!(entities[0].legacy_append_ordinal, 0);
    assert_eq!(entities[1].legacy_append_ordinal, 1);
    assert_eq!(
        entities[0].id.to_string(),
        "33333333-3333-4333-8333-333333333333"
    );
    assert!(entities[0].alias.is_none(), "matching uuid id has no alias");
    assert_eq!(
        entities[1].alias.as_deref(),
        Some("legacy-7"),
        "non-uuid legacy id is preserved as an alias"
    );
    for entity in &entities {
        assert!(entity.digest_is_valid());
    }
}

#[test]
fn move_domain_migration_equal_timestamps_preserve_ordinal_ordering() {
    let temp = tempdir().unwrap();
    let store = CanonicalFeedbackStore::new(temp.path().join(".feedback"));
    let legacy_path = temp.path().join("legacy").join("entries.ndjson");
    let first = sample_entry("legacy-a", "2026-01-01T00:00:00Z");
    let second = sample_entry("legacy-b", "2026-01-01T00:00:00Z");
    write_legacy_ndjson(
        &legacy_path,
        &[
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap(),
        ],
    );

    migrate_legacy_ndjson(&store, "demo-workspace", &legacy_path).unwrap();
    let entities = store.list_entities().unwrap();
    assert_eq!(entities[0].alias.as_deref(), Some("legacy-a"));
    assert_eq!(entities[1].alias.as_deref(), Some("legacy-b"));
}

#[test]
fn move_domain_migration_fails_closed_on_unowned_mixed_layout() {
    let temp = tempdir().unwrap();
    let store = CanonicalFeedbackStore::new(temp.path().join(".feedback"));
    let legacy_path = temp.path().join("legacy").join("entries.ndjson");
    write_legacy_ndjson(
        &legacy_path,
        &[serde_json::to_string(&sample_entry(
            "legacy-1",
            "2026-01-01T00:00:00Z",
        ))
        .unwrap()],
    );

    // Simulate a pre-existing, unowned canonical entity (no manifest).
    let stray_id = Uuid::new_v4();
    let stray = CanonicalFeedbackEntity::new(
        stray_id,
        None,
        0,
        sample_entry("legacy-stray", "2026-01-01T00:00:00Z"),
    );
    store.write_entity(&stray).unwrap();

    let result = migrate_legacy_ndjson(&store, "demo-workspace", &legacy_path);
    assert!(result.is_err(), "mixed layout without a manifest must fail closed");
}

#[test]
fn move_domain_migration_resume_is_idempotent_and_converges() {
    let temp = tempdir().unwrap();
    let store = CanonicalFeedbackStore::new(temp.path().join(".feedback"));
    let legacy_path = temp.path().join("legacy").join("entries.ndjson");
    write_legacy_ndjson(
        &legacy_path,
        &[
            serde_json::to_string(&sample_entry(
                "legacy-1",
                "2026-01-01T00:00:00Z",
            ))
            .unwrap(),
            serde_json::to_string(&sample_entry(
                "legacy-2",
                "2026-01-02T00:00:00Z",
            ))
            .unwrap(),
        ],
    );

    let first = migrate_legacy_ndjson(&store, "demo-workspace", &legacy_path).unwrap();
    let resumed = resume_migration(&store).unwrap();
    assert_eq!(first.manifest_id, resumed.manifest_id);
    assert!(resumed.completed);
    assert_eq!(store.list_entities().unwrap().len(), 2);

    // Resuming again after completion is a no-op.
    let resumed_again = resume_migration(&store).unwrap();
    assert!(resumed_again.completed);
    assert_eq!(store.list_entities().unwrap().len(), 2);
}

#[test]
fn move_domain_migration_rollback_converges_to_zero_and_is_idempotent() {
    let temp = tempdir().unwrap();
    let store = CanonicalFeedbackStore::new(temp.path().join(".feedback"));
    let legacy_path = temp.path().join("legacy").join("entries.ndjson");
    write_legacy_ndjson(
        &legacy_path,
        &[serde_json::to_string(&sample_entry(
            "legacy-1",
            "2026-01-01T00:00:00Z",
        ))
        .unwrap()],
    );

    migrate_legacy_ndjson(&store, "demo-workspace", &legacy_path).unwrap();
    assert_eq!(store.list_entities().unwrap().len(), 1);

    rollback_migration(&store).unwrap();
    assert!(store.list_entities().unwrap().is_empty());
    // Legacy backup is never deleted by rollback.
    assert!(legacy_path.is_file());

    // Rolling back again with no manifest present is an explicit error, not
    // a silent success — the caller must not report success while there is
    // nothing left to converge.
    assert!(rollback_migration(&store).is_err());
}

#[test]
fn move_domain_parse_legacy_records_rejects_trailing_malformed_input() {
    let temp = tempdir().unwrap();
    let legacy_path = temp.path().join("entries.ndjson");
    write_legacy_ndjson(
        &legacy_path,
        &[
            serde_json::to_string(&sample_entry(
                "legacy-1",
                "2026-01-01T00:00:00Z",
            ))
            .unwrap(),
            "not-json-at-all".to_string(),
        ],
    );

    assert!(parse_legacy_records(&legacy_path).is_err());
}

fn init_git_workspace(root: &std::path::Path) -> std::path::PathBuf {
    std::fs::create_dir_all(root).unwrap();
    run_git(root, &["init"]);
    root.to_path_buf()
}

#[test]
fn move_domain_entity_set_preflight_apply_and_rollback_preserve_fields() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    init_git_workspace(&repo);

    let source_workspace = repo.join("source");
    let target_workspace = repo.join("target");
    std::fs::create_dir_all(&source_workspace).unwrap();
    std::fs::create_dir_all(target_workspace.join(".feedback")).unwrap();

    let source_store =
        CanonicalFeedbackStore::new(source_workspace.join(".feedback"));
    let entity = source_store
        .append_new_entry(
            "demo-workspace",
            sample_entry("legacy-1", "2026-01-01T00:00:00Z"),
        )
        .unwrap();
    let entity_id = entity.id;
    let source_digest = entity.digest.clone();

    let plan = source_store
        .plan_move_set(&[entity_id], &target_workspace)
        .unwrap();
    assert!(plan.supported(), "expected supported plan: {plan:?}");

    let outcome = source_store.execute_move_set(&plan).unwrap();
    assert!(outcome.journal.entity_ids.contains(&entity_id));

    let target_store =
        CanonicalFeedbackStore::new(target_workspace.join(".feedback"));
    let migrated = target_store
        .read_entity(&entity_id)
        .unwrap()
        .expect("entity present at destination after apply");
    assert_eq!(migrated.digest, source_digest);
    assert_eq!(migrated.alias.as_deref(), Some("legacy-1"));
    assert_eq!(migrated.legacy_append_ordinal, 0);
    assert!(
        source_store.read_entity(&entity_id).unwrap().is_none(),
        "source entity must be removed only after destination persists"
    );

    let journal_id = outcome.journal.id;
    let rolled_back = source_store.rollback_move_set(journal_id).unwrap();
    assert!(rolled_back.journal.rollback_completed_entity_ids.contains(&entity_id));
    let restored = source_store
        .read_entity(&entity_id)
        .unwrap()
        .expect("entity restored to source after rollback");
    assert_eq!(restored.digest, source_digest);
    assert!(
        target_store.read_entity(&entity_id).unwrap().is_none(),
        "destination entity must be removed after rollback"
    );
}
