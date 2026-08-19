use crate::{
    EntityFeedbackStore,
    EntityRatingEvent,
    EntityRatingSubmission,
    EntityUrn,
    FeedbackNoteKind,
    FeedbackRating,
    IngestAuthor,
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
) -> Result<EntityRatingEvent, String> {
    let author = IngestAuthor::human(Some(submission.user_id.clone()))?;
    let mut rating = EntityRatingSubmission::new(submission.rating);
    rating.note_text = submission.comments;
    rating.note_kind = Some(FeedbackNoteKind::Note);
    rating.session_id =
        Some(format!("frontend-{}", submission.source_frontend));
    rating.agent_or_user_id = Some(submission.user_id);
    store.ingest_rating(&author, submission.target_entity_urn, rating)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_submission_is_persisted_as_rating_event() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = EntityFeedbackStore::new(dir.path(), "test-workspace")
            .expect("store");
        let ticket_urn =
            EntityUrn::ticket("test-workspace", "ticket-456").expect("urn");

        store.record_usage(ticket_urn.clone()).expect("usage");

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
        assert_eq!(summary.usage_count, 1);
    }
}
