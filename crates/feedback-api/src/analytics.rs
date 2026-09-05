use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader},
    path::Path,
};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::{FeedbackEntry, FeedbackRating, normalize_optional, normalize_required};

pub const TENTATIVE_RESOLUTION_QUIET_DAYS: i64 = 10;
pub const TENTATIVE_RESOLUTION_MINIMUM_VALID_EVENTS: usize = 100;

/// The analytic meaning of a rating. Ratings without a positive or negative
/// signal remain visible as neutral observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackClassification {
    Positive,
    Neutral,
    Negative,
}

impl From<Option<FeedbackRating>> for FeedbackClassification {
    fn from(rating: Option<FeedbackRating>) -> Self {
        match rating {
            Some(FeedbackRating::Helpful) => Self::Positive,
            Some(FeedbackRating::NotHelpful) => Self::Negative,
            Some(FeedbackRating::Mixed) | None => Self::Neutral,
        }
    }
}

/// Identifies the tool and optional operation an incident aggregates.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IncidentIdentity {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
}

impl IncidentIdentity {
    pub fn new(tool: impl Into<String>, operation: Option<String>) -> Result<Self, String> {
        let tool = normalize_required(tool.into(), "tool")?;
        let operation = normalize_optional(operation);
        Ok(Self { tool, operation })
    }
}

/// One parseable feedback observation supplied to read-only analytics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackAnalyticEvent {
    pub observed_at: DateTime<Utc>,
    pub classification: FeedbackClassification,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incident: Option<IncidentIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explicit_resolution_at: Option<DateTime<Utc>>,
}

impl FeedbackAnalyticEvent {
    pub fn new(
        observed_at: DateTime<Utc>,
        rating: Option<FeedbackRating>,
        incident: Option<IncidentIdentity>,
    ) -> Self {
        Self {
            observed_at,
            classification: rating.into(),
            incident,
            explicit_resolution_at: None,
        }
    }

    pub fn with_explicit_resolution(mut self, resolved_at: DateTime<Utc>) -> Self {
        self.explicit_resolution_at = Some(resolved_at);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IncidentResolution {
    Unresolved,
    Explicit { resolved_at: DateTime<Utc> },
    Tentative { assessed_at: DateTime<Utc> },
}

/// A derived, non-persistent aggregate of matching negative observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecurringIncident {
    pub identity: IncidentIdentity,
    pub negative_event_count: usize,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub resolution: IncidentResolution,
}

/// Read-only statistical result for an existing feedback NDJSON log.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackAnalyticsReport {
    pub valid_event_count: usize,
    pub malformed_line_count: usize,
    pub daily_volume: BTreeMap<String, usize>,
    pub source_distribution: BTreeMap<String, usize>,
    pub target_store_distribution: BTreeMap<String, usize>,
    pub rating_distribution: BTreeMap<String, usize>,
    pub first_observed_at: Option<DateTime<Utc>>,
    pub last_observed_at: Option<DateTime<Utc>>,
    pub incidents: Vec<RecurringIncident>,
}

/// Analyze current-schema feedback records without rewriting, migrating, or
/// canonicalizing the source log. A malformed JSON record or timestamp adds
/// to the data-quality total and never contributes to a statistical field.
pub fn analyze_feedback_ndjson(
    path: &Path,
    assessed_at: DateTime<Utc>,
) -> Result<FeedbackAnalyticsReport, String> {
    if !path.exists() {
        return Ok(FeedbackAnalyticsReport::default());
    }

    let file = fs::File::open(path).map_err(|err| {
        format!(
            "failed to open feedback analytics log {}: {err}",
            path.display()
        )
    })?;
    let mut report = FeedbackAnalyticsReport::default();
    let mut events = Vec::new();

    for line in BufReader::new(file).lines() {
        let line = line.map_err(|err| {
            format!(
                "failed reading feedback analytics log {}: {err}",
                path.display()
            )
        })?;
        if line.trim().is_empty() {
            report.malformed_line_count += 1;
            continue;
        }
        let Ok(entry) = serde_json::from_str::<FeedbackEntry>(&line) else {
            report.malformed_line_count += 1;
            continue;
        };
        let Ok(observed_at) = DateTime::parse_from_rfc3339(&entry.provenance.executed_at) else {
            report.malformed_line_count += 1;
            continue;
        };
        let observed_at = observed_at.with_timezone(&Utc);
        if observed_at > assessed_at {
            continue;
        }

        report.valid_event_count += 1;
        increment(
            &mut report.daily_volume,
            observed_at.date_naive().to_string(),
        );
        increment(
            &mut report.source_distribution,
            entry.source.as_str().to_string(),
        );
        increment(
            &mut report.target_store_distribution,
            entry.target.store().to_string(),
        );
        increment(
            &mut report.rating_distribution,
            entry.rating.map_or_else(
                || "unrated".to_string(),
                |rating| rating.as_str().to_string(),
            ),
        );
        report.first_observed_at = Some(
            report
                .first_observed_at
                .map_or(observed_at, |first| first.min(observed_at)),
        );
        report.last_observed_at = Some(
            report
                .last_observed_at
                .map_or(observed_at, |last| last.max(observed_at)),
        );
        events.push(FeedbackAnalyticEvent::new(
            observed_at,
            entry.rating,
            Some(IncidentIdentity {
                tool: entry.target.store().to_string(),
                operation: Some(entry.target.entity().to_string()),
            }),
        ));
    }

    let identities: BTreeMap<_, _> = events
        .iter()
        .filter_map(|event| event.incident.as_ref())
        .map(|identity| {
            (
                (identity.tool.clone(), identity.operation.clone()),
                identity.clone(),
            )
        })
        .collect();
    report.incidents = identities
        .into_values()
        .filter_map(|identity| RecurringIncident::from_events(&events, &identity, assessed_at))
        .collect();
    Ok(report)
}

/// Analyze canonical feedback entries after a completed schema cutover.
pub fn analyze_feedback_entries(
    entries: &[FeedbackEntry],
    assessed_at: DateTime<Utc>,
) -> FeedbackAnalyticsReport {
    let mut report = FeedbackAnalyticsReport::default();
    let mut events = Vec::new();

    for entry in entries {
        let Ok(observed_at) = DateTime::parse_from_rfc3339(&entry.provenance.executed_at) else {
            report.malformed_line_count += 1;
            continue;
        };
        let observed_at = observed_at.with_timezone(&Utc);
        if observed_at > assessed_at {
            continue;
        }

        report.valid_event_count += 1;
        increment(
            &mut report.daily_volume,
            observed_at.date_naive().to_string(),
        );
        increment(
            &mut report.source_distribution,
            entry.source.as_str().to_string(),
        );
        increment(
            &mut report.target_store_distribution,
            entry.target.store().to_string(),
        );
        increment(
            &mut report.rating_distribution,
            entry.rating.map_or_else(
                || "unrated".to_string(),
                |rating| rating.as_str().to_string(),
            ),
        );
        report.first_observed_at = Some(
            report
                .first_observed_at
                .map_or(observed_at, |first| first.min(observed_at)),
        );
        report.last_observed_at = Some(
            report
                .last_observed_at
                .map_or(observed_at, |last| last.max(observed_at)),
        );
        events.push(FeedbackAnalyticEvent::new(
            observed_at,
            entry.rating,
            Some(IncidentIdentity {
                tool: entry.target.store().to_string(),
                operation: Some(entry.target.entity().to_string()),
            }),
        ));
    }

    let identities: BTreeMap<_, _> = events
        .iter()
        .filter_map(|event| event.incident.as_ref())
        .map(|identity| {
            (
                (identity.tool.clone(), identity.operation.clone()),
                identity.clone(),
            )
        })
        .collect();
    report.incidents = identities
        .into_values()
        .filter_map(|identity| RecurringIncident::from_events(&events, &identity, assessed_at))
        .collect();
    report
}

fn increment(distribution: &mut BTreeMap<String, usize>, key: String) {
    *distribution.entry(key).or_default() += 1;
}

impl RecurringIncident {
    /// Groups negative observations by exact tool and operation identity.
    /// `events` must contain only parseable records, so the quiet-window count
    /// measures valid feedback events rather than physical input lines.
    pub fn from_events(
        events: &[FeedbackAnalyticEvent],
        identity: &IncidentIdentity,
        assessed_at: DateTime<Utc>,
    ) -> Option<Self> {
        let negative_events: Vec<&FeedbackAnalyticEvent> = events
            .iter()
            .filter(|event| {
                event.observed_at <= assessed_at
                    && event.classification == FeedbackClassification::Negative
                    && event.incident.as_ref() == Some(identity)
            })
            .collect();
        let first_seen = negative_events
            .iter()
            .map(|event| event.observed_at)
            .min()?;
        let last_seen = negative_events
            .iter()
            .map(|event| event.observed_at)
            .max()?;

        let resolution = explicit_resolution(events, identity, last_seen, assessed_at).map_or_else(
            || tentative_resolution(events, last_seen, assessed_at),
            |resolved_at| IncidentResolution::Explicit { resolved_at },
        );

        Some(Self {
            identity: identity.clone(),
            negative_event_count: negative_events.len(),
            first_seen,
            last_seen,
            resolution,
        })
    }
}

fn explicit_resolution(
    events: &[FeedbackAnalyticEvent],
    identity: &IncidentIdentity,
    last_seen: DateTime<Utc>,
    assessed_at: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    events
        .iter()
        .filter(|event| event.incident.as_ref() == Some(identity))
        .filter_map(|event| event.explicit_resolution_at)
        .filter(|resolved_at| *resolved_at >= last_seen && *resolved_at <= assessed_at)
        .max()
}

fn tentative_resolution(
    events: &[FeedbackAnalyticEvent],
    last_seen: DateTime<Utc>,
    assessed_at: DateTime<Utc>,
) -> IncidentResolution {
    let quiet_window_end = last_seen + Duration::days(TENTATIVE_RESOLUTION_QUIET_DAYS);
    let valid_event_count = events
        .iter()
        .filter(|event| {
            event.observed_at > last_seen
                && event.observed_at <= quiet_window_end
                && event.observed_at <= assessed_at
        })
        .count();

    if assessed_at >= quiet_window_end
        && valid_event_count >= TENTATIVE_RESOLUTION_MINIMUM_VALID_EVENTS
    {
        IncidentResolution::Tentative { assessed_at }
    } else {
        IncidentResolution::Unresolved
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::TimeZone;

    use super::*;
    use crate::{
        EntityUrn, FEEDBACK_SCHEMA_VERSION, FeedbackEntry, FeedbackProvenance, FeedbackSource,
        FeedbackStatus,
    };

    fn timestamp(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, day, 0, 0, 0).unwrap()
    }

    fn event(
        day: u32,
        rating: FeedbackRating,
        identity: Option<IncidentIdentity>,
    ) -> FeedbackAnalyticEvent {
        FeedbackAnalyticEvent::new(timestamp(day), Some(rating), identity)
    }

    fn persisted_entry(day: u32, rating: FeedbackRating, target_store: &str) -> FeedbackEntry {
        FeedbackEntry {
            id: format!("entry-{day}-{target_store}"),
            schema_version: FEEDBACK_SCHEMA_VERSION,
            source: FeedbackSource::TranscriptMined,
            target: EntityUrn::new("default", target_store, "target-a").unwrap(),
            rating: Some(rating),
            note_text: None,
            note_kind: None,
            provenance: FeedbackProvenance::new(
                Some("session-a".to_string()),
                Some("copilot".to_string()),
                Some(timestamp(day).to_rfc3339()),
            )
            .unwrap(),
            status: FeedbackStatus::New,
        }
    }

    #[test]
    fn analytics_classification_maps_ratings_stably() {
        assert_eq!(
            FeedbackClassification::from(Some(FeedbackRating::Helpful)),
            FeedbackClassification::Positive
        );
        assert_eq!(
            FeedbackClassification::from(Some(FeedbackRating::Mixed)),
            FeedbackClassification::Neutral
        );
        assert_eq!(
            FeedbackClassification::from(Some(FeedbackRating::NotHelpful)),
            FeedbackClassification::Negative
        );
    }

    #[test]
    fn analytics_incident_groups_matching_negative_events_and_bounds_time() {
        let identity = IncidentIdentity::new("ticket-mcp", Some("get".to_string())).unwrap();
        let other_identity = IncidentIdentity::new("ticket-mcp", Some("list".to_string())).unwrap();
        let events = vec![
            event(1, FeedbackRating::NotHelpful, Some(identity.clone())),
            event(2, FeedbackRating::Helpful, Some(identity.clone())),
            event(3, FeedbackRating::NotHelpful, Some(other_identity)),
            event(4, FeedbackRating::NotHelpful, Some(identity.clone())),
        ];

        let incident = RecurringIncident::from_events(&events, &identity, timestamp(4)).unwrap();

        assert_eq!(incident.negative_event_count, 2);
        assert_eq!(incident.first_seen, timestamp(1));
        assert_eq!(incident.last_seen, timestamp(4));
    }

    #[test]
    fn analytics_incident_uses_explicit_resolution_before_tentative_resolution() {
        let identity = IncidentIdentity::new("ticket-mcp", Some("get".to_string())).unwrap();
        let events = vec![
            event(1, FeedbackRating::NotHelpful, Some(identity.clone())),
            event(2, FeedbackRating::Helpful, Some(identity.clone()))
                .with_explicit_resolution(timestamp(2)),
        ];

        let incident = RecurringIncident::from_events(&events, &identity, timestamp(15)).unwrap();

        assert_eq!(
            incident.resolution,
            IncidentResolution::Explicit {
                resolved_at: timestamp(2)
            }
        );
    }

    #[test]
    fn analytics_incident_marks_ten_quiet_days_with_one_hundred_events_tentative() {
        let identity = IncidentIdentity::new("ticket-mcp", Some("get".to_string())).unwrap();
        let mut events = vec![event(1, FeedbackRating::NotHelpful, Some(identity.clone()))];
        events.extend((0..100).map(|_| event(10, FeedbackRating::Helpful, None)));

        let incident = RecurringIncident::from_events(&events, &identity, timestamp(11)).unwrap();

        assert_eq!(
            incident.resolution,
            IncidentResolution::Tentative {
                assessed_at: timestamp(11)
            }
        );
    }

    #[test]
    fn analytics_incident_requires_one_hundred_valid_events_for_tentative_resolution() {
        let identity = IncidentIdentity::new("ticket-mcp", Some("get".to_string())).unwrap();
        let mut events = vec![event(1, FeedbackRating::NotHelpful, Some(identity.clone()))];
        events.extend((0..99).map(|_| event(10, FeedbackRating::Helpful, None)));

        let incident = RecurringIncident::from_events(&events, &identity, timestamp(11)).unwrap();

        assert_eq!(incident.resolution, IncidentResolution::Unresolved);
    }

    #[test]
    fn analytics_report_counts_valid_records_and_malformed_lines_separately() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("entries.ndjson");
        let first = persisted_entry(1, FeedbackRating::Helpful, "rule");
        let second = persisted_entry(2, FeedbackRating::NotHelpful, "ticket");
        fs::write(
            &path,
            format!(
                "{}\n{{not json}}\n\n{}\n",
                serde_json::to_string(&first).unwrap(),
                serde_json::to_string(&second).unwrap(),
            ),
        )
        .unwrap();

        let report = analyze_feedback_ndjson(&path, timestamp(12)).unwrap();

        assert_eq!(report.valid_event_count, 2);
        assert_eq!(report.malformed_line_count, 2);
        assert_eq!(report.daily_volume.get("2026-01-01"), Some(&1));
        assert_eq!(report.source_distribution.get("transcript-mined"), Some(&2));
        assert_eq!(report.target_store_distribution.get("rule"), Some(&1));
        assert_eq!(report.rating_distribution.get("not-helpful"), Some(&1));
        assert_eq!(report.first_observed_at, Some(timestamp(1)));
        assert_eq!(report.last_observed_at, Some(timestamp(2)));
        assert_eq!(report.incidents.len(), 1);
    }
}
