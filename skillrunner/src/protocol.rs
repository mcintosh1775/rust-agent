use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityGrant {
    pub capability: String,
    pub scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvokeContext {
    pub tenant_id: String,
    pub run_id: String,
    pub step_id: String,
    pub time_budget_ms: u64,
    #[serde(default)]
    pub granted_capabilities: Vec<CapabilityGrant>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionRequest {
    pub action_id: String,
    pub action_type: String,
    pub args: Value,
    pub justification: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvokeRequest {
    pub id: String,
    pub context: InvokeContext,
    pub input: Value,
}

impl InvokeRequest {
    pub fn into_message(self) -> SkillMessage {
        SkillMessage::Invoke {
            id: self.id,
            context: self.context,
            input: self.input,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvokeResult {
    pub id: String,
    pub output: Value,
    #[serde(default)]
    pub action_requests: Vec<ActionRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDescription {
    pub name: String,
    pub version: String,
    pub description: String,
    pub inputs_schema: Value,
    pub outputs_schema: Value,
    #[serde(default)]
    pub requested_capabilities: Vec<CapabilityGrant>,
    #[serde(default)]
    pub action_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub details: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SkillMessage {
    Describe {
        id: String,
    },
    DescribeResult {
        id: String,
        skill: SkillDescription,
    },
    Invoke {
        id: String,
        context: InvokeContext,
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

impl SkillMessage {
    pub fn encode_ndjson(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut bytes = serde_json::to_vec(self)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub fn decode_ndjson(line: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(line)
    }
}

impl From<InvokeResult> for SkillMessage {
    fn from(value: InvokeResult) -> Self {
        Self::InvokeResult {
            id: value.id,
            output: value.output,
            action_requests: value.action_requests,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActionRequest, CapabilityGrant, InvokeContext, InvokeRequest, InvokeResult, SkillMessage,
    };
    use serde_json::json;

    #[test]
    fn invoke_round_trip_ndjson() {
        let request = InvokeRequest {
            id: "req-1".to_string(),
            context: InvokeContext {
                tenant_id: "single".to_string(),
                run_id: "run-1".to_string(),
                step_id: "step-1".to_string(),
                time_budget_ms: 5_000,
                granted_capabilities: vec![CapabilityGrant {
                    capability: "object.write".to_string(),
                    scope: "shownotes/*".to_string(),
                }],
            },
            input: json!({"text":"hello"}),
        };

        let encoded = request
            .clone()
            .into_message()
            .encode_ndjson()
            .expect("encode invoke request");
        let decoded = SkillMessage::decode_ndjson(&encoded).expect("decode invoke request");

        assert_eq!(decoded, request.into_message());
    }

    #[test]
    fn invoke_result_round_trip_ndjson() {
        let result = InvokeResult {
            id: "req-2".to_string(),
            output: json!({"markdown":"# Summary"}),
            action_requests: vec![ActionRequest {
                action_id: "a-1".to_string(),
                action_type: "object.write".to_string(),
                args: json!({"path":"shownotes/ep245.md","content":"# Summary"}),
                justification: "Persist output".to_string(),
            }],
        };

        let encoded = SkillMessage::from(result.clone())
            .encode_ndjson()
            .expect("encode invoke result");
        let decoded = SkillMessage::decode_ndjson(&encoded).expect("decode invoke result");

        assert_eq!(decoded, SkillMessage::from(result));
    }
}
