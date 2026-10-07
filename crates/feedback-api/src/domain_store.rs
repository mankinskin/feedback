use std::path::{Path, PathBuf};

use memory_kernel::{
    domain_store::{
        CreateEntity, DomainStore, DomainStoreResolution, ListEntities, ReadEntity, StoreAccessMode,
    },
    model::domain::DomainId,
    workspace::StoreRootDiagnostic,
};

use crate::{EntityFeedbackStore, EntityUrn, FeedbackEntry, canonical::FEEDBACK_STORE_INDEX_DIR};

/// Domain-owned configuration for a resolved Feedback store.
#[derive(Debug, Clone)]
pub struct FeedbackStoreConfig {
    root: PathBuf,
}

impl FeedbackStoreConfig {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve one explicitly selected Feedback workspace through the shared
    /// core. Writes retain the established legacy-store migration behavior;
    /// read-only access never creates a missing canonical store.
    pub fn resolve_workspace_store(
        local_workspace: &Path,
        access_mode: StoreAccessMode,
    ) -> Result<DomainStoreResolution, String> {
        if access_mode == StoreAccessMode::CreateOrOpen {
            memory_kernel::workspace::validate_explicit_store_root_for_write(
                local_workspace,
                FEEDBACK_STORE_INDEX_DIR,
            )
            .map_err(|error| error.to_string())?;
            crate::canonical::resolve_feedback_store_root(local_workspace)?;
        }

        let mut resolution = <Self as DomainStore>::resolve_store(local_workspace, access_mode)
            .map_err(|error| error.to_string())?;
        let legacy_root = resolution.local_workspace.join(FEEDBACK_STORE_INDEX_DIR);
        let canonical_root = memory_kernel::workspace::canonical_store_root(
            &resolution.local_workspace,
            FEEDBACK_STORE_INDEX_DIR,
        );
        let legacy_exists = legacy_root.is_dir();
        let canonical_exists = canonical_root.is_dir();

        if access_mode == StoreAccessMode::ReadOnly && legacy_exists && !canonical_exists {
            resolution.store_root = legacy_root.clone();
        }
        if legacy_exists {
            let diagnostic = if canonical_exists {
                StoreRootDiagnostic::BothLayoutsPresent {
                    domain: "feedback".to_string(),
                    legacy_path: legacy_root,
                    canonical_path: canonical_root,
                }
            } else {
                StoreRootDiagnostic::LegacyStore {
                    domain: "feedback".to_string(),
                    legacy_path: legacy_root,
                    canonical_path: canonical_root,
                }
            };
            if !resolution.diagnostics.contains(&diagnostic) {
                resolution.diagnostics.push(diagnostic);
            }
        }
        Ok(resolution)
    }
}

impl DomainStore for FeedbackStoreConfig {
    fn domain_id() -> DomainId {
        DomainId::new("feedback").expect("feedback is a valid domain id")
    }

    fn store_dir_name() -> &'static str {
        FEEDBACK_STORE_INDEX_DIR
    }
}

/// Typed capabilities over Feedback's existing entry and target-query model.
#[derive(Debug, Clone)]
pub struct FeedbackDomainStore {
    store: EntityFeedbackStore,
}

impl FeedbackDomainStore {
    pub fn new(config: FeedbackStoreConfig) -> Self {
        Self {
            store: EntityFeedbackStore::new(config.root),
        }
    }
}

impl CreateEntity for FeedbackDomainStore {
    type Entity = FeedbackEntry;
    type CreateInput = FeedbackEntry;
    type CreateResult = FeedbackEntry;
    type Error = String;

    fn create_entity(&self, input: Self::CreateInput) -> Result<Self::CreateResult, Self::Error> {
        self.store.record_entry(input)
    }
}

impl ReadEntity for FeedbackDomainStore {
    type EntityId = EntityUrn;
    type Entity = FeedbackEntry;
    type ReadResult = Vec<FeedbackEntry>;
    type Error = String;

    fn read_entity(&self, id: Self::EntityId) -> Result<Self::ReadResult, Self::Error> {
        self.store.entries_for(&id)
    }
}

impl ListEntities for FeedbackDomainStore {
    type Query = ();
    type Entity = FeedbackEntry;
    type ListResult = Vec<FeedbackEntry>;
    type Error = String;

    fn list_entities(&self, (): Self::Query) -> Result<Self::ListResult, Self::Error> {
        Ok(
            crate::canonical::CanonicalFeedbackStore::new(self.store.root.clone())
                .list_entities()?
                .into_iter()
                .map(|entity| entity.entry)
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use memory_kernel::{
        domain_store::{CreateEntity, ReadEntity, StoreAccessMode},
        workspace::canonical_store_root,
    };
    use tempfile::tempdir;

    use super::*;
    use crate::{FeedbackProvenance, FeedbackRating, FeedbackSource};

    #[test]
    fn selected_workspace_capabilities_create_and_read_only_its_canonical_store() {
        let parent = tempdir().unwrap();
        let selected = parent.path().join("selected");
        let sibling = parent.path().join("sibling");
        std::fs::create_dir_all(&selected).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();

        let resolution =
            FeedbackStoreConfig::resolve_workspace_store(&selected, StoreAccessMode::CreateOrOpen)
                .unwrap();
        let store = FeedbackDomainStore::new(FeedbackStoreConfig::new(resolution.store_root));
        let target = EntityUrn::ticket("default", "feedback-domain-store").unwrap();
        let entry = FeedbackEntry::new(
            FeedbackSource::Agent,
            target.clone(),
            Some(FeedbackRating::Helpful),
            Some("selected store".to_string()),
            None,
            FeedbackProvenance::new(None, Some("test".to_string()), None).unwrap(),
        )
        .unwrap();

        assert_eq!(store.create_entity(entry.clone()).unwrap(), entry);
        assert_eq!(store.read_entity(target).unwrap(), vec![entry]);
        assert!(
            canonical_store_root(&selected, FEEDBACK_STORE_INDEX_DIR)
                .join("entries")
                .is_dir()
        );
        assert!(!canonical_store_root(parent.path(), FEEDBACK_STORE_INDEX_DIR).exists());
        assert!(!canonical_store_root(&sibling, FEEDBACK_STORE_INDEX_DIR).exists());
    }
}
