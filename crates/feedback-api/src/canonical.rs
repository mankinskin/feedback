//! UUID-keyed canonical feedback entity storage, compatible with
//! [`memory_kernel::storage::move_kernel::MoveDomain`].
//!
//! Each feedback entry is persisted as its own folder
//! `<store_root>/entries/<uuid>/entity.json`, independent of the legacy
//! per-workspace NDJSON log used by [`crate::EntityFeedbackStore`]. A legacy
//! entry id that parses as a UUID keeps that UUID; any other legacy id is
//! deterministically re-derived (see [`canonical_entity_id`]).

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::FeedbackEntry;

/// Subdirectory under a `.feedback` store root that holds entity folders.
pub const FEEDBACK_ENTITY_SUBDIR: &str = "entries";
/// File name of the canonical entity payload inside its entity folder.
pub const FEEDBACK_ENTITY_FILE_NAME: &str = "entity.json";
/// Store-marker directory name used for workspace<->store-root resolution.
pub const FEEDBACK_STORE_INDEX_DIR: &str = ".feedback";

/// Fixed namespace used to derive a canonical UUID from a non-UUID legacy
/// feedback id plus a workspace slug. Never changes once published.
const FEEDBACK_LEGACY_NAMESPACE: Uuid = Uuid::from_bytes([
    0x6f, 0x1e, 0x3a, 0x02, 0x9c, 0x77, 0x4b, 0x21, 0x8e, 0x54, 0x2d, 0x9a, 0x11, 0x7c, 0x5f, 0x3d,
]);

/// A canonical, UUID-keyed feedback entity: the persisted [`FeedbackEntry`]
/// plus its immutable legacy alias and physical chronology ordinal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalFeedbackEntity {
    pub id: Uuid,
    /// Immutable legacy id, present only when it differs from `id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// Immutable zero-based physical NDJSON record ordinal this entity was
    /// migrated from, or the next monotonic ordinal for a freshly-created
    /// canonical entity.
    pub legacy_append_ordinal: u64,
    pub entry: FeedbackEntry,
    /// SHA-256 over the serialized payload plus alias and chronology
    /// metadata; recompute with [`CanonicalFeedbackEntity::recompute_digest`].
    pub digest: String,
}

impl CanonicalFeedbackEntity {
    pub fn new(
        id: Uuid,
        alias: Option<String>,
        legacy_append_ordinal: u64,
        entry: FeedbackEntry,
    ) -> Self {
        let digest = compute_digest(&entry, alias.as_deref(), legacy_append_ordinal);
        Self {
            id,
            alias,
            legacy_append_ordinal,
            entry,
            digest,
        }
    }

    pub fn recompute_digest(&self) -> String {
        compute_digest(
            &self.entry,
            self.alias.as_deref(),
            self.legacy_append_ordinal,
        )
    }

    pub fn digest_is_valid(&self) -> bool {
        self.digest == self.recompute_digest()
    }
}

/// Canonical digest: SHA-256 over the serialized payload plus immutable
/// alias and chronology metadata.
pub fn compute_digest(
    entry: &FeedbackEntry,
    alias: Option<&str>,
    legacy_append_ordinal: u64,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(entry).expect("feedback entry always serializes"));
    if let Some(alias) = alias {
        hasher.update(alias.as_bytes());
    }
    hasher.update(legacy_append_ordinal.to_le_bytes());
    format!("{:x}", hasher.finalize())
}

/// Resolve the canonical UUID for a legacy feedback id. A legacy id that
/// parses as a UUID keeps that UUID; otherwise a UUIDv5 is derived from the
/// fixed feedback namespace, the workspace slug, and the legacy id.
pub fn canonical_entity_id(workspace_slug: &str, legacy_id: &str) -> Uuid {
    match Uuid::parse_str(legacy_id) {
        Ok(uuid) => uuid,
        Err(_) => Uuid::new_v5(
            &FEEDBACK_LEGACY_NAMESPACE,
            format!("{workspace_slug}:{legacy_id}").as_bytes(),
        ),
    }
}

/// UUID-keyed canonical feedback entity store rooted at a `.feedback`
/// store-marker directory (not a workspace root).
#[derive(Debug, Clone)]
pub struct CanonicalFeedbackStore {
    store_root: PathBuf,
}

impl CanonicalFeedbackStore {
    /// Build a store directly from an already-resolved `.feedback` store
    /// root (the store-marker directory itself, not the workspace root).
    pub fn new(store_root: impl Into<PathBuf>) -> Self {
        Self {
            store_root: store_root.into(),
        }
    }

    /// Resolve a `.feedback` store root from any workspace-relative path.
    pub fn open(workspace_root: &Path) -> Self {
        Self::new(memory_kernel::workspace::resolve_store_root_from(
            workspace_root,
            FEEDBACK_STORE_INDEX_DIR,
        ))
    }

    pub fn store_root(&self) -> &Path {
        &self.store_root
    }

    pub fn entities_dir(&self) -> PathBuf {
        self.store_root.join(FEEDBACK_ENTITY_SUBDIR)
    }

    pub fn entity_dir(&self, id: &Uuid) -> PathBuf {
        self.entities_dir().join(id.to_string())
    }

    pub fn entity_path(&self, id: &Uuid) -> PathBuf {
        self.entity_dir(id).join(FEEDBACK_ENTITY_FILE_NAME)
    }

    /// Atomically persist a canonical entity (write-tmp-then-rename).
    pub fn write_entity(&self, entity: &CanonicalFeedbackEntity) -> Result<(), String> {
        let dir = self.entity_dir(&entity.id);
        fs::create_dir_all(&dir).map_err(|err| {
            format!(
                "failed to create feedback entity directory {}: {err}",
                dir.display()
            )
        })?;
        let path = dir.join(FEEDBACK_ENTITY_FILE_NAME);
        let tmp_path = dir.join(format!("{FEEDBACK_ENTITY_FILE_NAME}.tmp"));
        let bytes = serde_json::to_vec_pretty(entity)
            .map_err(|err| format!("failed to serialize canonical feedback entity: {err}"))?;
        fs::write(&tmp_path, bytes).map_err(|err| {
            format!(
                "failed to write feedback entity {}: {err}",
                tmp_path.display()
            )
        })?;
        fs::rename(&tmp_path, &path).map_err(|err| {
            format!(
                "failed to publish feedback entity {}: {err}",
                path.display()
            )
        })
    }

    pub fn read_entity(&self, id: &Uuid) -> Result<Option<CanonicalFeedbackEntity>, String> {
        let path = self.entity_path(id);
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = fs::read(&path)
            .map_err(|err| format!("failed to read feedback entity {}: {err}", path.display()))?;
        serde_json::from_slice(&bytes).map(Some).map_err(|err| {
            format!(
                "invalid canonical feedback entity {}: {err}",
                path.display()
            )
        })
    }

    pub fn remove_entity(&self, id: &Uuid) -> Result<(), String> {
        let dir = self.entity_dir(id);
        if dir.is_dir() {
            fs::remove_dir_all(&dir).map_err(|err| {
                format!(
                    "failed to remove feedback entity directory {}: {err}",
                    dir.display()
                )
            })?;
        }
        Ok(())
    }

    /// All canonical entities, sorted by ordinal then UUID.
    pub fn list_entities(&self) -> Result<Vec<CanonicalFeedbackEntity>, String> {
        let dir = self.entities_dir();
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut entities = Vec::new();
        for entry in fs::read_dir(&dir)
            .map_err(|err| format!("failed to list feedback entities {}: {err}", dir.display()))?
        {
            let entry = entry.map_err(|err| err.to_string())?;
            if !entry.file_type().map_err(|err| err.to_string())?.is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let Ok(id) = Uuid::parse_str(&name) else {
                continue;
            };
            if let Some(entity) = self.read_entity(&id)? {
                entities.push(entity);
            }
        }
        entities.sort_by(|left, right| {
            left.legacy_append_ordinal
                .cmp(&right.legacy_append_ordinal)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(entities)
    }

    /// Next monotonic ordinal for a freshly-created canonical entity.
    pub fn next_ordinal(&self) -> Result<u64, String> {
        Ok(self
            .list_entities()?
            .iter()
            .map(|entity| entity.legacy_append_ordinal)
            .max()
            .map(|max| max + 1)
            .unwrap_or(0))
    }

    /// Canonicalize and persist a freshly-created [`FeedbackEntry`], assigning
    /// it the next monotonic ordinal.
    pub fn append_new_entry(
        &self,
        workspace_slug: &str,
        entry: FeedbackEntry,
    ) -> Result<CanonicalFeedbackEntity, String> {
        let id = canonical_entity_id(workspace_slug, &entry.id);
        let alias = (entry.id != id.to_string()).then(|| entry.id.clone());
        let ordinal = self.next_ordinal()?;
        let entity = CanonicalFeedbackEntity::new(id, alias, ordinal, entry);
        self.write_entity(&entity)?;
        Ok(entity)
    }
}
