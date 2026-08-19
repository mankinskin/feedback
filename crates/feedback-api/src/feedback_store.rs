use super::*;

impl EntityFeedbackStore {
    pub fn new(
        root: impl Into<PathBuf>,
        workspace_slug: impl Into<String>,
    ) -> Result<Self, String> {
        let workspace_slug =
            normalize_required(workspace_slug.into(), "workspace_slug")?;
        Ok(Self {
            root: root.into(),
            workspace_slug,
        })
    }

    pub fn workspace_slug(&self) -> &str {
        &self.workspace_slug
    }

    pub fn record_usage(
        &self,
        urn: EntityUrn,
    ) -> Result<EntityUsageEvent, String> {
        self.ensure_workspace_urn(&urn)?;
        let event = EntityUsageEvent::new(urn);
        append_ndjson(&self.usage_events_path(), &event)?;
        Ok(event)
    }

    pub fn record_rating(
        &self,
        urn: EntityUrn,
        input: EntityRatingInput,
    ) -> Result<EntityRatingEvent, String> {
        self.ensure_workspace_urn(&urn)?;
        let event = input.into_event(urn);
        append_ndjson(&self.rating_events_path(), &event)?;
        Ok(event)
    }

    /// Ingest a usage event attributed to a classified author. Author metadata
    /// is normalized by [`IngestAuthor`] before the event is persisted.
    pub fn ingest_usage(
        &self,
        author: &IngestAuthor,
        urn: EntityUrn,
    ) -> Result<EntityUsageEvent, String> {
        self.ensure_workspace_urn(&urn)?;
        let event = EntityUsageEvent::with_author(urn, author);
        append_ndjson(&self.usage_events_path(), &event)?;
        Ok(event)
    }

    /// Ingest a rating event attributed to a classified author. The author
    /// identity backfills a missing `agent_or_user_id` *before* the paired
    /// `session_id`/`agent_or_user_id` invariant is validated, so a caller may
    /// submit a `session_id` with an authenticated author and omit the actor id.
    pub fn ingest_rating(
        &self,
        author: &IngestAuthor,
        urn: EntityUrn,
        submission: EntityRatingSubmission,
    ) -> Result<EntityRatingEvent, String> {
        self.ensure_workspace_urn(&urn)?;

        let note_text = normalize_optional(submission.note_text);
        let session_id = normalize_optional(submission.session_id);
        // The author is always the attributable actor: backfill a missing
        // `agent_or_user_id` from the author identity before validation.
        let agent_or_user_id = normalize_optional(submission.agent_or_user_id)
            .or_else(|| author.id().map(str::to_string));
        let note_kind =
            resolve_note_kind(note_text.as_deref(), submission.note_kind)?;

        // A session reference still requires an attributable actor. Unlike the
        // direct (author-less) rating path, an actor without a session is
        // valid here because the author is the actor.
        if session_id.is_some() && agent_or_user_id.is_none() {
            return Err(
                "feedback session references require session_id and agent_or_user_id together"
                    .to_string(),
            );
        }

        let input = EntityRatingInput {
            rating: submission.rating,
            note_text,
            note_kind,
            session_id,
            agent_or_user_id,
        };
        let event = input.into_event_with_author(urn, author);
        append_ndjson(&self.rating_events_path(), &event)?;
        Ok(event)
    }

    pub fn record_entry(
        &self,
        entry: FeedbackEntry,
    ) -> Result<FeedbackEntry, String> {
        self.ensure_workspace_urn(&entry.target)?;
        append_ndjson(&self.entries_path(), &entry)?;
        Ok(entry)
    }

    pub fn entries_for(
        &self,
        urn: &EntityUrn,
    ) -> Result<Vec<FeedbackEntry>, String> {
        self.ensure_workspace_urn(urn)?;
        let mut entries: Vec<FeedbackEntry> =
            read_ndjson::<FeedbackEntry>(&self.entries_path())?
                .into_iter()
                .filter(|item| &item.target == urn)
                .collect();
        entries.sort_by(|left, right| {
            left.provenance
                .executed_at
                .cmp(&right.provenance.executed_at)
        });
        Ok(entries)
    }

    /// Apply a retention policy to the persisted usage and rating logs using
    /// the current time as the age reference.
    pub fn apply_retention(
        &self,
        policy: &RetentionPolicy,
    ) -> Result<RetentionOutcome, String> {
        self.apply_retention_at(policy, Utc::now())
    }

    /// Apply a retention policy relative to an explicit reference time. The
    /// logs are rewritten in chronological order, keeping only events allowed
    /// by the policy. Applying the same policy twice removes nothing.
    pub fn apply_retention_at(
        &self,
        policy: &RetentionPolicy,
        now: chrono::DateTime<Utc>,
    ) -> Result<RetentionOutcome, String> {
        let usage = prune_ndjson::<EntityUsageEvent>(
            &self.usage_events_path(),
            policy,
            now,
            |event| &event.timestamp,
        )?;
        let rating = prune_ndjson::<EntityRatingEvent>(
            &self.rating_events_path(),
            policy,
            now,
            |event| &event.timestamp,
        )?;

        Ok(RetentionOutcome { usage, rating })
    }

    pub fn summary_for(
        &self,
        urn: &EntityUrn,
    ) -> Result<EntityFeedbackSummary, String> {
        self.ensure_workspace_urn(urn)?;
        Ok(self.load_core()?.summary_for(urn))
    }

    pub fn entities_by_usage_frequency(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<EntityFeedbackSummary>, String> {
        Ok(self.load_core()?.entities_by_usage_frequency(limit))
    }

    pub fn low_rated_entities(
        &self
    ) -> Result<Vec<EntityFeedbackSummary>, String> {
        Ok(self.load_core()?.low_rated_entities())
    }

    pub fn unresolved_note_entities(
        &self
    ) -> Result<Vec<EntityFeedbackSummary>, String> {
        Ok(self.load_core()?.unresolved_note_entities())
    }

    fn load_core(&self) -> Result<EntityFeedbackCore, String> {
        let usage_events: Vec<EntityUsageEvent> =
            read_ndjson(&self.usage_events_path())?;
        let rating_events: Vec<EntityRatingEvent> =
            read_ndjson(&self.rating_events_path())?;

        // Validate rating invariants on read without discarding the stored
        // timestamp or author metadata.
        for event in &rating_events {
            EntityRatingInput::new(
                event.rating,
                event.note_text.clone(),
                event.note_kind,
                event.session_id.clone(),
                event.agent_or_user_id.clone(),
            )?;
        }

        Ok(EntityFeedbackCore {
            usage_events,
            rating_events,
        })
    }

    fn workspace_root(&self) -> PathBuf {
        self.root.join(&self.workspace_slug)
    }

    fn feedback_dir(&self) -> PathBuf {
        self.workspace_root().join(FEEDBACK_CORE_DIR)
    }

    pub(super) fn usage_events_path(&self) -> PathBuf {
        self.feedback_dir().join(FEEDBACK_CORE_USAGE_FILE)
    }

    pub(super) fn rating_events_path(&self) -> PathBuf {
        self.feedback_dir().join(FEEDBACK_CORE_RATING_FILE)
    }

    pub(super) fn entries_path(&self) -> PathBuf {
        self.feedback_dir().join(FEEDBACK_ENTRY_FILE)
    }

    fn ensure_workspace_urn(
        &self,
        urn: &EntityUrn,
    ) -> Result<(), String> {
        if urn.workspace() == self.workspace_slug() {
            Ok(())
        } else {
            Err(format!(
                "entity urn workspace '{}' does not match store workspace '{}': {}",
                urn.workspace(),
                self.workspace_slug(),
                urn
            ))
        }
    }
}

impl EntityFeedbackCore {
    pub fn record_usage(
        &mut self,
        urn: EntityUrn,
    ) -> EntityUsageEvent {
        let event = EntityUsageEvent::new(urn);
        self.usage_events.push(event.clone());
        event
    }

    pub fn record_rating(
        &mut self,
        urn: EntityUrn,
        input: EntityRatingInput,
    ) -> EntityRatingEvent {
        let event = input.into_event(urn);
        self.rating_events.push(event.clone());
        event
    }

    pub fn summary_for(
        &self,
        urn: &EntityUrn,
    ) -> EntityFeedbackSummary {
        let mut summary = EntityFeedbackSummary::new(urn.clone());

        for event in self.usage_events.iter().filter(|e| &e.urn == urn) {
            summary.usage_count += 1;
            summary.last_used_at = Some(event.timestamp.clone());
        }
        for event in self.rating_events.iter().filter(|e| &e.urn == urn) {
            match event.rating {
                FeedbackRating::Helpful => summary.helpful_count += 1,
                FeedbackRating::Mixed => summary.mixed_count += 1,
                FeedbackRating::NotHelpful => summary.not_helpful_count += 1,
            }
            if event.has_note() {
                summary.note_count += 1;
                summary.unresolved_count += 1;
            }
            summary.last_rated_at = Some(event.timestamp.clone());
        }

        summary
    }

    pub fn entities_by_usage_frequency(
        &self,
        limit: Option<usize>,
    ) -> Vec<EntityFeedbackSummary> {
        let mut summaries = self.collect_all_summaries();
        summaries.sort_by(|left, right| {
            right
                .usage_count
                .cmp(&left.usage_count)
                .then_with(|| left.urn.as_str().cmp(&right.urn.as_str()))
        });
        if let Some(limit) = limit {
            summaries.truncate(limit);
        }
        summaries
    }

    pub fn low_rated_entities(&self) -> Vec<EntityFeedbackSummary> {
        self.collect_all_summaries()
            .into_iter()
            .filter(EntityFeedbackSummary::has_low_rating)
            .collect()
    }

    pub fn unresolved_note_entities(&self) -> Vec<EntityFeedbackSummary> {
        self.collect_all_summaries()
            .into_iter()
            .filter(EntityFeedbackSummary::has_unresolved_notes)
            .collect()
    }

    fn collect_all_summaries(&self) -> Vec<EntityFeedbackSummary> {
        let mut index: HashMap<EntityUrn, EntityFeedbackSummary> =
            HashMap::new();

        for event in &self.usage_events {
            let entry = index.entry(event.urn.clone()).or_insert_with(|| {
                EntityFeedbackSummary::new(event.urn.clone())
            });
            entry.usage_count += 1;
            entry.last_used_at = Some(event.timestamp.clone());
        }

        for event in &self.rating_events {
            let entry = index.entry(event.urn.clone()).or_insert_with(|| {
                EntityFeedbackSummary::new(event.urn.clone())
            });
            match event.rating {
                FeedbackRating::Helpful => entry.helpful_count += 1,
                FeedbackRating::Mixed => entry.mixed_count += 1,
                FeedbackRating::NotHelpful => entry.not_helpful_count += 1,
            }
            if event.has_note() {
                entry.note_count += 1;
                entry.unresolved_count += 1;
            }
            entry.last_rated_at = Some(event.timestamp.clone());
        }

        let mut summaries: Vec<_> = index.into_values().collect();
        summaries
            .sort_by(|left, right| left.urn.as_str().cmp(&right.urn.as_str()));
        summaries
    }
}
