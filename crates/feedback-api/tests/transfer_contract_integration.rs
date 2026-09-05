//! Temporary-root parity test for the feedback transfer contract
//! (see `transcripts/03-09-2026_repository-entity-distribution/05-feedback-transfer-contract.md`).
//!
//! Migrates a fixture `entries.ndjson`, transfers a fixture canonical entity
//! set from `feedback/test-fixtures/transfer-fixture` into a canonical
//! `.workflow-tools/feedback` container, rolls it back, and confirms
//! discovery finds the same physical store (same `entry_id`,
//! `canonical_path`, `digest`) whether scanned from a synthesized
//! `meta-workspace` root or directly from `workflow-tools/feedback`.

use std::{
    fs,
    path::{
        Path,
        PathBuf,
    },
};

use feedback_api::canonical::{
    CanonicalFeedbackEntity,
    CanonicalFeedbackStore,
};
use memory_kernel::ContentKind;
use uuid::Uuid;

fn run_git(
    repo_root: &Path,
    args: &[&str],
) {
    let status = std::process::Command::new("git")
        .current_dir(repo_root)
        .args(args)
        .status()
        .expect("git command");
    assert!(status.success(), "git {args:?} failed: {status}");
}

fn copy_dir_recursive(
    from: &Path,
    to: &Path,
) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir_recursive(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), &dest).unwrap();
        }
    }
}

/// Normalize the checked-in fixture entity so its digest is always valid,
/// independent of whatever placeholder digest is committed in the fixture
/// file. This keeps the fixture human-readable without hand-computing a
/// SHA-256 by hand.
fn normalize_fixture_entity(store: &CanonicalFeedbackStore) -> Uuid {
    let entities = store.list_entities().unwrap();
    assert_eq!(entities.len(), 1, "fixture must contain exactly one entity");
    let raw = &entities[0];
    let normalized = CanonicalFeedbackEntity::new(
        raw.id,
        raw.alias.clone(),
        raw.legacy_append_ordinal,
        raw.entry.clone(),
    );
    store.write_entity(&normalized).unwrap();
    raw.id
}

#[test]
fn feedback_transfer_contract_temporary_root_parity() {
    let temp = tempfile::tempdir().unwrap();
    let meta_workspace = temp.path().join("meta-workspace");
    let workflow_tools_feedback = meta_workspace.join("workflow-tools").join("feedback");
    fs::create_dir_all(&workflow_tools_feedback).unwrap();
    run_git(&meta_workspace, &["init"]);

    // Seed the source fixture into the temporary root.
    let fixture_source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("feedback crate root")
        .join("test-fixtures")
        .join("transfer-fixture");
    let fixture_target = workflow_tools_feedback.join("test-fixtures").join("transfer-fixture");
    copy_dir_recursive(&fixture_source, &fixture_target);

    let source_store = CanonicalFeedbackStore::new(fixture_target.join(".feedback"));
    let entity_id = normalize_fixture_entity(&source_store);
    let source_digest = source_store
        .read_entity(&entity_id)
        .unwrap()
        .expect("normalized fixture entity")
        .digest;

    // Transfer the fixture entity set into the canonical container. The
    // destination repo is presumed already onboarded to the canonical
    // `.workflow-tools/<domain>` layout, so pre-create the (empty) container
    // the same way an already-migrated sibling domain would leave it.
    fs::create_dir_all(
        workflow_tools_feedback.join(".workflow-tools").join("feedback"),
    )
    .unwrap();
    let plan = source_store
        .plan_move_set(&[entity_id], &workflow_tools_feedback)
        .unwrap();
    assert!(plan.supported(), "expected a supported move plan: {plan:?}");
    let outcome = source_store.execute_move_set(&plan).unwrap();
    assert!(outcome.journal.entity_ids.contains(&entity_id));

    let canonical_container_store =
        CanonicalFeedbackStore::new(workflow_tools_feedback.join(".workflow-tools").join("feedback"));
    let migrated = canonical_container_store
        .read_entity(&entity_id)
        .unwrap()
        .expect("entity present in canonical container after apply");
    assert_eq!(migrated.digest, source_digest, "digest must survive the move");
    assert!(
        source_store.read_entity(&entity_id).unwrap().is_none(),
        "source entity removed only after destination persisted"
    );

    // Discovery parity: scanning from the synthesized "meta-workspace" root
    // and scanning directly from "workflow-tools/feedback" must agree on the
    // same physical store.
    let from_root = memory_kernel::discover_stores(&meta_workspace);
    let from_submodule = memory_kernel::discover_stores(&workflow_tools_feedback);

    let feedback_store_from_root = from_root
        .iter()
        .find(|store| store.kind == ContentKind::Feedback)
        .expect("root discovery finds a feedback store");
    let feedback_store_from_submodule = from_submodule
        .iter()
        .find(|store| store.kind == ContentKind::Feedback)
        .expect("submodule discovery finds a feedback store");

    let canonical_root_a =
        fs::canonicalize(&feedback_store_from_root.store_root).unwrap();
    let canonical_root_b =
        fs::canonicalize(&feedback_store_from_submodule.store_root).unwrap();
    assert_eq!(
        canonical_root_a, canonical_root_b,
        "root and submodule discovery must resolve to the same physical store_root"
    );

    let store_from_root = CanonicalFeedbackStore::new(canonical_root_a.clone());
    let store_from_submodule = CanonicalFeedbackStore::new(canonical_root_b.clone());
    let tuples_from_root = discovery_tuples(&store_from_root);
    let tuples_from_submodule = discovery_tuples(&store_from_submodule);
    assert_eq!(
        tuples_from_root, tuples_from_submodule,
        "(entry_id, canonical_path, digest) tuples must match between discovery vantage points"
    );

    // Roll back and compare checksums/chronology before and after.
    let journal_id = outcome.journal.id;
    let rollback_outcome = source_store.rollback_move_set(journal_id).unwrap();
    assert!(rollback_outcome.journal.rollback_completed_entity_ids.contains(&entity_id));
    let restored = source_store
        .read_entity(&entity_id)
        .unwrap()
        .expect("entity restored to the source after rollback");
    assert_eq!(restored.digest, source_digest, "rollback must restore the exact digest");
    assert_eq!(
        restored.legacy_append_ordinal, 0,
        "rollback must restore the exact chronology ordinal"
    );
    assert!(
        canonical_container_store.read_entity(&entity_id).unwrap().is_none(),
        "destination entity removed after rollback"
    );
}

fn discovery_tuples(
    store: &CanonicalFeedbackStore
) -> Vec<(Uuid, PathBuf, String)> {
    store
        .list_entities()
        .unwrap()
        .into_iter()
        .map(|entity| {
            (
                entity.id,
                store.entity_path(&entity.id),
                entity.digest,
            )
        })
        .collect()
}
