use std::{path::PathBuf, str::FromStr};

use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use feedback_api::{
    EntityFeedbackStore, EntityUrn, FeedbackEntry, FeedbackNoteKind, FeedbackProvenance,
    FeedbackRating, FeedbackSource, IngestAuthor, canonical::CanonicalFeedbackStore,
};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "feedback")]
#[command(about = "Feedback CLI over feedback-api store")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Ingest {
        #[arg(long)]
        store_root: PathBuf,
        #[arg(long)]
        workspace_slug: String,
        #[arg(long)]
        source: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        rating: Option<String>,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        note_kind: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        author: Option<String>,
    },
    Inbox {
        #[arg(long)]
        store_root: PathBuf,
        #[arg(long)]
        workspace_slug: String,
        #[arg(long)]
        target: String,
    },
    Summary {
        #[arg(long)]
        store_root: PathBuf,
        #[arg(long)]
        workspace_slug: String,
        #[arg(long)]
        target: String,
    },
    /// Produce a read-only analytics report for current feedback entries.
    Analytics {
        #[arg(long)]
        store_root: PathBuf,
        #[arg(long)]
        workspace_slug: String,
        /// RFC3339 assessment time; defaults to the current time.
        #[arg(long)]
        assessed_at: Option<String>,
        /// Output format: json or text.
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Migrate valid legacy feedback entries to canonical schema storage and
    /// permanently remove the source log, reporting only aggregate discards.
    Cutover {
        #[arg(long)]
        store_root: PathBuf,
        #[arg(long)]
        workspace_slug: String,
    },
    Mine {
        #[arg(long)]
        store_root: PathBuf,
        #[arg(long)]
        workspace_slug: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        transcript: String,
        #[arg(long)]
        author: Option<String>,
    },
    /// Move a normalized set of canonical feedback entities to another
    /// workspace store, or resume/roll back a previously started set move.
    Move {
        /// Physical workspace root that owns the source canonical store.
        #[arg(long)]
        workspace_root: PathBuf,
        /// Comma-separated canonical entity UUIDs to move (required unless
        /// --resume/--rollback is used).
        #[arg(long, value_delimiter = ',')]
        ids: Vec<String>,
        /// Destination workspace root (required in plan/execute mode).
        #[arg(long)]
        to_workspace_root: Option<PathBuf>,
        /// Plan only; do not execute the move.
        #[arg(long)]
        dry_run: bool,
        /// Resume an interrupted set move from a journal UUID.
        #[arg(long)]
        resume: Option<String>,
        /// Roll back a set move from a journal UUID.
        #[arg(long)]
        rollback: Option<String>,
    },
}

fn parse_rating(raw: Option<String>) -> Result<Option<FeedbackRating>, String> {
    raw.map(|value| FeedbackRating::from_str(&value))
        .transpose()
}

fn parse_note_kind(raw: Option<String>) -> Result<Option<FeedbackNoteKind>, String> {
    raw.map(|value| FeedbackNoteKind::from_str(&value))
        .transpose()
}

fn store(store_root: PathBuf, workspace_slug: String) -> Result<EntityFeedbackStore, String> {
    EntityFeedbackStore::new(store_root, workspace_slug)
}

fn main() {
    if let Err(err) = run() {
        eprintln!("feedback: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
    match cli.command {
        Command::Ingest {
            store_root,
            workspace_slug,
            source,
            target,
            rating,
            note,
            note_kind,
            session_id,
            author,
        } => {
            let store = store(store_root, workspace_slug)?;
            let source = FeedbackSource::from_str(&source)?;
            let target = EntityUrn::from_str(&target)?;
            let rating = parse_rating(rating)?;
            let note_kind = parse_note_kind(note_kind)?;
            let provenance = FeedbackProvenance::new(session_id, author, None)?;
            let entry = FeedbackEntry::new(source, target, rating, note, note_kind, provenance)?;
            let persisted = store.record_entry(entry)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&persisted).map_err(|err| err.to_string())?
            );
            Ok(())
        }
        Command::Inbox {
            store_root,
            workspace_slug,
            target,
        } => {
            let store = store(store_root, workspace_slug)?;
            let target = EntityUrn::from_str(&target)?;
            let entries = store.entries_for(&target)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&entries).map_err(|err| err.to_string())?
            );
            Ok(())
        }
        Command::Summary {
            store_root,
            workspace_slug,
            target,
        } => {
            let store = store(store_root, workspace_slug)?;
            let target = EntityUrn::from_str(&target)?;
            let summary = store.summary_for(&target)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&summary).map_err(|err| err.to_string())?
            );
            Ok(())
        }
        Command::Analytics {
            store_root,
            workspace_slug,
            assessed_at,
            format,
        } => {
            let store = store(store_root, workspace_slug)?;
            let assessed_at = parse_assessed_at(assessed_at)?;
            let report = store.analytics_at(assessed_at)?;
            print_analytics(&report, &format)
        }
        Command::Cutover {
            store_root,
            workspace_slug,
        } => {
            let legacy_path = store_root
                .join(&workspace_slug)
                .join("feedback-core")
                .join("entries.ndjson");
            let canonical = CanonicalFeedbackStore::new(store_root);
            let outcome = feedback_api::migration::migrate_and_discard_legacy_ndjson(
                &canonical,
                &workspace_slug,
                &legacy_path,
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "migrated_count": outcome.migrated_count,
                    "discarded_count": outcome.discarded_count,
                }))
                .map_err(|err| err.to_string())?
            );
            Ok(())
        }
        Command::Mine {
            store_root,
            workspace_slug,
            target,
            transcript,
            author,
        } => {
            let store = store(store_root, workspace_slug.clone())?;
            let target = EntityUrn::from_str(&target)?;
            let author_id = author.unwrap_or_else(|| "transcript-miner".to_string());
            let _author = IngestAuthor::privileged_agent(author_id.clone())?;
            let entry = FeedbackEntry::new(
                FeedbackSource::TranscriptMined,
                target,
                Some(FeedbackRating::Mixed),
                Some(transcript),
                Some(FeedbackNoteKind::Suggestion),
                FeedbackProvenance::new(None, Some(author_id), None)?,
            )?;
            let persisted = store.record_entry(entry)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&persisted).map_err(|err| err.to_string())?
            );
            Ok(())
        }
        Command::Move {
            workspace_root,
            ids,
            to_workspace_root,
            dry_run,
            resume,
            rollback,
        } => cmd_move(MoveArgs {
            workspace_root,
            ids,
            to_workspace_root,
            dry_run,
            resume,
            rollback,
        }),
    }
}

fn parse_assessed_at(raw: Option<String>) -> Result<DateTime<Utc>, String> {
    raw.map(|value| {
        DateTime::parse_from_rfc3339(&value)
            .map(|timestamp| timestamp.with_timezone(&Utc))
            .map_err(|err| format!("invalid --assessed-at RFC3339 timestamp: {err}"))
    })
    .transpose()
    .map(|timestamp| timestamp.unwrap_or_else(Utc::now))
}

fn print_analytics(
    report: &feedback_api::analytics::FeedbackAnalyticsReport,
    format: &str,
) -> Result<(), String> {
    match format {
        "json" => println!(
            "{}",
            serde_json::to_string_pretty(report).map_err(|err| err.to_string())?
        ),
        "text" => println!(
            "valid_events: {}\nmalformed_lines: {}\nfirst_observed_at: {}\nlast_observed_at: {}\nincidents: {}",
            report.valid_event_count,
            report.malformed_line_count,
            report
                .first_observed_at
                .map_or_else(|| "none".to_string(), |value| value.to_rfc3339()),
            report
                .last_observed_at
                .map_or_else(|| "none".to_string(), |value| value.to_rfc3339()),
            report.incidents.len(),
        ),
        other => return Err(format!("invalid --format '{other}', expected json or text")),
    }
    Ok(())
}

struct MoveArgs {
    workspace_root: PathBuf,
    ids: Vec<String>,
    to_workspace_root: Option<PathBuf>,
    dry_run: bool,
    resume: Option<String>,
    rollback: Option<String>,
}

fn cmd_move(args: MoveArgs) -> Result<(), String> {
    if args.resume.is_some() && args.rollback.is_some() {
        return Err("move accepts only one of --resume or --rollback".to_string());
    }

    let store = CanonicalFeedbackStore::open(&args.workspace_root);

    if let Some(journal_id) = args.resume.as_deref() {
        let journal_id = journal_id
            .parse::<Uuid>()
            .map_err(|err| format!("invalid --resume journal UUID: {err}"))?;
        let outcome = store.resume_move_set(journal_id)?;
        return print_json(&serde_json::json!({
            "command": "move",
            "status": "ok",
            "mode": "resume",
            "journal_id": outcome.journal.id,
            "phase": outcome.journal.phase,
            "completed_entity_ids": outcome.journal.completed_entity_ids,
        }));
    }

    if let Some(journal_id) = args.rollback.as_deref() {
        let journal_id = journal_id
            .parse::<Uuid>()
            .map_err(|err| format!("invalid --rollback journal UUID: {err}"))?;
        let outcome = store.rollback_move_set(journal_id)?;
        return print_json(&serde_json::json!({
            "command": "move",
            "status": "ok",
            "mode": "rollback",
            "journal_id": outcome.journal.id,
            "phase": outcome.journal.phase,
            "rollback_completed_entity_ids": outcome.journal.rollback_completed_entity_ids,
        }));
    }

    if args.ids.is_empty() {
        return Err("move requires --ids unless --resume/--rollback is used".to_string());
    }
    let to_workspace_root = args
        .to_workspace_root
        .ok_or_else(|| "move requires --to-workspace-root in plan/execute mode".to_string())?;
    let ids = args
        .ids
        .iter()
        .map(|id| {
            id.parse::<Uuid>()
                .map_err(|err| format!("invalid feedback entity UUID '{id}': {err}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let plan = store.plan_move_set(&ids, &to_workspace_root)?;

    if args.dry_run || !plan.supported() {
        return print_json(&serde_json::json!({
            "command": "move",
            "status": if plan.supported() { "ok" } else { "blocked" },
            "mode": "plan",
            "dry_run": true,
            "entity_ids": plan.entity_ids,
            "target_store_root": plan.target_store_root.display().to_string(),
            "blockers": plan.blockers(),
        }));
    }

    let outcome = store.execute_move_set(&plan)?;
    print_json(&serde_json::json!({
        "command": "move",
        "status": "ok",
        "mode": "execute",
        "journal_id": outcome.journal.id,
        "phase": outcome.journal.phase,
        "entity_ids": outcome.entity_ids,
        "completed_entity_ids": outcome.journal.completed_entity_ids,
    }))
}

fn print_json(value: &serde_json::Value) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|err| err.to_string())?
    );
    Ok(())
}

#[cfg(test)]
mod move_cli_tests {
    use super::*;

    fn init_git(root: &std::path::Path) {
        std::fs::create_dir_all(root).unwrap();
        let status = std::process::Command::new("git")
            .current_dir(root)
            .arg("init")
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn move_dry_run_does_not_mutate_either_store() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_git(&repo);
        let source_workspace = repo.join("source");
        let target_workspace = repo.join("target");
        std::fs::create_dir_all(&source_workspace).unwrap();
        std::fs::create_dir_all(target_workspace.join(".feedback")).unwrap();

        let source_store = CanonicalFeedbackStore::open(&source_workspace);
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
        let entity = source_store.append_new_entry("demo", entry).unwrap();

        cmd_move(MoveArgs {
            workspace_root: source_workspace.clone(),
            ids: vec![entity.id.to_string()],
            to_workspace_root: Some(target_workspace.clone()),
            dry_run: true,
            resume: None,
            rollback: None,
        })
        .unwrap();

        assert!(
            source_store.read_entity(&entity.id).unwrap().is_some(),
            "dry run must not remove the source entity"
        );
        let target_store = CanonicalFeedbackStore::open(&target_workspace);
        assert!(
            target_store.read_entity(&entity.id).unwrap().is_none(),
            "dry run must not create a destination entity"
        );
    }

    #[test]
    fn move_apply_resume_and_rollback_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_git(&repo);
        let source_workspace = repo.join("source");
        let target_workspace = repo.join("target");
        std::fs::create_dir_all(&source_workspace).unwrap();
        std::fs::create_dir_all(target_workspace.join(".feedback")).unwrap();

        let source_store = CanonicalFeedbackStore::open(&source_workspace);
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
        let entity = source_store.append_new_entry("demo", entry).unwrap();

        let plan = source_store
            .plan_move_set(&[entity.id], &target_workspace)
            .unwrap();
        let outcome = source_store.execute_move_set(&plan).unwrap();
        let journal_id = outcome.journal.id;

        let target_store = CanonicalFeedbackStore::open(&target_workspace);
        assert!(target_store.read_entity(&entity.id).unwrap().is_some());
        assert!(source_store.read_entity(&entity.id).unwrap().is_none());

        // Resume after a completed apply is idempotent, not a partial
        // success report.
        cmd_move(MoveArgs {
            workspace_root: source_workspace.clone(),
            ids: vec![],
            to_workspace_root: None,
            dry_run: false,
            resume: Some(journal_id.to_string()),
            rollback: None,
        })
        .unwrap();
        assert!(target_store.read_entity(&entity.id).unwrap().is_some());

        cmd_move(MoveArgs {
            workspace_root: source_workspace.clone(),
            ids: vec![],
            to_workspace_root: None,
            dry_run: false,
            resume: None,
            rollback: Some(journal_id.to_string()),
        })
        .unwrap();
        assert!(
            source_store.read_entity(&entity.id).unwrap().is_some(),
            "rollback must restore the source entity"
        );
        assert!(
            target_store.read_entity(&entity.id).unwrap().is_none(),
            "rollback must remove the destination entity"
        );
    }
}
