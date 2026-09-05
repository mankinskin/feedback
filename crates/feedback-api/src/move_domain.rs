//! Feedback-domain adapter onto the domain-neutral move kernel.
//!
//! Only canonical entity folders (see [`crate::canonical`]) participate in a
//! move; the legacy NDJSON backup is never moved. The feedback store has no
//! board or lease model and no cross-entity edges, so those kernel hooks
//! return their empty defaults.

use std::{
    collections::BTreeMap,
    path::{
        Path,
        PathBuf,
    },
};

use memory_kernel::storage::move_kernel::{
    self,
    MoveDomain,
    MoveError,
    MoveOutcome,
    MovePlan,
    MoveReferences,
    MoveResult,
    MoveSetExecutionPhase,
    MoveSetOutcome,
    MoveSetPlan,
    load_move_set_journal,
};
use uuid::Uuid;

use crate::canonical::{
    CanonicalFeedbackStore,
    FEEDBACK_ENTITY_FILE_NAME,
    FEEDBACK_ENTITY_SUBDIR,
    FEEDBACK_STORE_INDEX_DIR,
};

fn from_move_error(error: MoveError) -> String {
    error.to_string()
}

/// Feedback-domain implementation of the move kernel's [`MoveDomain`] trait.
pub struct FeedbackMoveDomain<'a> {
    store: &'a CanonicalFeedbackStore,
}

impl<'a> FeedbackMoveDomain<'a> {
    pub fn new(store: &'a CanonicalFeedbackStore) -> Self {
        Self { store }
    }
}

impl MoveDomain for FeedbackMoveDomain<'_> {
    fn entity_subdir(&self) -> &str {
        FEEDBACK_ENTITY_SUBDIR
    }

    fn store_index_dir(&self) -> &str {
        FEEDBACK_STORE_INDEX_DIR
    }

    fn source_store_root(&self) -> PathBuf {
        self.store.store_root().to_path_buf()
    }

    fn source_entity_path(
        &self,
        entity_id: &Uuid,
    ) -> MoveResult<Option<PathBuf>> {
        let dir = self.store.entity_dir(entity_id);
        Ok(dir.is_dir().then_some(dir))
    }

    fn source_entity_paths_for_set(
        &self,
        entity_ids: &[Uuid],
    ) -> MoveResult<BTreeMap<Uuid, PathBuf>> {
        let mut result = BTreeMap::new();
        for entity_id in entity_ids {
            if let Some(path) = self.source_entity_path(entity_id)? {
                result.insert(*entity_id, path);
            }
        }
        Ok(result)
    }

    fn related_entities(
        &self,
        _entity_id: &Uuid,
    ) -> MoveResult<MoveReferences> {
        Ok(MoveReferences::default())
    }

    fn target_store_present(
        &self,
        target_store_root: &Path,
    ) -> MoveResult<bool> {
        Ok(target_store_root.is_dir())
    }

    fn entity_indexed_in(
        &self,
        store_root: &Path,
        entity_id: &Uuid,
    ) -> MoveResult<bool> {
        Ok(store_root
            .join(FEEDBACK_ENTITY_SUBDIR)
            .join(entity_id.to_string())
            .join(FEEDBACK_ENTITY_FILE_NAME)
            .is_file())
    }

    fn scan_store(
        &self,
        _store_root: &Path,
    ) -> MoveResult<()> {
        // Canonical entities are read directly from disk on every access;
        // there is no cached index to rescan.
        Ok(())
    }
}

impl CanonicalFeedbackStore {
    /// Build a read-only preflight plan for moving one canonical entity to
    /// `target_workspace_root`.
    pub fn plan_move_preflight(
        &self,
        entity_id: &Uuid,
        target_workspace_root: &Path,
    ) -> Result<MovePlan, String> {
        let domain = FeedbackMoveDomain::new(self);
        move_kernel::plan_move(&domain, entity_id, target_workspace_root)
            .map_err(from_move_error)
    }

    /// Build one normalized preflight plan for a set of canonical entities.
    pub fn plan_move_set(
        &self,
        entity_ids: &[Uuid],
        target_workspace_root: &Path,
    ) -> Result<MoveSetPlan, String> {
        let domain = FeedbackMoveDomain::new(self);
        move_kernel::plan_move_set(&domain, entity_ids, target_workspace_root)
            .map_err(from_move_error)
    }

    /// Execute a supported single-entity move with a fresh journal.
    pub fn execute_move_with_journal(
        &self,
        plan: &MovePlan,
    ) -> Result<MoveOutcome, String> {
        let domain = FeedbackMoveDomain::new(self);
        move_kernel::execute_move(&domain, plan).map_err(from_move_error)
    }

    /// Resume an interrupted single-entity move from its journal id.
    pub fn resume_move_with_journal(
        &self,
        journal_id: Uuid,
    ) -> Result<MoveOutcome, String> {
        let domain = FeedbackMoveDomain::new(self);
        move_kernel::resume_move(&domain, journal_id).map_err(from_move_error)
    }

    /// Roll back a single-entity move from its journal id.
    pub fn rollback_move_with_journal(
        &self,
        journal_id: Uuid,
    ) -> Result<MoveOutcome, String> {
        let domain = FeedbackMoveDomain::new(self);
        move_kernel::rollback_move(&domain, journal_id).map_err(from_move_error)
    }

    /// Execute a supported normalized set move with one shared
    /// [`memory_kernel::storage::move_kernel::MoveSetJournal`] lock lifecycle.
    pub fn execute_move_set(
        &self,
        plan: &MoveSetPlan,
    ) -> Result<MoveSetOutcome, String> {
        let domain = FeedbackMoveDomain::new(self);
        move_kernel::execute_move_set(&domain, plan).map_err(from_move_error)
    }

    /// Resume an interrupted set move from its journal id. A journal that
    /// already reached `Validated`/`RolledBack` short-circuits to its
    /// recorded outcome instead of re-entering the kernel's execution loop
    /// with an empty (already-cleared) entity-plan list.
    pub fn resume_move_set(
        &self,
        journal_id: Uuid,
    ) -> Result<MoveSetOutcome, String> {
        let existing = load_move_set_journal(self.store_root(), journal_id)
            .map_err(from_move_error)?;
        if matches!(
            existing.phase,
            MoveSetExecutionPhase::Validated | MoveSetExecutionPhase::RolledBack
        ) {
            let entity_ids = existing.entity_ids.clone();
            return Ok(MoveSetOutcome {
                journal: existing,
                entity_ids,
                entity_outcomes: Vec::new(),
            });
        }
        let domain = FeedbackMoveDomain::new(self);
        move_kernel::resume_move_set(&domain, journal_id).map_err(from_move_error)
    }

    /// Roll back a completed or partially completed set move.
    pub fn rollback_move_set(
        &self,
        journal_id: Uuid,
    ) -> Result<MoveSetOutcome, String> {
        let domain = FeedbackMoveDomain::new(self);
        move_kernel::rollback_move_set(&domain, journal_id).map_err(from_move_error)
    }
}

#[cfg(test)]
#[path = "move_domain_tests.rs"]
mod move_domain_tests;
