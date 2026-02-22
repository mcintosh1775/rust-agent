use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SkillMessage {
    Describe {
        id: String,
    },
    DescribeResult {
        id: String,
        skill: SkillDescription,
    },
    Invoke {
        id: String,
        #[serde(default)]
        input: Value,
    },
    InvokeResult {
        id: String,
        output: Value,
        #[serde(default)]
        action_requests: Vec<ActionRequest>,
    },
    Error {
        id: String,
        error: ProtocolError,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct ProtocolError {
    code: String,
    message: String,
    #[serde(default)]
    details: Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct ActionRequest {
    action_id: String,
    action_type: String,
    args: Value,
    justification: String,
    #[serde(default)]
    action_contract_version: Option<String>,
    #[serde(default)]
    action_schema_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SkillDescription {
    name: String,
    version: String,
    description: String,
    inputs_schema: Value,
    outputs_schema: Value,
    #[serde(default)]
    requested_capabilities: Vec<CapabilityGrant>,
    #[serde(default)]
    action_types: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CapabilityGrant {
    capability: String,
    scope: String,
}

#[derive(Debug, Serialize)]
struct AuditVerificationIssue {
    index: usize,
    event_id: Option<String>,
    code: String,
    detail: String,
    severity: String,
}

#[derive(Debug)]
struct VerifyConfig {
    seed: String,
    seq_field: String,
    prev_field: String,
    hash_field: String,
}

fn main() {
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        return;
    }
    let message = match serde_json::from_str::<SkillMessage>(&input) {
        Ok(message) => message,
        Err(error) => {
            let _ = emit(
                &SkillMessage::Error {
                    id: "invalid".into(),
                    error: ProtocolError {
                        code: "INVALID_JSON".into(),
                        message: error.to_string(),
                        details: Value::Null,
                    },
                },
                false,
            );
            return;
        }
    };

    match message {
        SkillMessage::Describe { id } => {
            emit(
                &SkillMessage::DescribeResult {
                    id,
                    skill: describe_skill(),
                },
                true,
            );
        }
        SkillMessage::Invoke { id, input } => {
            emit(&invoke(input, id), true);
        }
        SkillMessage::DescribeResult { .. }
        | SkillMessage::InvokeResult { .. }
        | SkillMessage::Error { .. } => {}
    }
}

fn emit(message: &SkillMessage, newline: bool) {
    let payload = serde_json::to_string(message).unwrap_or_else(|_| {
        r#"{"type":"error","id":"internal","error":{"code":"ENCODE_FAILED","message":"failed to encode message","details":{}}}"#.to_string()
    });
    if newline {
        println!("{payload}");
    } else {
        print!("{payload}");
    }
}

fn describe_skill() -> SkillDescription {
    SkillDescription {
        name: "audit_chain_verifier".into(),
        version: "0.1.0".into(),
        description: "Verify deterministic tamper-evidence chain continuity from event arrays.".into(),
        inputs_schema: serde_json::json!({
            "type": "object",
            "required": ["events"],
            "properties": {
                "events": {
                    "type": "array",
                    "items": {
                        "type": "object"
                    },
                    "description": "Array of chain events with sequence/hash metadata."
                },
                "seed": {"type": "string"},
                "seq_field": {"type": "string"},
                "prev_hash_field": {"type": "string"},
                "hash_field": {"type": "string"},
                "request_write": {"type": "boolean"}
            }
        }),
        outputs_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "markdown": {"type": "string"},
                "skill": {"type": "string"},
                "generated_at": {"type": "string"},
                "chain_valid": {"type": "boolean"},
                "issues": {"type": "array"},
                "verified_count": {"type": "integer"},
                "failed_count": {"type": "integer"},
                "available_skills": {"type": "array", "items": {"type": "string"}},
            }
        }),
        requested_capabilities: vec![],
        action_types: vec![],
    }
}

fn invoke(input: Value, id: String) -> SkillMessage {
    match parse_config(&input) {
        Ok(config) => {
            let events = input.get("events").and_then(Value::as_array);
            if events.is_none() {
                return SkillMessage::Error {
                    id,
                    error: ProtocolError {
                        code: "INVALID_INPUT".into(),
                        message: "Missing required field `events` as an array.".into(),
                        details: serde_json::json!({"path":"events"}),
                    },
                };
            }

            let (verified_count, issues, valid) = verify_chain(events.expect("events checked"), &config);
            SkillMessage::InvokeResult {
                id,
                output: serde_json::json!({
                    "markdown": render_markdown(&issues, verified_count, valid, &config),
                    "skill": "audit_chain_verifier",
                    "generated_at": timestamp_utc(),
                    "chain_valid": valid,
                    "issues": issues,
                    "verified_count": verified_count,
                    "failed_count": issues.len(),
                    "available_skills": [],
                }),
                action_requests: maybe_build_write_action(&input),
            }
        }
        Err(error) => SkillMessage::Error {
            id,
            error: ProtocolError {
                code: "INVALID_INPUT".into(),
                message: error.to_string(),
                details: serde_json::json!({"field":"input"}),
            },
        },
    }
}

fn parse_config(input: &Value) -> Result<VerifyConfig, String> {
    let object = input.as_object().ok_or_else(|| "Input must be a JSON object.".to_string())?;

    let events = object.get("events");
    if events.is_none() {
        return Err("Missing `events` field.".to_string());
    }

    Ok(VerifyConfig {
        seed: value_to_string(object.get("seed"), "GENESIS"),
        seq_field: value_to_string(object.get("seq_field"), "tamper_chain_seq"),
        prev_field: value_to_string(object.get("prev_hash_field"), "tamper_prev_hash"),
        hash_field: value_to_string(object.get("hash_field"), "tamper_hash"),
    })
}

fn value_to_string(value: Option<&Value>, default_value: &str) -> String {
    let prepared = value.and_then(|value| match value {
        Value::String(raw) if !raw.trim().is_empty() => Some(raw.to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    });
    prepared
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| default_value.to_string())
}

fn verify_chain(events: &[Value], config: &VerifyConfig) -> (usize, Vec<AuditVerificationIssue>, bool) {
    let mut issues = Vec::new();
    let mut expected_prev = config.seed.clone();
    let mut verified_count = 0usize;

    for (index, event_value) in events.iter().enumerate() {
        let event = match event_value.as_object() {
            Some(event) => event,
            None => {
                issues.push(AuditVerificationIssue {
                    index,
                    event_id: None,
                    code: "INVALID_EVENT".into(),
                    detail: "Event must be an object.".into(),
                    severity: "high".into(),
                });
                continue;
            }
        };

        let event_id = event
            .get("id")
            .and_then(Value::as_str)
            .or_else(|| event.get("event_id").and_then(Value::as_str))
            .map(ToString::to_string);

        let provided_prev = event
            .get(&config.prev_field)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let provided_hash = event
            .get(&config.hash_field)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let seq_token = event
            .get(&config.seq_field)
            .map(|value| value_to_string(Some(value), &""))
            .unwrap_or_else(|| index.to_string());

        if provided_prev != expected_prev {
            issues.push(AuditVerificationIssue {
                index,
                event_id: event_id.clone(),
                code: "CHAIN_PREV_MISMATCH".into(),
                detail: format!(
                    "Expected prev hash `{}` but received `{}`.",
                    expected_prev, provided_prev
                ),
                severity: "high".into(),
            });
        }

        let payload = scrub_event(event, &config);
        let expected_hash = sha256_hex(format!("{}|{}|{}", seq_token, expected_prev, payload));
        if provided_hash != expected_hash {
            issues.push(AuditVerificationIssue {
                index,
                event_id,
                code: "HASH_MISMATCH".into(),
                detail: format!(
                    "Expected tamper hash `{}` but received `{}`.",
                    expected_hash, provided_hash
                ),
                severity: if provided_prev != expected_prev { "high" } else { "medium" }.into(),
            });
        } else if provided_prev == expected_prev {
            verified_count += 1;
        }

        expected_prev = provided_hash;
    }

    let is_valid = issues.is_empty();
    (verified_count, issues, is_valid)
}

fn scrub_event(event: &Map<String, Value>, config: &VerifyConfig) -> String {
    let mut map: BTreeMap<String, Value> = BTreeMap::new();
    for (key, value) in event {
        if key == &config.hash_field || key == &config.prev_field {
            continue;
        }
        if key == &config.seq_field {
            continue;
        }
        if key == "id" || key == "event_id" {
            continue;
        }
        map.insert(key.clone(), canonical_value(value));
    }
    let canonical = canonicalize_payload(&map);
    canonical.to_string()
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Object(source) => {
            let mut map = BTreeMap::new();
            for (key, value) in source {
                map.insert(key.clone(), canonical_value(value));
            }
            Value::Object(
                map.into_iter()
                    .map(|(key, value)| (key, value))
                    .collect::<Map<String, Value>>(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_value).collect()),
        Value::Number(_) | Value::String(_) | Value::Bool(_) | Value::Null => value.clone(),
    }
}

fn canonicalize_payload(value: &BTreeMap<String, Value>) -> Value {
    let mut map = Map::new();
    for (key, value) in value {
        map.insert(key.clone(), value.clone());
    }
    Value::Object(map)
}

fn sha256_hex(input: String) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn render_markdown(
    issues: &[AuditVerificationIssue],
    verified_count: usize,
    valid: bool,
    config: &VerifyConfig,
) -> String {
    let mut lines = Vec::new();
    lines.push("# Audit Chain Verification Report".to_string());
    lines.push("".to_string());
    lines.push(format!("- Seed: `{}`", config.seed));
    lines.push(format!(
        "- Status: `{}`",
        if valid { "VALID" } else { "INVALID" }
    ));
    lines.push(format!("- Verified events: `{verified_count}`"));
    lines.push(format!("- Failing events: `{}`", issues.len()));
    lines.push("".to_string());

    if issues.is_empty() {
        lines.push("## Result".to_string());
        lines.push("- No chain violations detected.".to_string());
    } else {
        lines.push("## Violations".to_string());
        for issue in issues {
            lines.push(format!(
                "- [{}] {} (index {}/id {:?}): {}",
                issue.severity, issue.code, issue.index, issue.event_id, issue.detail
            ));
        }
    }
    lines.push("".to_string());
    lines.push(format!("_Generated at {}_", timestamp_utc()));
    lines.join("\n")
}

fn maybe_build_write_action(input: &Value) -> Vec<ActionRequest> {
    let request_write = input.get("request_write").and_then(Value::as_bool).unwrap_or(false);
    if !request_write {
        return Vec::new();
    }
    vec![ActionRequest {
        action_id: "audit-chain-write".into(),
        action_type: "object.write".into(),
        args: serde_json::json!({
            "path": "audit_chain_verification.md",
            "content": "# Audit Chain Verification"
        }),
        justification: "Persist audit chain verification result for operator follow-up."
            .into(),
        action_contract_version: None,
        action_schema_id: None,
    }]
}

fn timestamp_utc() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", now.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_event(seq: u64, prev: &str, data: &str) -> Value {
        let hash_input = format!("{}|{}|{{\"data\":\"{}\"}}", seq, prev, data);
        let hash = sha256_hex(hash_input);
        serde_json::json!({
            "tamper_chain_seq": seq,
            "tamper_prev_hash": prev,
            "tamper_hash": hash,
            "id": format!("evt-{}", seq),
            "data": data,
        })
    }

    #[test]
    fn verifies_valid_chain() {
        let seed = "seed";
        let first = mk_event(1, seed, "ingest");
        let second = mk_event(2, first["tamper_hash"].as_str().unwrap(), "route");
        let events = vec![first.clone(), second];
        let config = VerifyConfig {
            seed: seed.into(),
            seq_field: "tamper_chain_seq".into(),
            prev_field: "tamper_prev_hash".into(),
            hash_field: "tamper_hash".into(),
        };
        let (verified, issues, valid) = verify_chain(&events, &config);
        assert!(valid);
        assert_eq!(issues.len(), 0);
        assert_eq!(verified, 2);
    }

    #[test]
    fn flags_prev_mismatch_and_hash_mismatch() {
        let seed = "seed";
        let mut first = mk_event(1, seed, "ingest");
        let second = mk_event(2, first["tamper_hash"].as_str().unwrap(), "route");
        if let Some(obj) = first.as_object_mut() {
            obj.insert("tamper_prev_hash".into(), serde_json::json!("wrong-prev"));
        }
        let events = vec![first, second];
        let config = VerifyConfig {
            seed: seed.into(),
            seq_field: "tamper_chain_seq".into(),
            prev_field: "tamper_prev_hash".into(),
            hash_field: "tamper_hash".into(),
        };
        let (_verified, issues, valid) = verify_chain(&events, &config);
        assert!(!valid);
        assert!(issues.len() >= 1);
    }

    #[test]
    fn rejects_non_object_events() {
        let config = VerifyConfig {
            seed: "seed".into(),
            seq_field: "tamper_chain_seq".into(),
            prev_field: "tamper_prev_hash".into(),
            hash_field: "tamper_hash".into(),
        };
        let events = vec![serde_json::json!("bad-event")];
        let (verified, issues, valid) = verify_chain(&events, &config);
        assert!(!valid);
        assert_eq!(verified, 0);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "INVALID_EVENT");
    }
}
