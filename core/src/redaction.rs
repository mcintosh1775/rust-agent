use serde_json::{Map, Value};

const REDACTED: &str = "[REDACTED]";
const SENSITIVE_KEY_PATTERNS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "authorization",
    "cookie",
    "private_key",
    "nsec",
];

pub fn redact_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (key, val) in map {
                if is_sensitive_key(key) {
                    out.insert(key.clone(), Value::String(REDACTED.to_string()));
                } else {
                    out.insert(key.clone(), redact_json(val));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(redact_json).collect()),
        Value::String(text) => Value::String(redact_text(text)),
        _ => value.clone(),
    }
}

pub fn redact_text(text: &str) -> String {
    let nsec_redacted = redact_prefixed_token(text, "nsec1");
    redact_bearer_tokens(&nsec_redacted)
}

pub fn redact_memory_content(
    content_json: &Value,
    summary_text: Option<&str>,
) -> (Value, Option<String>, bool) {
    let redacted_content = redact_json(content_json);
    let redacted_summary = summary_text.map(redact_text);
    let content_changed = redacted_content != *content_json;
    let summary_changed = summary_text
        .zip(redacted_summary.as_deref())
        .is_some_and(|(original, redacted)| original != redacted);
    (
        redacted_content,
        redacted_summary,
        content_changed || summary_changed,
    )
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    SENSITIVE_KEY_PATTERNS
        .iter()
        .any(|pattern| key.contains(pattern))
}

fn redact_prefixed_token(input: &str, prefix: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut idx = 0usize;
    while idx < input.len() {
        let Some(found) = input[idx..].find(prefix) else {
            out.push_str(&input[idx..]);
            break;
        };

        let start = idx + found;
        out.push_str(&input[idx..start]);

        let mut end = start + prefix.len();
        while end < input.len() && is_token_char(input.as_bytes()[end]) {
            end += 1;
        }

        // Only redact likely bech32-like payloads, not short accidental matches.
        if end - start >= prefix.len() + 16 {
            out.push_str(REDACTED);
        } else {
            out.push_str(&input[start..end]);
        }
        idx = end;
    }
    out
}

fn redact_bearer_tokens(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut idx = 0usize;
    while idx < input.len() {
        let Some(found) = find_case_insensitive(&input[idx..], "bearer ") else {
            out.push_str(&input[idx..]);
            break;
        };

        let marker = idx + found;
        out.push_str(&input[idx..marker]);
        out.push_str(&input[marker..marker + 7]); // keep "Bearer " spelling from source

        let mut token_end = marker + 7;
        while token_end < input.len() && !is_terminator(input.as_bytes()[token_end]) {
            token_end += 1;
        }
        out.push_str(REDACTED);
        idx = token_end;
    }
    out
}

fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let hay_lower = haystack.to_ascii_lowercase();
    hay_lower.find(&needle.to_ascii_lowercase())
}

fn is_token_char(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit()
}

fn is_terminator(b: u8) -> bool {
    b.is_ascii_whitespace() || matches!(b, b'"' | b'\'' | b',' | b';' | b')' | b']')
}

#[cfg(test)]
mod tests {
    use super::{redact_json, redact_memory_content, redact_text};
    use serde_json::json;

    #[test]
    fn redacts_sensitive_object_keys() {
        let input = json!({
            "token": "abc",
            "nested": {
                "api_key": "xyz",
                "safe": "ok"
            }
        });
        let redacted = redact_json(&input);
        assert_eq!(redacted["token"], "[REDACTED]");
        assert_eq!(redacted["nested"]["api_key"], "[REDACTED]");
        assert_eq!(redacted["nested"]["safe"], "ok");
    }

    #[test]
    fn redacts_nsec_and_bearer_tokens_in_text() {
        let input = "auth Bearer very-secret-token and nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq";
        let redacted = redact_text(input);
        assert!(redacted.contains("Bearer [REDACTED]"));
        assert!(!redacted.contains("very-secret-token"));
        assert!(!redacted.contains("nsec1qqqq"));
    }

    #[test]
    fn redacts_memory_payload_and_marks_redaction_applied() {
        let (content, summary, applied) = redact_memory_content(
            &json!({
                "token": "abc",
                "notes": "Bearer top-secret-token"
            }),
            Some("Use nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq"),
        );

        assert!(applied);
        assert_eq!(content["token"], "[REDACTED]");
        assert_eq!(content["notes"], "Bearer [REDACTED]");
        assert_eq!(summary.as_deref(), Some("Use [REDACTED]"));
    }
}
