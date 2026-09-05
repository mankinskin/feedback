//! Resumable legacy-NDJSON -> canonical-entity-folder migration.
//!
//! Migration validates the complete legacy log before staging any entity,
//! rejects malformed records and duplicate canonical ids before any
//! mutation, atomically publishes a durable manifest before writing entity
//! folders, and supports manifest-driven resume or rollback. Legacy NDJSON
//! is never deleted or rewritten by migration; it remains a read-only
//! migration backup.

use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    FEEDBACK_SCHEMA_VERSION, FeedbackEntry,
    canonical::{CanonicalFeedbackEntity, CanonicalFeedbackStore, canonical_entity_id},
};

const MIGRATION_MANIFEST_FILE: &str = ".migration-manifest.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MigrationEntryStatus {
    Pending,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationManifestEntry {
    pub ordinal: u64,
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub status: MigrationEntryStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationManifest {
    pub id: Uuid,
    pub legacy_path: PathBuf,
    pub entries: Vec<MigrationManifestEntry>,
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationOutcome {
    pub manifest_id: Uuid,
    pub migrated_ids: Vec<Uuid>,
    pub completed: bool,
}

/// Result of a completed no-backup legacy-schema cutover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaCutoverOutcome {
    pub migrated_count: usize,
    pub discarded_count: usize,
}

/// Migrate parseable legacy records into the canonical schema and permanently
/// remove the legacy file once all canonical entities have been published.
/// Malformed JSON or timestamp-invalid lines are omitted and reported only by
/// their aggregate discarded count.
pub fn migrate_and_discard_legacy_ndjson(
    store: &CanonicalFeedbackStore,
    workspace_slug: &str,
    legacy_path: &Path,
) -> Result<SchemaCutoverOutcome, String> {
    if !legacy_path.is_file() {
        return Err(format!(
            "legacy feedback log {} does not exist",
            legacy_path.display()
        ));
    }
    if !store.list_entities()?.is_empty() {
        return Err(
            "canonical feedback entities already exist; destructive cutover requires an empty destination"
                .to_string(),
        );
    }

    let file = fs::File::open(legacy_path).map_err(|err| {
        format!(
            "failed to open legacy feedback log {}: {err}",
            legacy_path.display()
        )
    })?;
    let mut migrated_count = 0;
    let mut discarded_count = 0;
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|err| {
            format!(
                "failed reading legacy feedback log {}: {err}",
                legacy_path.display()
            )
        })?;
        let Ok(mut entry) = serde_json::from_str::<FeedbackEntry>(&line) else {
            discarded_count += 1;
            continue;
        };
        if chrono::DateTime::parse_from_rfc3339(&entry.provenance.executed_at).is_err() {
            discarded_count += 1;
            continue;
        }
        entry.schema_version = FEEDBACK_SCHEMA_VERSION;
        let id = canonical_entity_id(workspace_slug, &entry.id);
        let alias = (entry.id != id.to_string()).then(|| entry.id.clone());
        store.write_entity(&CanonicalFeedbackEntity::new(
            id,
            alias,
            migrated_count as u64,
            entry,
        ))?;
        migrated_count += 1;
    }
    fs::remove_file(legacy_path).map_err(|err| {
        format!(
            "failed to remove legacy feedback log {}: {err}",
            legacy_path.display()
        )
    })?;
    Ok(SchemaCutoverOutcome {
        migrated_count,
        discarded_count,
    })
}

fn manifest_path(store: &CanonicalFeedbackStore) -> PathBuf {
    store.store_root().join(MIGRATION_MANIFEST_FILE)
}

fn persist_manifest(
    store: &CanonicalFeedbackStore,
    manifest: &MigrationManifest,
) -> Result<(), String> {
    fs::create_dir_all(store.store_root()).map_err(|err| {
        format!(
            "failed to create feedback store root {}: {err}",
            store.store_root().display()
        )
    })?;
    let path = manifest_path(store);
    let tmp_path = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|err| format!("failed to serialize migration manifest: {err}"))?;
    fs::write(&tmp_path, bytes).map_err(|err| {
        format!(
            "failed to write migration manifest {}: {err}",
            tmp_path.display()
        )
    })?;
    fs::rename(&tmp_path, &path).map_err(|err| {
        format!(
            "failed to publish migration manifest {}: {err}",
            path.display()
        )
    })
}

fn load_manifest(store: &CanonicalFeedbackStore) -> Result<Option<MigrationManifest>, String> {
    let path = manifest_path(store);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|err| {
        format!(
            "failed to read migration manifest {}: {err}",
            path.display()
        )
    })?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|err| format!("invalid migration manifest {}: {err}", path.display()))
}

/// Parse every legacy NDJSON record from `path` in physical file order,
/// validating the complete log before returning anything. Blank lines are
/// skipped without consuming an ordinal; every non-blank line must parse as
/// a [`FeedbackEntry`] or the whole read fails with no partial result.
/// Ordinals are the zero-based, sequential index of valid records as they
/// physically appear in the file.
pub fn parse_legacy_records(path: &Path) -> Result<Vec<(u64, FeedbackEntry)>, String> {
    if !path.is_file() {
        return Err(format!(
            "legacy feedback log {} does not exist",
            path.display()
        ));
    }
    let file = fs::File::open(path).map_err(|err| {
        format!(
            "failed to open legacy feedback log {}: {err}",
            path.display()
        )
    })?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    let mut ordinal: u64 = 0;

    for (line_number, line) in reader.lines().enumerate() {
        let line = line.map_err(|err| {
            format!(
                "failed reading legacy feedback log {} line {}: {err}",
                path.display(),
                line_number + 1
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let entry = serde_json::from_str::<FeedbackEntry>(&line).map_err(|err| {
            format!(
                "malformed legacy feedback record {} line {}: {err}",
                path.display(),
                line_number + 1
            )
        })?;
        records.push((ordinal, entry));
        ordinal += 1;
    }

    Ok(records)
}

/// Migrate a legacy NDJSON log into canonical UUID-keyed entity folders.
/// Fails closed on a mixed layout (canonical entities already present with
/// no owning manifest) and rejects malformed records or duplicate canonical
/// ids before any mutation.
pub fn migrate_legacy_ndjson(
    store: &CanonicalFeedbackStore,
    workspace_slug: &str,
    legacy_path: &Path,
) -> Result<MigrationOutcome, String> {
    if manifest_path(store).is_file() {
        return Err(
            "a feedback migration manifest already exists; use resume or rollback".to_string(),
        );
    }
    let existing = store.list_entities()?;
    if !existing.is_empty() {
        return Err(
            "canonical feedback entities already exist with no owning migration manifest (unowned mixed layout)"
                .to_string(),
        );
    }

    let records = parse_legacy_records(legacy_path)?;

    let mut seen_ids = std::collections::BTreeSet::new();
    let mut entries = Vec::with_capacity(records.len());
    for (ordinal, entry) in &records {
        let id = canonical_entity_id(workspace_slug, &entry.id);
        if !seen_ids.insert(id) {
            return Err(format!(
                "duplicate canonical feedback id {id} detected at ordinal {ordinal} during migration"
            ));
        }
        let alias = (entry.id != id.to_string()).then(|| entry.id.clone());
        entries.push(MigrationManifestEntry {
            ordinal: *ordinal,
            id,
            alias,
            status: MigrationEntryStatus::Pending,
        });
    }

    let manifest = MigrationManifest {
        id: Uuid::new_v4(),
        legacy_path: legacy_path.to_path_buf(),
        entries,
        completed: false,
    };
    persist_manifest(store, &manifest)?;

    execute_manifest(store, manifest, &records)
}

fn execute_manifest(
    store: &CanonicalFeedbackStore,
    mut manifest: MigrationManifest,
    records: &[(u64, FeedbackEntry)],
) -> Result<MigrationOutcome, String> {
    let by_ordinal: BTreeMap<u64, &FeedbackEntry> = records
        .iter()
        .map(|(ordinal, entry)| (*ordinal, entry))
        .collect();

    for index in 0..manifest.entries.len() {
        if manifest.entries[index].status == MigrationEntryStatus::Done {
            continue;
        }
        let ordinal = manifest.entries[index].ordinal;
        let entry = by_ordinal.get(&ordinal).ok_or_else(|| {
            format!("legacy record for ordinal {ordinal} missing during migration execution")
        })?;
        let canonical = CanonicalFeedbackEntity::new(
            manifest.entries[index].id,
            manifest.entries[index].alias.clone(),
            ordinal,
            (*entry).clone(),
        );
        store.write_entity(&canonical)?;
        manifest.entries[index].status = MigrationEntryStatus::Done;
        persist_manifest(store, &manifest)?;
    }

    manifest.completed = true;
    persist_manifest(store, &manifest)?;

    Ok(MigrationOutcome {
        manifest_id: manifest.id,
        migrated_ids: manifest.entries.iter().map(|entry| entry.id).collect(),
        completed: true,
    })
}

/// Resume an interrupted migration from its durable manifest. Re-reads the
/// (read-only) legacy log to rebuild any not-yet-written entities; already
/// completed entries are left untouched. Idempotent: resuming a completed
/// migration is a no-op that reports success.
pub fn resume_migration(store: &CanonicalFeedbackStore) -> Result<MigrationOutcome, String> {
    let manifest = load_manifest(store)?
        .ok_or_else(|| "no feedback migration manifest found to resume".to_string())?;
    if manifest.completed {
        return Ok(MigrationOutcome {
            manifest_id: manifest.id,
            migrated_ids: manifest.entries.iter().map(|entry| entry.id).collect(),
            completed: true,
        });
    }
    let records = parse_legacy_records(&manifest.legacy_path)?;
    execute_manifest(store, manifest, &records)
}

/// Roll back a migration, removing every canonical entity folder the
/// manifest wrote and deleting the manifest itself. Idempotent: rolling
/// back twice removes nothing further. Legacy NDJSON is never touched.
pub fn rollback_migration(store: &CanonicalFeedbackStore) -> Result<MigrationOutcome, String> {
    let manifest = load_manifest(store)?
        .ok_or_else(|| "no feedback migration manifest found to rollback".to_string())?;

    let mut migrated_ids = Vec::with_capacity(manifest.entries.len());
    for entry in &manifest.entries {
        if entry.status == MigrationEntryStatus::Done {
            store.remove_entity(&entry.id)?;
        }
        migrated_ids.push(entry.id);
    }

    let path = manifest_path(store);
    if path.is_file() {
        fs::remove_file(&path).map_err(|err| {
            format!(
                "failed to remove migration manifest {}: {err}",
                path.display()
            )
        })?;
    }

    Ok(MigrationOutcome {
        manifest_id: manifest.id,
        migrated_ids,
        completed: false,
    })
}
