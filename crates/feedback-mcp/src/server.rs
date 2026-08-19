use std::{
    path::PathBuf,
    str::FromStr,
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
use rmcp::{
    ErrorData as McpError,
    ServerHandler,
    ServiceExt,
    handler::server::{
        tool::ToolRouter,
        wrapper::Parameters,
    },
    model::*,
    schemars::{
        self,
        JsonSchema,
    },
    tool,
    tool_handler,
    tool_router,
    transport::stdio,
};
use serde::{
    Deserialize,
    Serialize,
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IngestInput {
    /// Concrete workspace path, repo root, .feedback store path, or path inside that store. Do not use omitted, empty, 'default', '.', or '..' for entity creation.
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
    /// Concrete workspace path, repo root, .feedback store path, or path inside that store. Do not use omitted, empty, 'default', '.', or '..' for entity creation.
    pub workspace: String,
    pub workspace_slug: String,
    pub target: String,
}

#[derive(Clone)]
pub struct FeedbackServer {
    tool_router: ToolRouter<Self>,
}

impl FeedbackServer {
    pub fn new(
        _store_root: PathBuf,
        _workspace_slug: String,
    ) -> Self {
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
            memory_kernel::workspace::validate_explicit_workspace_selector(
                Some(workspace),
            )
            .map_err(|err| McpError::invalid_params(err.to_string(), None))?;
        let root = memory_kernel::workspace::resolve_store_root_from(
            std::path::Path::new(workspace),
            ".feedback",
        );
        EntityFeedbackStore::new(root, workspace_slug.to_string())
            .map_err(|err| McpError::invalid_params(err, None))
    }

    fn json_result<T: Serialize>(
        value: &T
    ) -> Result<CallToolResult, McpError> {
        let text = serde_json::to_string(value).map_err(|err| {
            McpError::internal_error(format!("serialization: {err}"), None)
        })?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
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
        let provenance =
            FeedbackProvenance::new(input.session_id, input.author, None)
                .map_err(|err| McpError::invalid_params(err, None))?;
        let entry = FeedbackEntry::new(
            source, target, rating, input.note, note_kind, provenance,
        )
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
            FeedbackProvenance::new(
                None,
                Some("feedback-mcp".to_string()),
                None,
            )
            .map_err(|err| McpError::invalid_params(err, None))?,
        )
        .map_err(|err| McpError::invalid_params(err, None))?;
        let persisted = store
            .record_entry(entry)
            .map_err(|err| McpError::internal_error(err, None))?;
        Self::json_result(&persisted)
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
                "Feedback MCP server. Use feedback_ingest, feedback_inbox/query, feedback_mine, and feedback_summary tools."
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
    use super::*;

    #[test]
    fn advertises_tools_capability() {
        let server = FeedbackServer::new(PathBuf::new(), "default".to_string());

        assert!(server.get_info().capabilities.tools.is_some());
    }

    #[test]
    fn workspace_validation_rejects_ambient_aliases() {
        for value in [None, Some(""), Some("default"), Some("."), Some("..")] {
            let err =
                memory_kernel::workspace::validate_explicit_workspace_selector(
                    value,
                )
                .expect_err("should reject ambient selector");
            let err_msg = err.to_string();
            assert!(
                err_msg.contains("invalid workspace selector"),
                "error should mention 'invalid workspace selector': {err_msg}"
            );
            assert!(
                err_msg.contains(
                    "entity creation requires an explicit workspace path"
                ),
                "error should state the requirement: {err_msg}"
            );
        }
    }
}
