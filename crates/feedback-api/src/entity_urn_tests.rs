//! Regression tests for feedback targets (session 4e24320a): `feedback_ingest`
//! rejected `file://` and `tool://` targets because `EntityUrn` only parsed
//! `ce://<workspace>/<store>/<entity>`.

use std::str::FromStr;

use crate::EntityUrn;

fn parse(value: &str) -> Result<EntityUrn, String> {
    EntityUrn::from_str(value)
}

// --- exact failure scenario -------------------------------------------------

#[test]
fn file_target_from_failing_session_parses() {
    let urn = parse("file://C:/x/y.rs").expect("file target must parse");
    assert_eq!(urn.store(), "file");
    assert_eq!(urn.entity(), "C:/x/y.rs");
    assert_eq!(urn.workspace(), "");
    assert_eq!(urn.to_string(), "file://C:/x/y.rs");
}

#[test]
fn tool_target_from_failing_session_parses() {
    let urn = parse("tool://session-mcp/session_peek_skeleton").expect("tool target must parse");
    assert_eq!(urn.store(), "tool");
    assert_eq!(urn.entity(), "session-mcp/session_peek_skeleton");
    assert_eq!(urn.to_string(), "tool://session-mcp/session_peek_skeleton");
}

// --- entity form ------------------------------------------------------------

#[test]
fn entity_scheme_parses_and_round_trips() {
    let urn = parse("entity://memory-api/rule/some-rule").unwrap();
    assert_eq!(urn.workspace(), "memory-api");
    assert_eq!(urn.store(), "rule");
    assert_eq!(urn.entity(), "some-rule");
    assert_eq!(urn.to_string(), "entity://memory-api/rule/some-rule");
    assert_eq!(parse(&urn.to_string()).unwrap(), urn);
}

#[test]
fn legacy_ce_scheme_parses_and_emits_entity_scheme() {
    let legacy = parse("ce://memory-api/rule/some-rule").unwrap();
    let current = parse("entity://memory-api/rule/some-rule").unwrap();
    assert_eq!(legacy, current);
    assert_eq!(legacy.to_string(), "entity://memory-api/rule/some-rule");
}

#[test]
fn constructors_emit_entity_scheme() {
    assert_eq!(
        EntityUrn::ticket("default", "abc").unwrap().to_string(),
        "entity://default/ticket/abc"
    );
}

#[test]
fn percent_encoded_workspace_round_trips() {
    let urn = EntityUrn::rule("C:/repo root", "r1").unwrap();
    let rendered = urn.to_string();
    assert!(rendered.starts_with("entity://C%3A%2Frepo%20root/rule/"));
    assert_eq!(parse(&rendered).unwrap(), urn);
}

#[test]
fn entity_scheme_rejects_wrong_segment_count() {
    for value in [
        "entity://ws/rule",
        "entity://ws",
        "entity://ws/rule/x/extra",
        "ce://ws/rule/x/extra",
        "entity://ws//x",
        "entity:///rule/x",
    ] {
        assert!(parse(value).is_err(), "'{value}' must be rejected");
    }
}

// --- external form edge cases -----------------------------------------------

#[test]
fn rejects_missing_separator_and_empty_parts() {
    for value in ["", "   ", "C:/x/y.rs", "file:/x", "://value", "file://", "file://   "] {
        assert!(parse(value).is_err(), "'{value}' must be rejected");
    }
}

#[test]
fn rejects_invalid_scheme_characters() {
    for value in ["1file://x", "fi le://x", "fi_le://x", "-file://x", "fi/le://x"] {
        assert!(parse(value).is_err(), "'{value}' must be rejected");
    }
}

#[test]
fn accepts_rfc3986_scheme_characters_and_trims() {
    let urn = parse("  git+ssh.v2-x://host/repo  ").unwrap();
    assert_eq!(urn.store(), "git+ssh.v2-x");
    assert_eq!(urn.entity(), "host/repo");
}

#[test]
fn external_value_keeps_inner_separators_verbatim() {
    let urn = parse("https://example.com/a://b?c=d").unwrap();
    assert_eq!(urn.store(), "https");
    assert_eq!(urn.entity(), "example.com/a://b?c=d");
    assert_eq!(parse(&urn.to_string()).unwrap(), urn);
}

#[test]
fn external_differs_from_entity_with_same_store() {
    let external = parse("rule://ws/rule/x").unwrap();
    let entity = parse("entity://ws/rule/x").unwrap();
    assert_ne!(external, entity);
}

// --- serde compatibility ----------------------------------------------------

#[test]
fn legacy_stored_json_deserializes() {
    let json = r#"{"workspace":"memory-api","store":"rule","entity":"some-rule"}"#;
    let urn: EntityUrn = serde_json::from_str(json).unwrap();
    assert_eq!(urn, EntityUrn::rule("memory-api", "some-rule").unwrap());
    assert_eq!(serde_json::to_string(&urn).unwrap(), json);
}

#[test]
fn external_json_round_trips() {
    let urn = parse("file://C:/x/y.rs").unwrap();
    let json = serde_json::to_string(&urn).unwrap();
    assert_eq!(json, r#"{"scheme":"file","value":"C:/x/y.rs"}"#);
    let back: EntityUrn = serde_json::from_str(&json).unwrap();
    assert_eq!(back, urn);
}

#[test]
fn malformed_json_is_rejected() {
    for json in [r#"{"workspace":"ws","store":"rule"}"#, r#"{"scheme":"file"}"#, "{}"] {
        assert!(serde_json::from_str::<EntityUrn>(json).is_err(), "{json} must be rejected");
    }
}
