use crate::{
    EntityFeedbackStore, EntityUrn, FeedbackEntry, FeedbackNoteKind, FeedbackProvenance,
    FeedbackRating, FeedbackSource,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FrontendFeedbackSubmission {
    pub source_frontend: String,
    pub user_id: String,
    pub rating: FeedbackRating,
    pub comments: Option<String>,
    pub target_entity_urn: EntityUrn,
}

pub fn ingest_frontend_feedback(
    store: &EntityFeedbackStore,
    submission: FrontendFeedbackSubmission,
) -> Result<FeedbackEntry, String> {
    let entry = FeedbackEntry::new(
        FeedbackSource::Frontend,
        submission.target_entity_urn,
        Some(submission.rating),
        submission.comments,
        Some(FeedbackNoteKind::Note),
        FeedbackProvenance::new(
            Some(format!("frontend-{}", submission.source_frontend)),
            Some(submission.user_id),
            None,
        )?,
    )?;
    store.record_entry(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_submission_is_persisted_as_rating_event() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = EntityFeedbackStore::new(dir.path());
        let ticket_urn = EntityUrn::ticket("test-workspace", "ticket-456").expect("urn");

        let submission = FrontendFeedbackSubmission {
            source_frontend: "ticket-viewer".to_string(),
            user_id: "user-123".to_string(),
            rating: FeedbackRating::Helpful,
            comments: Some("Ticket resolved perfectly".to_string()),
            target_entity_urn: ticket_urn.clone(),
        };
        ingest_frontend_feedback(&store, submission).expect("ingest");

        let summary = store.summary_for(&ticket_urn).expect("summary");
        assert_eq!(summary.helpful_count, 1);
    }
}
