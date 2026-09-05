use std::{path::PathBuf, str::FromStr};

use chrono::{DateTime, Utc};
use feedback_api::{
    EntityFeedbackStore, EntityUrn, FeedbackEntry, FeedbackNoteKind, FeedbackProvenance,
    FeedbackRating, FeedbackSource, canonical::CanonicalFeedbackStore,
};
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars::{self, JsonSchema},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IngestInput {
    /// Concrete workspace path, repo root, .feedback store path, or path inside that store. Do not use omitted, empty, 'default', or '..' for entity creation; use '.' explicitly to target the MCP server process's current working directory.
    pub workspace: String,
    pub workspace_slug: String,
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub rating: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub note_kind: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryInput {
    /// Concrete workspace path, repo root, .feedback store path, or path inside that store. Do not use omitted, empty, 'default', or '..' for entity creation; use '.' explicitly to target the MCP server process's current working directory.
    pub workspace: String,
    pub workspace_slug: String,
    pub target: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnalyticsInput {
    /// Concrete workspace path, repo root, .feedback store path, or path inside that store.
    pub workspace: String,
    pub workspace_slug: String,
    /// RFC3339 assessment time; defaults to the current time.
    #[serde(default)]
    pub assessed_at: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FeedbackMoveInput {
    /// Physical workspace root that owns the source canonical feedback store.
    pub workspace: String,
    /// Canonical feedback entity UUIDs to move.
    pub ids: Vec<String>,
    /// Destination workspace root.
    pub to_workspace_root: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FeedbackMoveJournalInput {
    /// Physical workspace root that owns the source canonical feedback store.
    pub workspace: String,
    /// Move-set journal UUID.
    pub id: String,
}

#[derive(Clone)]
pub struct FeedbackServer {
    tool_router: ToolRouter<Self>,
}

impl FeedbackServer {
    pub fn new(_store_root: PathBuf, _workspace_slug: String) -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    fn store_for(
        &self,
        workspace: &str,
        workspace_slug: &str,
    ) -> Result<EntityFeedbackStore, McpError> {
        let workspace =
            memory_kernel::workspace::validate_explicit_workspace_selector(Some(workspace))
                .map_err(|err| McpError::invalid_params(err.to_string(), None))?;
        let root = memory_kernel::workspace::resolve_store_root_from(
            std::path::Path::new(workspace),
            ".feedback",
        );
        EntityFeedbackStore::new(root, workspace_slug.to_string())
            .map_err(|err| McpError::invalid_params(err, None))
    }

    fn json_result<T: Serialize>(value: &T) -> Result<CallToolResult, McpError> {
        let text = serde_json::to_string(value)
            .map_err(|err| McpError::internal_error(format!("serialization: {err}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    fn canonical_store_for(&self, workspace: &str) -> Result<CanonicalFeedbackStore, McpError> {
        let workspace =
            memory_kernel::workspace::validate_explicit_workspace_selector(Some(workspace))
                .map_err(|err| McpError::invalid_params(err.to_string(), None))?;
        Ok(CanonicalFeedbackStore::open(std::path::Path::new(
            workspace,
        )))
    }

    fn parse_ids(ids: &[String]) -> Result<Vec<uuid::Uuid>, McpError> {
        ids.iter()
            .map(|id| {
                id.parse::<uuid::Uuid>().map_err(|err| {
                    McpError::invalid_params(
                        format!("invalid feedback entity UUID '{id}': {err}"),
                        None,
                    )
                })
            })
            .collect()
    }
}

#[tool_router]
impl FeedbackServer {
    #[tool(
        name = "feedback_ingest",
        description = "Persist a feedback entry in the feedback-api store."
    )]
    pub async fn feedback_ingest(
        &self,
        Parameters(input): Parameters<IngestInput>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.store_for(&input.workspace, &input.workspace_slug)?;
        let source = FeedbackSource::from_str(&input.source)
            .map_err(|err| McpError::invalid_params(err, None))?;
        let target = EntityUrn::from_str(&input.target)
            .map_err(|err| McpError::invalid_params(err, None))?;
        let rating = input
            .rating
            .map(|value| FeedbackRating::from_str(&value))
            .transpose()
            .map_err(|err| McpError::invalid_params(err, None))?;
        let note_kind = input
            .note_kind
            .map(|value| FeedbackNoteKind::from_str(&value))
            .transpose()
            .map_err(|err| McpError::invalid_params(err, None))?;
        let provenance = FeedbackProvenance::new(input.session_id, input.author, None)
            .map_err(|err| McpError::invalid_params(err, None))?;
        let entry = FeedbackEntry::new(source, target, rating, input.note, note_kind, provenance)
            .map_err(|err| McpError::invalid_params(err, None))?;
        let persisted = store
            .record_entry(entry)
            .map_err(|err| McpError::internal_error(err, None))?;
        Self::json_result(&persisted)
    }

    #[tool(
        name = "feedback_inbox",
        description = "List persisted feedback entries for a target entity URN."
    )]
    pub async fn feedback_inbox(
        &self,
        Parameters(input): Parameters<QueryInput>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.store_for(&input.workspace, &input.workspace_slug)?;
        let target = EntityUrn::from_str(&input.target)
            .map_err(|err| McpError::invalid_params(err, None))?;
        let entries = store
            .entries_for(&target)
            .map_err(|err| McpError::internal_error(err, None))?;
        Self::json_result(&entries)
    }

    #[tool(
        name = "feedback_query",
        description = "Alias for feedback_inbox; lists entries for a target URN."
    )]
    pub async fn feedback_query(
        &self,
        Parameters(input): Parameters<QueryInput>,
    ) -> Result<CallToolResult, McpError> {
        self.feedback_inbox(Parameters(input)).await
    }

    #[tool(
        name = "feedback_summary",
        description = "Return aggregate usage/rating summary for a target entity URN."
    )]
    pub async fn feedback_summary(
        &self,
        Parameters(input): Parameters<QueryInput>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.store_for(&input.workspace, &input.workspace_slug)?;
        let target = EntityUrn::from_str(&input.target)
            .map_err(|err| McpError::invalid_params(err, None))?;
        let summary = store
            .summary_for(&target)
            .map_err(|err| McpError::internal_error(err, None))?;
        Self::json_result(&summary)
    }

    #[tool(
        name = "feedback_analytics",
        description = "Return a read-only current-schema feedback analytics report."
    )]
    pub async fn feedback_analytics(
        &self,
        Parameters(input): Parameters<AnalyticsInput>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.store_for(&input.workspace, &input.workspace_slug)?;
        let assessed_at = input
            .assessed_at
            .map(|value| {
                DateTime::parse_from_rfc3339(&value)
                    .map(|timestamp| timestamp.with_timezone(&Utc))
                    .map_err(|err| {
                        McpError::invalid_params(
                            format!("invalid assessed_at RFC3339 timestamp: {err}"),
                            None,
                        )
                    })
            })
            .transpose()?
            .unwrap_or_else(Utc::now);
        let report = store
            .analytics_at(assessed_at)
            .map_err(|err| McpError::internal_error(err, None))?;
        Self::json_result(&report)
    }

    #[tool(
        name = "feedback_mine",
        description = "Persist a transcript-mined feedback entry from supplied note text and target URN."
    )]
    pub async fn feedback_mine(
        &self,
        Parameters(input): Parameters<QueryInput>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.store_for(&input.workspace, &input.workspace_slug)?;
        let target = EntityUrn::from_str(&input.target)
            .map_err(|err| McpError::invalid_params(err, None))?;
        let entry = FeedbackEntry::new(
            FeedbackSource::TranscriptMined,
            target,
            Some(FeedbackRating::Mixed),
            Some("transcript-mined signal".to_string()),
            Some(FeedbackNoteKind::Suggestion),
            FeedbackProvenance::new(None, Some("feedback-mcp".to_string()), None)
                .map_err(|err| McpError::invalid_params(err, None))?,
        )
        .map_err(|err| McpError::invalid_params(err, None))?;
        let persisted = store
            .record_entry(entry)
            .map_err(|err| McpError::internal_error(err, None))?;
        Self::json_result(&persisted)
    }

    #[tool(
        name = "feedback_move_preflight",
        description = "Read-only preflight plan for moving a set of canonical feedback entities to another workspace store."
    )]
    pub async fn feedback_move_preflight(
        &self,
        Parameters(input): Parameters<FeedbackMoveInput>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.canonical_store_for(&input.workspace)?;
        let ids = Self::parse_ids(&input.ids)?;
        let target_workspace_root = std::path::PathBuf::from(&input.to_workspace_root);
        let plan = store
            .plan_move_set(&ids, &target_workspace_root)
            .map_err(|err| McpError::internal_error(err, None))?;
        Self::json_result(&serde_json::json!({
            "command": "move",
            "status": if plan.supported() { "ok" } else { "blocked" },
            "mode": "preflight",
            "entity_ids": plan.entity_ids,
            "target_store_root": plan.target_store_root.display().to_string(),
            "blockers": plan.blockers(),
        }))
    }

    #[tool(
        name = "feedback_move_apply",
        description = "Execute a supported feedback entity-set move to another workspace store."
    )]
    pub async fn feedback_move_apply(
        &self,
        Parameters(input): Parameters<FeedbackMoveInput>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.canonical_store_for(&input.workspace)?;
        let ids = Self::parse_ids(&input.ids)?;
        let target_workspace_root = std::path::PathBuf::from(&input.to_workspace_root);
        let plan = store
            .plan_move_set(&ids, &target_workspace_root)
            .map_err(|err| McpError::internal_error(err, None))?;
        if !plan.supported() {
            return Err(McpError::invalid_params(
                "move preflight blocked; run feedback_move_preflight for details",
                None,
            ));
        }
        let outcome = store
            .execute_move_set(&plan)
            .map_err(|err| McpError::internal_error(err, None))?;
        Self::json_result(&serde_json::json!({
            "command": "move",
            "status": "ok",
            "mode": "apply",
            "journal_id": outcome.journal.id,
            "phase": outcome.journal.phase,
            "entity_ids": outcome.entity_ids,
        }))
    }

    #[tool(
        name = "feedback_move_resume",
        description = "Resume an interrupted feedback entity-set move from its journal id."
    )]
    pub async fn feedback_move_resume(
        &self,
        Parameters(input): Parameters<FeedbackMoveJournalInput>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.canonical_store_for(&input.workspace)?;
        let journal_id = input.id.parse::<uuid::Uuid>().map_err(|err| {
            McpError::invalid_params(format!("invalid journal UUID: {err}"), None)
        })?;
        let outcome = store
            .resume_move_set(journal_id)
            .map_err(|err| McpError::internal_error(err, None))?;
        Self::json_result(&serde_json::json!({
            "command": "move",
            "status": "ok",
            "mode": "resume",
            "journal_id": outcome.journal.id,
            "phase": outcome.journal.phase,
            "completed_entity_ids": outcome.journal.completed_entity_ids,
        }))
    }

    #[tool(
        name = "feedback_move_rollback",
        description = "Roll back a completed or partially completed feedback entity-set move."
    )]
    pub async fn feedback_move_rollback(
        &self,
        Parameters(input): Parameters<FeedbackMoveJournalInput>,
    ) -> Result<CallToolResult, McpError> {
        let store = self.canonical_store_for(&input.workspace)?;
        let journal_id = input.id.parse::<uuid::Uuid>().map_err(|err| {
            McpError::invalid_params(format!("invalid journal UUID: {err}"), None)
        })?;
        let outcome = store
            .rollback_move_set(journal_id)
            .map_err(|err| McpError::internal_error(err, None))?;
        Self::json_result(&serde_json::json!({
            "command": "move",
            "status": "ok",
            "mode": "rollback",
            "journal_id": outcome.journal.id,
            "phase": outcome.journal.phase,
            "rollback_completed_entity_ids": outcome.journal.rollback_completed_entity_ids,
        }))
    }
}

#[tool_handler]
impl ServerHandler for FeedbackServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            server_info: Implementation {
                name: env!("CARGO_PKG_NAME").to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                ..Default::default()
            },
            instructions: Some(
                "Feedback MCP server. Use feedback_ingest, feedback_inbox/query, feedback_mine, feedback_summary, and feedback_move_* for entity-set moves."
                    .to_string(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

pub async fn run_mcp_server(
    store_root: PathBuf,
    workspace_slug: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let service = FeedbackServer::new(store_root, workspace_slug)
        .serve(stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    fn result_json(result: CallToolResult) -> serde_json::Value {
        let text = result
            .content
            .iter()
            .find_map(|content| match &content.raw {
                RawContent::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .expect("text result");
        serde_json::from_str(text).expect("JSON result")
    }

    #[test]
    fn advertises_tools_capability() {
        let server = FeedbackServer::new(PathBuf::new(), "default".to_string());

        assert!(server.get_info().capabilities.tools.is_some());
    }

    #[test]
    fn workspace_validation_rejects_ambient_aliases() {
        for value in [None, Some(""), Some("default"), Some("..")] {
            let err = memory_kernel::workspace::validate_explicit_workspace_selector(value)
                .expect_err("should reject ambient selector");
            let err_msg = err.to_string();
            assert!(
                err_msg.contains("invalid workspace selector"),
                "error should mention 'invalid workspace selector': {err_msg}"
            );
            assert!(
                err_msg.contains("entity creation requires an explicit workspace path"),
                "error should state the requirement: {err_msg}"
            );
        }
    }

    #[test]
    fn workspace_validation_accepts_current_directory() {
        memory_kernel::workspace::validate_explicit_workspace_selector(Some("."))
            .expect("'.' should resolve to the MCP server's cwd");
    }

    #[tokio::test]
    async fn analytics_tool_returns_shared_report_and_rejects_invalid_time() {
        let directory = tempfile::tempdir().unwrap();
        let store =
            EntityFeedbackStore::new(directory.path().join(".feedback"), "default").unwrap();
        let entry = FeedbackEntry::new(
            FeedbackSource::Agent,
            EntityUrn::rule("default", "rule-a").unwrap(),
            Some(FeedbackRating::NotHelpful),
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
        store.record_entry(entry).unwrap();
        let server = FeedbackServer::new(PathBuf::new(), "default".to_string());
        let input = AnalyticsInput {
            workspace: directory.path().to_string_lossy().to_string(),
            workspace_slug: "default".to_string(),
            assessed_at: Some(
                Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0)
                    .unwrap()
                    .to_rfc3339(),
            ),
        };

        let report = result_json(server.feedback_analytics(Parameters(input)).await.unwrap());

        assert_eq!(report["valid_event_count"], 1);
        assert_eq!(report["rating_distribution"]["not-helpful"], 1);
        let error = server
            .feedback_analytics(Parameters(AnalyticsInput {
                workspace: directory.path().to_string_lossy().to_string(),
                workspace_slug: "default".to_string(),
                assessed_at: Some("not-a-time".to_string()),
            }))
            .await
            .expect_err("invalid time must fail");
        assert!(
            error
                .to_string()
                .contains("invalid assessed_at RFC3339 timestamp")
        );
    }
}

#[cfg(test)]
#[path = "server_move_tests.rs"]
mod move_tests;
