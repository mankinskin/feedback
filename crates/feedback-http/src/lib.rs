use std::{
    net::SocketAddr,
    path::PathBuf,
    str::FromStr,
    sync::Arc,
};

use axum::{
    Json,
    Router,
    extract::State,
    routing::{
        get,
        post,
    },
};
use feedback_api::{
    EntityFeedbackStore,
    EntityUrn,
    FeedbackEntry,
    FeedbackNoteKind,
    FeedbackProvenance,
    FeedbackRating,
    FeedbackSource,
};
use serde::{
    Deserialize,
    Serialize,
};

#[derive(Clone)]
pub struct AppState {
    pub store_root: PathBuf,
    pub workspace_slug: String,
}

#[derive(Debug, Deserialize)]
pub struct IngestRequest {
    pub workspace: Option<String>,
    pub workspace_slug: Option<String>,
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
    pub workspace_slug: Option<String>,
    pub target: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

fn store_for(
    state: &AppState,
    workspace: Option<&str>,
    workspace_slug: Option<&str>,
) -> Result<EntityFeedbackStore, String> {
    let root = if let Some(workspace) = workspace {
        let workspace =
            memory_kernel::workspace::validate_explicit_workspace_selector(
                Some(workspace),
            )
            .map_err(|err| err.to_string())?;
        memory_kernel::workspace::resolve_store_root_from(
            std::path::Path::new(workspace),
            ".feedback",
        )
    } else {
        state.store_root.clone()
    };
    let slug = workspace_slug
        .map(str::to_string)
        .unwrap_or_else(|| state.workspace_slug.clone());
    EntityFeedbackStore::new(root, slug)
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
) -> Result<Json<FeedbackEntry>, (axum::http::StatusCode, Json<ErrorResponse>)>
{
    let store = store_for(
        &state,
        req.workspace.as_deref(),
        req.workspace_slug.as_deref(),
    )
    .map_err(invalid)?;
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
    let provenance = FeedbackProvenance::new(req.session_id, req.author, None)
        .map_err(invalid)?;
    let entry = FeedbackEntry::new(
        source, target, rating, req.note, note_kind, provenance,
    )
    .map_err(invalid)?;
    let persisted = store.record_entry(entry).map_err(internal)?;
    Ok(Json(persisted))
}

async fn inbox(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> Result<
    Json<Vec<FeedbackEntry>>,
    (axum::http::StatusCode, Json<ErrorResponse>),
> {
    let store = store_for(
        &state,
        req.workspace.as_deref(),
        req.workspace_slug.as_deref(),
    )
    .map_err(invalid)?;
    let target = EntityUrn::from_str(&req.target).map_err(invalid)?;
    let entries = store.entries_for(&target).map_err(internal)?;
    Ok(Json(entries))
}

async fn summary(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> Result<
    Json<serde_json::Value>,
    (axum::http::StatusCode, Json<ErrorResponse>),
> {
    let store = store_for(
        &state,
        req.workspace.as_deref(),
        req.workspace_slug.as_deref(),
    )
    .map_err(invalid)?;
    let target = EntityUrn::from_str(&req.target).map_err(invalid)?;
    let summary = store.summary_for(&target).map_err(internal)?;
    Ok(Json(serde_json::json!(summary)))
}

async fn mine(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<FeedbackEntry>, (axum::http::StatusCode, Json<ErrorResponse>)>
{
    let store = store_for(
        &state,
        req.workspace.as_deref(),
        req.workspace_slug.as_deref(),
    )
    .map_err(invalid)?;
    let target = EntityUrn::from_str(&req.target).map_err(invalid)?;
    let entry = FeedbackEntry::new(
        FeedbackSource::TranscriptMined,
        target,
        Some(FeedbackRating::Mixed),
        Some("transcript-mined signal".to_string()),
        Some(FeedbackNoteKind::Suggestion),
        FeedbackProvenance::new(None, Some("feedback-http".to_string()), None)
            .map_err(invalid)?,
    )
    .map_err(invalid)?;
    let persisted = store.record_entry(entry).map_err(internal)?;
    Ok(Json(persisted))
}

fn invalid(
    err: impl ToString
) -> (axum::http::StatusCode, Json<ErrorResponse>) {
    (
        axum::http::StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: err.to_string(),
        }),
    )
}

fn internal(
    err: impl ToString
) -> (axum::http::StatusCode, Json<ErrorResponse>) {
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: err.to_string(),
        }),
    )
}

pub async fn run(
    state: AppState,
    addr: SocketAddr,
) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app(state)).await
}
