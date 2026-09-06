use std::process::Command;

use feedback_api::canonical::CanonicalFeedbackStore;
use rmcp::handler::server::wrapper::Parameters;
use serde_json::Value;
use tempfile::TempDir;

use super::{FeedbackMoveInput, FeedbackMoveJournalInput, FeedbackServer};

fn run_git(repo_root: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(repo_root)
        .args(args)
        .status()
        .expect("git command");
    assert!(status.success(), "git {args:?} failed: {status}");
}

fn extract_json(result: rmcp::model::CallToolResult) -> Value {
    let text = result
        .content
        .iter()
        .find_map(|content| {
            if let rmcp::model::RawContent::Text(text) = &content.raw {
                Some(text.text.clone())
            } else {
                None
            }
        })
        .expect("text content");
    serde_json::from_str(&text).expect("parse json")
}

fn seed_entity(source_workspace: &std::path::Path) -> uuid::Uuid {
    use feedback_api::{
        EntityUrn, FeedbackEntry, FeedbackProvenance, FeedbackRating, FeedbackSource,
    };

    let store = CanonicalFeedbackStore::open(source_workspace);
    let target = EntityUrn::ticket("demo", "t").unwrap();
    let provenance = FeedbackProvenance::new(None, Some("a".to_string()), None).unwrap();
    let entry = FeedbackEntry::new(
        FeedbackSource::Agent,
        target,
        Some(FeedbackRating::Helpful),
        None,
        None,
        provenance,
    )
    .unwrap();
    store.append_new_entry("demo", entry).unwrap().id
}

#[tokio::test]
async fn move_preflight_apply_resume_rollback_routes_preserve_journal_semantics() {
    let tmp = TempDir::new().expect("tempdir");
    let repo_root = tmp.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("repo root");
    run_git(&repo_root, &["init"]);

    let source_workspace = repo_root.join("source-workspace");
    let target_workspace = repo_root.join("target-workspace");
    std::fs::create_dir_all(&source_workspace).expect("source workspace");
    std::fs::create_dir_all(target_workspace.join(".feedback")).expect("target feedback dir");

    let entity_id = seed_entity(&source_workspace);
    let server = FeedbackServer::new();

    let preflight = server
        .feedback_move_preflight(Parameters(FeedbackMoveInput {
            workspace: source_workspace.to_string_lossy().to_string(),
            ids: vec![entity_id.to_string()],
            to_workspace_root: target_workspace.to_string_lossy().to_string(),
        }))
        .await
        .expect("feedback move preflight");
    let preflight_json = extract_json(preflight);
    assert_eq!(preflight_json["status"], "ok");
    assert_eq!(preflight_json["mode"], "preflight");

    let apply = server
        .feedback_move_apply(Parameters(FeedbackMoveInput {
            workspace: source_workspace.to_string_lossy().to_string(),
            ids: vec![entity_id.to_string()],
            to_workspace_root: target_workspace.to_string_lossy().to_string(),
        }))
        .await
        .expect("feedback move apply");
    let apply_json = extract_json(apply);
    assert_eq!(apply_json["status"], "ok");
    assert_eq!(apply_json["mode"], "apply");
    let journal_id = apply_json["journal_id"].as_str().unwrap().to_string();

    let resume = server
        .feedback_move_resume(Parameters(FeedbackMoveJournalInput {
            workspace: source_workspace.to_string_lossy().to_string(),
            id: journal_id.clone(),
        }))
        .await
        .expect("feedback move resume");
    let resume_json = extract_json(resume);
    assert_eq!(resume_json["status"], "ok");
    assert_eq!(resume_json["mode"], "resume");

    let rollback = server
        .feedback_move_rollback(Parameters(FeedbackMoveJournalInput {
            workspace: source_workspace.to_string_lossy().to_string(),
            id: journal_id,
        }))
        .await
        .expect("feedback move rollback");
    let rollback_json = extract_json(rollback);
    assert_eq!(rollback_json["status"], "ok");
    assert_eq!(rollback_json["mode"], "rollback");

    let source_store = CanonicalFeedbackStore::open(&source_workspace);
    assert!(
        source_store.read_entity(&entity_id).unwrap().is_some(),
        "rollback must restore the source entity"
    );
    let target_store = CanonicalFeedbackStore::open(&target_workspace);
    assert!(
        target_store.read_entity(&entity_id).unwrap().is_none(),
        "rollback must remove the destination entity"
    );
}
