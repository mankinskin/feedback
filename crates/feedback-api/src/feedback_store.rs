use super::*;

impl EntityFeedbackStore {
    /// Build a store from an already-resolved `.feedback` directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Persist one canonical feedback entry.
    pub fn record_entry(&self, entry: FeedbackEntry) -> Result<FeedbackEntry, String> {
        canonical::CanonicalFeedbackStore::new(self.root.clone())
            .append_new_entry(entry.target.workspace(), entry.clone())?;
        Ok(entry)
    }

    /// List canonical feedback entries for one target, ordered by creation ordinal.
    pub fn entries_for(&self, urn: &EntityUrn) -> Result<Vec<FeedbackEntry>, String> {
        Ok(canonical::CanonicalFeedbackStore::new(self.root.clone())
            .list_entities()?
            .into_iter()
            .map(|entity| entity.entry)
            .filter(|entry| &entry.target == urn)
            .collect())
    }

    /// Produce a read-only report from canonical feedback entries.
    pub fn analytics_at(
        &self,
        assessed_at: chrono::DateTime<Utc>,
    ) -> Result<analytics::FeedbackAnalyticsReport, String> {
        let entries = canonical::CanonicalFeedbackStore::new(self.root.clone())
            .list_entities()?
            .into_iter()
            .map(|entity| entity.entry)
            .collect::<Vec<_>>();
        Ok(analytics::analyze_feedback_entries(&entries, assessed_at))
    }

    /// Summarize canonical feedback entries for one target.
    pub fn summary_for(&self, urn: &EntityUrn) -> Result<EntityFeedbackSummary, String> {
        let mut summary = EntityFeedbackSummary::new(urn.clone());
        for entry in self.entries_for(urn)? {
            match entry.rating {
                Some(FeedbackRating::Helpful) => summary.helpful_count += 1,
                Some(FeedbackRating::Mixed) => summary.mixed_count += 1,
                Some(FeedbackRating::NotHelpful) => summary.not_helpful_count += 1,
                None => {}
            }
            if entry.note_text.is_some() {
                summary.note_count += 1;
                if !matches!(
                    entry.status,
                    FeedbackStatus::Actioned | FeedbackStatus::Dismissed
                ) {
                    summary.unresolved_count += 1;
                }
            }
            summary.last_rated_at = Some(entry.provenance.executed_at);
        }
        Ok(summary)
    }
}
