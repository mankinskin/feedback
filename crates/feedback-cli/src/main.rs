use std::{
    path::PathBuf,
    str::FromStr,
};

use clap::{
    Parser,
    Subcommand,
};
use feedback_api::{
    EntityFeedbackStore,
    EntityUrn,
    FeedbackEntry,
    FeedbackNoteKind,
    FeedbackProvenance,
    FeedbackRating,
    FeedbackSource,
    IngestAuthor,
};

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
}

fn parse_rating(raw: Option<String>) -> Result<Option<FeedbackRating>, String> {
    raw.map(|value| FeedbackRating::from_str(&value))
        .transpose()
}

fn parse_note_kind(
    raw: Option<String>
) -> Result<Option<FeedbackNoteKind>, String> {
    raw.map(|value| FeedbackNoteKind::from_str(&value))
        .transpose()
}

fn store(
    store_root: PathBuf,
    workspace_slug: String,
) -> Result<EntityFeedbackStore, String> {
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
            let entry = FeedbackEntry::new(
                source, target, rating, note, note_kind, provenance,
            )?;
            let persisted = store.record_entry(entry)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&persisted)
                    .map_err(|err| err.to_string())?
            );
            Ok(())
        },
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
                serde_json::to_string_pretty(&entries)
                    .map_err(|err| err.to_string())?
            );
            Ok(())
        },
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
                serde_json::to_string_pretty(&summary)
                    .map_err(|err| err.to_string())?
            );
            Ok(())
        },
        Command::Mine {
            store_root,
            workspace_slug,
            target,
            transcript,
            author,
        } => {
            let store = store(store_root, workspace_slug.clone())?;
            let target = EntityUrn::from_str(&target)?;
            let author_id =
                author.unwrap_or_else(|| "transcript-miner".to_string());
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
                serde_json::to_string_pretty(&persisted)
                    .map_err(|err| err.to_string())?
            );
            Ok(())
        },
    }
}
