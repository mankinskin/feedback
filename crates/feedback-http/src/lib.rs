use std::{net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc};

use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use feedback_api::{
    EntityFeedbackStore, EntityUrn, FeedbackEntry, FeedbackNoteKind, FeedbackProvenance,
    FeedbackRating, FeedbackSource,
};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct AppState {
    pub store_root: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct IngestRequest {
    pub workspace: Option<String>,
    pub source: String,
    pub target: String,
    pub rating: Option<String>,
    pub note: Option<String>,
    pub note_kind: Option<String>,
    pub session_id: Option<String>,
    pub author: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct QueryRequest {
    pub workspace: Option<String>,
    pub target: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

fn store_for(state: &AppState, workspace: Option<&str>) -> Result<EntityFeedbackStore, String> {
    let root = if let Some(workspace) = workspace {
        let workspace =
            memory_kernel::workspace::normalize_explicit_workspace_selector(Some(workspace))
                .map_err(|err| err.to_string())?;
        return EntityFeedbackStore::open_read_only(&workspace);
    } else {
        state.store_root.clone()
    };
    Ok(EntityFeedbackStore::new(root))
}

fn store_for_write(
    _state: &AppState,
    workspace: Option<&str>,
) -> Result<EntityFeedbackStore, String> {
    let selector = memory_kernel::workspace::validate_explicit_workspace_selector(workspace)
        .map_err(|err| err.to_string())?;
    EntityFeedbackStore::open(std::path::Path::new(selector))
}

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/api/feedback/ingest", post(ingest))
        .route("/api/feedback/inbox", post(inbox))
        .route("/api/feedback/query", post(inbox))
        .route("/api/feedback/summary", post(summary))
        .route("/api/feedback/mine", post(mine))
        .route("/api/feedback/health", get(health))
        .with_state(Arc::new(state))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status":"ok"}))
}

async fn ingest(
    State(state): State<Arc<AppState>>,
    Json(req): Json<IngestRequest>,
) -> Result<Json<FeedbackEntry>, (axum::http::StatusCode, Json<ErrorResponse>)> {
    let store = store_for_write(&state, req.workspace.as_deref()).map_err(invalid)?;
    let source = FeedbackSource::from_str(&req.source).map_err(invalid)?;
    let target = EntityUrn::from_str(&req.target).map_err(invalid)?;
    let rating = req
        .rating
        .map(|value| FeedbackRating::from_str(&value))
        .transpose()
        .map_err(invalid)?;
    let note_kind = req
        .note_kind
        .map(|value| FeedbackNoteKind::from_str(&value))
        .transpose()
        .map_err(invalid)?;
    let provenance = FeedbackProvenance::new(req.session_id, req.author, None).map_err(invalid)?;
    let entry = FeedbackEntry::new(source, target, rating, req.note, note_kind, provenance)
        .map_err(invalid)?;
    let persisted = store.record_entry(entry).map_err(internal)?;
    Ok(Json(persisted))
}

async fn inbox(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<Vec<FeedbackEntry>>, (axum::http::StatusCode, Json<ErrorResponse>)> {
    let store = store_for(&state, req.workspace.as_deref()).map_err(invalid)?;
    let target = EntityUrn::from_str(&req.target).map_err(invalid)?;
    let entries = store.entries_for(&target).map_err(internal)?;
    Ok(Json(entries))
}

async fn summary(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<ErrorResponse>)> {
    let store = store_for(&state, req.workspace.as_deref()).map_err(invalid)?;
    let target = EntityUrn::from_str(&req.target).map_err(invalid)?;
    let summary = store.summary_for(&target).map_err(internal)?;
    Ok(Json(serde_json::json!(summary)))
}

async fn mine(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<FeedbackEntry>, (axum::http::StatusCode, Json<ErrorResponse>)> {
    let store = store_for_write(&state, req.workspace.as_deref()).map_err(invalid)?;
    let target = EntityUrn::from_str(&req.target).map_err(invalid)?;
    let entry = FeedbackEntry::new(
        FeedbackSource::TranscriptMined,
        target,
        Some(FeedbackRating::Mixed),
        Some("transcript-mined signal".to_string()),
        Some(FeedbackNoteKind::Suggestion),
        FeedbackProvenance::new(None, Some("feedback-http".to_string()), None).map_err(invalid)?,
    )
    .map_err(invalid)?;
    let persisted = store.record_entry(entry).map_err(internal)?;
    Ok(Json(persisted))
}

fn invalid(err: impl ToString) -> (axum::http::StatusCode, Json<ErrorResponse>) {
    (
        axum::http::StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: err.to_string(),
        }),
    )
}

fn internal(err: impl ToString) -> (axum::http::StatusCode, Json<ErrorResponse>) {
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: err.to_string(),
        }),
    )
}

pub async fn run(state: AppState, addr: SocketAddr) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app(state)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ingest_rejects_omitted_and_ambient_workspace_selectors() {
        let state = Arc::new(AppState {
            store_root: PathBuf::from("unused-feedback-fallback"),
        });

        for selector in [None, Some(""), Some("  "), Some("default"), Some("..")] {
            let result = ingest(
                State(state.clone()),
                Json(IngestRequest {
                    workspace: selector.map(str::to_string),
                    source: "agent".to_string(),
                    target: "ce://default/ticket/selector-test".to_string(),
                    rating: None,
                    note: None,
                    note_kind: None,
                    session_id: None,
                    author: None,
                }),
            )
            .await;

            let (status, error) = result.unwrap_err();
            assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
            assert!(error.error.contains("requires an explicit workspace path"));
        }
    }

    #[tokio::test]
    async fn mine_rejects_omitted_and_ambient_workspace_selectors() {
        let state = Arc::new(AppState {
            store_root: PathBuf::from("unused-feedback-fallback"),
        });

        for selector in [None, Some(""), Some("  "), Some("default"), Some("..")] {
            let result = mine(
                State(state.clone()),
                Json(QueryRequest {
                    workspace: selector.map(str::to_string),
                    target: "ce://default/ticket/selector-test".to_string(),
                }),
            )
            .await;

            let (status, error) = result.unwrap_err();
            assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
            assert!(error.error.contains("requires an explicit workspace path"));
        }
    }

    #[tokio::test]
    async fn selected_workspace_ingest_reads_back_only_from_its_canonical_store() {
        let parent = tempfile::tempdir().unwrap();
        let selected = parent.path().join("selected");
        let sibling = parent.path().join("sibling");
        std::fs::create_dir_all(&selected).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        let state = Arc::new(AppState {
            store_root: parent.path().join("ambient-feedback"),
        });
        let target = "ce://default/ticket/http-selected-store";

        let _persisted = ingest(
            State(state.clone()),
            Json(IngestRequest {
                workspace: Some(selected.to_string_lossy().into_owned()),
                source: "agent".to_string(),
                target: target.to_string(),
                rating: Some("helpful".to_string()),
                note: Some("http selected store".to_string()),
                note_kind: None,
                session_id: None,
                author: Some("test".to_string()),
            }),
        )
        .await
        .unwrap();
        let entries = inbox(
            State(state),
            Json(QueryRequest {
                workspace: Some(selected.to_string_lossy().into_owned()),
                target: target.to_string(),
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].note_text.as_deref(), Some("http selected store"));
        assert!(selected.join(".workflow-tools/feedback/entries").is_dir());
        assert!(!parent.path().join(".workflow-tools/feedback").exists());
        assert!(!sibling.join(".workflow-tools/feedback").exists());
    }
}
