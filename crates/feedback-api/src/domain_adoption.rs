//! Feedback-domain adapter boundary onto the generic entity kernel's domain
//! manifest and migration-dry-run contracts
//! (`memory_kernel::model::domain_manifest`, `memory_kernel::MigrationDryRunReport`).
//!
//! Per `transcripts/15-09-2026_entity-kernel-all-domains/06-domain-adoption.md`,
//! feedback is the last source-of-truth domain migrated: it has no existing
//! `migration.rs` to reconcile, so this module is the first migration
//! adapter built directly against the kernel protocol rather than reconciled
//! from a prior implementation. It registers feedback's one currently
//! delivered entity type (`feedback-entry`, backed by
//! [`crate::FeedbackEntry`]) with the kernel's generic [`DomainManifest`],
//! proves a zero-change current-state dry-run report, and provides a bounded
//! identifier adapter for [`crate::canonical::CanonicalFeedbackEntity`] — the
//! clear existing type already consumed by [`crate::move_domain`]. It is
//! purely additive: no feedback business schema, store behavior, analytics,
//! or move/store logic changes.

use std::collections::BTreeMap;

use memory_kernel::{
    model::{
        domain::{DomainId, DomainSchemaVersion, EntityTypeId, EntityTypeSchemaVersion},
        domain_manifest::{
            DomainManifest, DomainManifestError, EntityTypeMembership, EntityTypeStatus,
        },
        entity::EntityId,
    },
    MigrationDryRunReport,
};

use crate::{canonical::CanonicalFeedbackEntity, FEEDBACK_SCHEMA_VERSION};

/// The kernel [`DomainId`] under which feedback-api registers itself.
pub const FEEDBACK_DOMAIN_ID: &str = "feedback";

/// The feedback domain's own [`DomainSchemaVersion`] as of this adoption
/// step. This tracks the domain manifest's own membership/activation set,
/// not [`FEEDBACK_SCHEMA_VERSION`] (the `feedback-entry` entity type's own
/// schema version, see [`EntityTypeMembership`]).
pub const FEEDBACK_DOMAIN_SCHEMA_VERSION: u32 = 1;

/// The kernel [`EntityTypeId`] string registered for [`crate::FeedbackEntry`].
pub const FEEDBACK_ENTITY_TYPE_ID: &str = "feedback-entry";

/// Build the feedback domain's [`DomainManifest`] from the currently
/// delivered `feedback-entry` entity type. Registered
/// [`EntityTypeStatus::Active`] at [`FEEDBACK_SCHEMA_VERSION`], since
/// feedback-api has no existing inactive-type concept to preserve as of this
/// adoption step.
pub fn feedback_domain_manifest() -> Result<DomainManifest, DomainManifestError> {
    let domain_id = DomainId::new(FEEDBACK_DOMAIN_ID).expect("FEEDBACK_DOMAIN_ID is non-empty");

    let entity_types = vec![EntityTypeMembership {
        entity_type_id: EntityTypeId::new(FEEDBACK_ENTITY_TYPE_ID)
            .expect("FEEDBACK_ENTITY_TYPE_ID is non-empty"),
        schema_version: EntityTypeSchemaVersion(FEEDBACK_SCHEMA_VERSION),
        status: EntityTypeStatus::Active,
    }];

    DomainManifest::new(
        domain_id,
        DomainSchemaVersion(FEEDBACK_DOMAIN_SCHEMA_VERSION),
        entity_types,
    )
}

/// Build a kernel-shaped dry-run report for the feedback domain's current
/// manifest state: the active entity type's target schema version equals
/// its current version, so the report always plans zero steps and zero
/// violations. This proves feedback-api can produce the kernel's generic
/// [`MigrationDryRunReport`] shape from its own domain manifest without any
/// actual schema change, and without touching existing store, canonical, or
/// move-domain behavior.
pub fn feedback_domain_current_state_report() -> Result<MigrationDryRunReport, DomainManifestError>
{
    let manifest = feedback_domain_manifest()?;
    let target_versions: BTreeMap<EntityTypeId, EntityTypeSchemaVersion> = manifest
        .active_entity_types()
        .map(|membership| (membership.entity_type_id.clone(), membership.schema_version))
        .collect();

    Ok(MigrationDryRunReport::plan(
        &manifest,
        manifest.schema_version,
        &target_versions,
        &BTreeMap::new(),
        &[],
    ))
}

/// Bounded adapter from [`CanonicalFeedbackEntity`] (the existing type
/// already consumed by [`crate::move_domain::FeedbackMoveDomain`]) onto the
/// kernel's [`EntityId`]. `CanonicalFeedbackEntity::id` is already a UUID,
/// which is exactly the kernel's [`EntityId`] representation, so this is a
/// pure identity read — it performs no I/O and does not touch canonical
/// entity persistence or move/store behavior.
pub fn feedback_entity_id(entity: &CanonicalFeedbackEntity) -> EntityId {
    entity.id
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::{
        canonical::CanonicalFeedbackEntity, EntityUrn, FeedbackEntry, FeedbackProvenance,
        FeedbackRating, FeedbackSource,
    };

    #[test]
    fn feedback_domain_manifest_registers_one_active_entity_type() {
        let manifest = feedback_domain_manifest().expect("feedback domain manifest is valid");

        assert_eq!(manifest.domain_id.as_str(), FEEDBACK_DOMAIN_ID);
        assert_eq!(
            manifest.schema_version,
            DomainSchemaVersion(FEEDBACK_DOMAIN_SCHEMA_VERSION)
        );

        let entity_type_id = EntityTypeId::new(FEEDBACK_ENTITY_TYPE_ID).unwrap();
        let membership = manifest
            .membership(&entity_type_id)
            .expect("feedback-entry registered in feedback domain manifest");
        assert_eq!(
            membership.schema_version,
            EntityTypeSchemaVersion(FEEDBACK_SCHEMA_VERSION)
        );
        assert!(membership.status.is_active());

        assert_eq!(manifest.active_entity_types().count(), 1);
        assert_eq!(manifest.inactive_entity_types().count(), 0);
    }

    #[test]
    fn current_state_report_plans_zero_steps_and_zero_violations() {
        let report = feedback_domain_current_state_report()
            .expect("current-state dry-run report can be built");

        assert_eq!(report.domain_id.as_str(), FEEDBACK_DOMAIN_ID);
        assert_eq!(
            report.from_domain_schema_version,
            DomainSchemaVersion(FEEDBACK_DOMAIN_SCHEMA_VERSION)
        );
        assert_eq!(
            report.to_domain_schema_version,
            DomainSchemaVersion(FEEDBACK_DOMAIN_SCHEMA_VERSION)
        );
        assert!(report.steps.is_empty());
        assert!(report.inactive_entity_types.is_empty());
        assert!(report.external_urn_violations.is_empty());
    }

    #[test]
    fn feedback_entity_id_reads_the_canonical_uuid_unchanged() {
        let id = Uuid::new_v4();
        let target = EntityUrn::ticket("default", "00000000-0000-0000-0000-000000000000")
            .expect("target urn is valid");
        let provenance = FeedbackProvenance::new(None, Some("tester".to_string()), None)
            .expect("provenance is valid");
        let entry = FeedbackEntry::new(
            FeedbackSource::User,
            target,
            Some(FeedbackRating::Helpful),
            None,
            None,
            provenance,
        )
        .expect("feedback entry is valid");
        let entity = CanonicalFeedbackEntity::new(id, None, 0, entry);

        assert_eq!(feedback_entity_id(&entity), id);
    }
}
