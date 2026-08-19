use super::*;

pub(super) fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

/// Resolve the effective note kind for a feedback event: a note kind requires
/// note text, and note text without an explicit kind defaults to `Note`.
pub(super) fn resolve_note_kind(
    note_text: Option<&str>,
    note_kind: Option<FeedbackNoteKind>,
) -> Result<Option<FeedbackNoteKind>, String> {
    match (note_text, note_kind) {
        (Some(_), Some(kind)) => Ok(Some(kind)),
        (Some(_), None) => Ok(Some(FeedbackNoteKind::Note)),
        (None, None) => Ok(None),
        (None, Some(_)) =>
            Err("feedback note kind requires feedback note text".to_string()),
    }
}

pub(super) fn append_ndjson<T: Serialize>(
    path: &Path,
    item: &T,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create feedback core directory {}: {err}",
                parent.display()
            )
        })?;
    }

    let line = serde_json::to_string(item)
        .map_err(|err| format!("failed to serialize ndjson item: {err}"))?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| {
            format!(
                "failed to open feedback core log {}: {err}",
                path.display()
            )
        })?;
    writeln!(file, "{line}").map_err(|err| {
        format!(
            "failed to append feedback core log {}: {err}",
            path.display()
        )
    })
}

pub(super) fn read_ndjson<T>(path: &Path) -> Result<Vec<T>, String>
where
    T: for<'de> Deserialize<'de>,
{
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = fs::File::open(path).map_err(|err| {
        format!("failed to open feedback core log {}: {err}", path.display())
    })?;
    let reader = BufReader::new(file);
    let mut items = Vec::new();

    for (index, line) in reader.lines().enumerate() {
        let line = line.map_err(|err| {
            format!(
                "failed reading feedback core log {} line {}: {err}",
                path.display(),
                index + 1
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let item = serde_json::from_str::<T>(&line).map_err(|err| {
            format!(
                "invalid feedback core event {} line {}: {err}",
                path.display(),
                index + 1
            )
        })?;
        items.push(item);
    }

    Ok(items)
}

/// Rewrite an NDJSON event log in place, keeping only the events allowed by a
/// retention policy. Events are assumed to be appended in chronological order;
/// the age filter drops events older than `now - max_age`, and `max_events`
/// then keeps the most recent surviving events. Returns retained/removed
/// counts. Applying the same policy again removes nothing.
pub(super) fn prune_ndjson<T>(
    path: &Path,
    policy: &RetentionPolicy,
    now: chrono::DateTime<Utc>,
    timestamp_of: impl Fn(&T) -> &str,
) -> Result<RetentionKindOutcome, String>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    let events: Vec<T> = read_ndjson(path)?;
    let original = events.len();
    if original == 0 {
        return Ok(RetentionKindOutcome::default());
    }

    let mut kept: Vec<T> = Vec::with_capacity(original);
    for event in events {
        if let Some(max_age) = policy.max_age {
            let raw = timestamp_of(&event);
            let parsed =
                chrono::DateTime::parse_from_rfc3339(raw).map_err(|err| {
                    format!(
                        "invalid feedback core timestamp '{raw}' in {}: {err}",
                        path.display()
                    )
                })?;
            if now.signed_duration_since(parsed.with_timezone(&Utc)) > max_age {
                continue;
            }
        }
        kept.push(event);
    }

    if let Some(max_events) = policy.max_events
        && kept.len() > max_events
    {
        let overflow = kept.len() - max_events;
        kept.drain(0..overflow);
    }

    let retained = kept.len();
    let removed = original - retained;

    if removed > 0 {
        rewrite_ndjson(path, &kept)?;
    }

    Ok(RetentionKindOutcome { retained, removed })
}

/// Atomically replace an NDJSON log with the provided items.
pub(super) fn rewrite_ndjson<T: Serialize>(
    path: &Path,
    items: &[T],
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create feedback core directory {}: {err}",
                parent.display()
            )
        })?;
    }

    let mut buffer = String::new();
    for item in items {
        let line = serde_json::to_string(item)
            .map_err(|err| format!("failed to serialize ndjson item: {err}"))?;
        buffer.push_str(&line);
        buffer.push('\n');
    }

    let tmp_path = path.with_extension("ndjson.tmp");
    fs::write(&tmp_path, &buffer).map_err(|err| {
        format!(
            "failed to write feedback core log {}: {err}",
            tmp_path.display()
        )
    })?;
    fs::rename(&tmp_path, path).map_err(|err| {
        format!(
            "failed to replace feedback core log {}: {err}",
            path.display()
        )
    })
}
