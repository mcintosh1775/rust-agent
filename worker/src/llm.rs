use anyhow::{anyhow, Context, Result};
use core::{resolve_secret_value, EnvFileSecretResolver};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{env, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmMode {
    LocalOnly,
    LocalFirst,
    RemoteOnly,
}

impl LlmMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalOnly => "local_only",
            Self::LocalFirst => "local_first",
            Self::RemoteOnly => "remote_only",
        }
    }

    fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "local_only" => Ok(Self::LocalOnly),
            "local_first" | "" => Ok(Self::LocalFirst),
            "remote_only" => Ok(Self::RemoteOnly),
            other => Err(anyhow!(
                "invalid LLM_MODE `{}` (supported: local_only, local_first, remote_only)",
                other
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmRoute {
    Local,
    Remote,
}

impl LlmRoute {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LlmEndpointConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub mode: LlmMode,
    pub timeout: Duration,
    pub max_prompt_bytes: usize,
    pub max_output_bytes: usize,
    pub local: Option<LlmEndpointConfig>,
    pub remote: Option<LlmEndpointConfig>,
    pub remote_egress_enabled: bool,
    pub remote_host_allowlist: Vec<String>,
    pub remote_token_budget_per_run: Option<u64>,
    pub remote_cost_per_1k_tokens_usd: f64,
}

#[derive(Debug, Clone)]
struct LlmInferActionArgs {
    prompt: String,
    system: Option<String>,
    prefer: Option<LlmRoute>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmInferResult {
    pub route: String,
    pub model: String,
    pub response_text: String,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Debug, Deserialize)]
struct ChatMessageResponse {
    content: String,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    total_tokens: Option<u32>,
}

impl LlmConfig {
    pub fn from_env() -> Result<Self> {
        let mode_raw = env::var("LLM_MODE").unwrap_or_else(|_| "local_first".to_string());
        let mode = LlmMode::parse(&mode_raw)?;

        let local_base_url = env::var("LLM_LOCAL_BASE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:11434/v1".to_string());
        let local_model =
            env::var("LLM_LOCAL_MODEL").unwrap_or_else(|_| "qwen2.5:7b-instruct".to_string());
        let local_api_key = read_env_secret("LLM_LOCAL_API_KEY", "LLM_LOCAL_API_KEY_REF")?;

        let local = non_empty_trimmed(local_model.as_str()).map(|model| LlmEndpointConfig {
            base_url: normalize_base_url(&local_base_url),
            model: model.to_string(),
            api_key: local_api_key
                .as_deref()
                .and_then(non_empty_trimmed)
                .map(ToString::to_string),
        });

        let remote = match (
            env::var("LLM_REMOTE_BASE_URL").ok(),
            env::var("LLM_REMOTE_MODEL").ok(),
        ) {
            (Some(base_url), Some(model)) => {
                let model = non_empty_trimmed(model.as_str())
                    .ok_or_else(|| anyhow!("LLM_REMOTE_MODEL must not be empty when provided"))?;
                Some(LlmEndpointConfig {
                    base_url: normalize_base_url(&base_url),
                    model: model.to_string(),
                    api_key: read_env_secret("LLM_REMOTE_API_KEY", "LLM_REMOTE_API_KEY_REF")?
                        .as_deref()
                        .and_then(non_empty_trimmed)
                        .map(ToString::to_string),
                })
            }
            (None, None) => None,
            _ => {
                return Err(anyhow!(
                    "LLM_REMOTE_BASE_URL and LLM_REMOTE_MODEL must be set together"
                ));
            }
        };

        Ok(Self {
            mode,
            timeout: Duration::from_millis(read_env_u64("LLM_TIMEOUT_MS", 12_000)?),
            max_prompt_bytes: read_env_u64("LLM_MAX_PROMPT_BYTES", 32_000)? as usize,
            max_output_bytes: read_env_u64("LLM_MAX_OUTPUT_BYTES", 64_000)? as usize,
            local,
            remote,
            remote_egress_enabled: read_env_bool("LLM_REMOTE_EGRESS_ENABLED", false),
            remote_host_allowlist: read_env_csv("LLM_REMOTE_HOST_ALLOWLIST"),
            remote_token_budget_per_run: read_env_u64_optional("LLM_REMOTE_TOKEN_BUDGET_PER_RUN")?,
            remote_cost_per_1k_tokens_usd: read_env_f64("LLM_REMOTE_COST_PER_1K_TOKENS_USD", 0.0)?,
        })
    }
}

pub fn policy_scope_for_action(args: &Value, config: &LlmConfig) -> Result<String> {
    let parsed = parse_action_args(args, config.max_prompt_bytes)?;
    let route = select_route(config, parsed.prefer)?;
    let endpoint = endpoint_for_route(config, route)?;
    Ok(format!("{}:{}", route.as_str(), endpoint.model))
}

pub async fn execute_llm_infer(args: &Value, config: &LlmConfig) -> Result<LlmInferResult> {
    let parsed = parse_action_args(args, config.max_prompt_bytes)?;
    let route = select_route(config, parsed.prefer)?;
    let endpoint = endpoint_for_route(config, route)?;
    enforce_remote_egress_policy(route, endpoint, config)?;

    let mut messages = Vec::with_capacity(2);
    if let Some(system) = parsed.system {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: system,
        });
    }
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: parsed.prompt,
    });

    let request = ChatCompletionRequest {
        model: endpoint.model.clone(),
        messages,
        max_tokens: parsed.max_tokens,
        temperature: parsed.temperature,
    };
    let url = format!(
        "{}/chat/completions",
        endpoint.base_url.trim_end_matches('/')
    );
    let client = Client::builder()
        .timeout(config.timeout)
        .build()
        .with_context(|| "failed building LLM HTTP client")?;

    let mut req = client.post(&url).json(&request);
    if let Some(api_key) = endpoint.api_key.as_deref() {
        req = req.bearer_auth(api_key);
    }

    let response = req
        .send()
        .await
        .with_context(|| format!("llm.infer request failed for route {}", route.as_str()))?
        .error_for_status()
        .with_context(|| format!("llm.infer endpoint returned error for {}", route.as_str()))?;

    let payload = response
        .json::<ChatCompletionResponse>()
        .await
        .with_context(|| "failed decoding llm.infer response JSON")?;
    let text = payload
        .choices
        .first()
        .map(|choice| choice.message.content.clone())
        .ok_or_else(|| anyhow!("llm.infer response missing choices[0].message.content"))?;

    if text.len() > config.max_output_bytes {
        return Err(anyhow!(
            "llm.infer output exceeded {} bytes",
            config.max_output_bytes
        ));
    }

    Ok(LlmInferResult {
        route: route.as_str().to_string(),
        model: endpoint.model.clone(),
        response_text: text,
        prompt_tokens: payload.usage.as_ref().and_then(|u| u.prompt_tokens),
        completion_tokens: payload.usage.as_ref().and_then(|u| u.completion_tokens),
        total_tokens: payload.usage.and_then(|u| u.total_tokens),
    })
}

fn select_route(config: &LlmConfig, prefer: Option<LlmRoute>) -> Result<LlmRoute> {
    let local_available = config.local.is_some();
    let remote_available = config.remote.is_some();

    match config.mode {
        LlmMode::LocalOnly => {
            if !local_available {
                return Err(anyhow!(
                    "LLM_MODE=local_only but local model is not configured"
                ));
            }
            if matches!(prefer, Some(LlmRoute::Remote)) {
                return Err(anyhow!(
                    "llm.infer prefer=remote is not allowed when LLM_MODE=local_only"
                ));
            }
            Ok(LlmRoute::Local)
        }
        LlmMode::RemoteOnly => {
            if !remote_available {
                return Err(anyhow!(
                    "LLM_MODE=remote_only but remote model is not configured"
                ));
            }
            if matches!(prefer, Some(LlmRoute::Local)) {
                return Err(anyhow!(
                    "llm.infer prefer=local is not allowed when LLM_MODE=remote_only"
                ));
            }
            Ok(LlmRoute::Remote)
        }
        LlmMode::LocalFirst => {
            if matches!(prefer, Some(LlmRoute::Remote)) && remote_available {
                return Ok(LlmRoute::Remote);
            }
            if local_available {
                return Ok(LlmRoute::Local);
            }
            if remote_available {
                return Ok(LlmRoute::Remote);
            }
            Err(anyhow!(
                "no LLM endpoint is configured (set local and/or remote endpoint env vars)"
            ))
        }
    }
}

fn endpoint_for_route(config: &LlmConfig, route: LlmRoute) -> Result<&LlmEndpointConfig> {
    match route {
        LlmRoute::Local => config
            .local
            .as_ref()
            .ok_or_else(|| anyhow!("local route selected but local endpoint is not configured")),
        LlmRoute::Remote => config
            .remote
            .as_ref()
            .ok_or_else(|| anyhow!("remote route selected but remote endpoint is not configured")),
    }
}

fn enforce_remote_egress_policy(
    route: LlmRoute,
    endpoint: &LlmEndpointConfig,
    config: &LlmConfig,
) -> Result<()> {
    if !matches!(route, LlmRoute::Remote) {
        return Ok(());
    }
    if !config.remote_egress_enabled {
        return Err(anyhow!(
            "llm.infer remote route blocked: LLM_REMOTE_EGRESS_ENABLED is not enabled"
        ));
    }
    if config.remote_host_allowlist.is_empty() {
        return Err(anyhow!(
            "llm.infer remote route blocked: LLM_REMOTE_HOST_ALLOWLIST is empty"
        ));
    }

    let url = Url::parse(&endpoint.base_url)
        .with_context(|| format!("invalid remote base URL `{}`", endpoint.base_url))?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("remote base URL missing host: {}", endpoint.base_url))?;

    if config
        .remote_host_allowlist
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(host))
    {
        Ok(())
    } else {
        Err(anyhow!(
            "llm.infer remote host `{}` is not allowlisted",
            host
        ))
    }
}

fn parse_action_args(args: &Value, max_prompt_bytes: usize) -> Result<LlmInferActionArgs> {
    let prompt = args
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("llm.infer args.prompt is required"))?
        .to_string();
    if prompt.len() > max_prompt_bytes {
        return Err(anyhow!(
            "llm.infer prompt exceeded {} bytes",
            max_prompt_bytes
        ));
    }

    let system = args
        .get("system")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let max_tokens = args
        .get("max_tokens")
        .and_then(Value::as_u64)
        .map(|value| value.min(u32::MAX as u64) as u32);
    let temperature = args
        .get("temperature")
        .and_then(Value::as_f64)
        .map(|value| value as f32);
    let prefer = args
        .get("prefer")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(parse_route_preference)
        .transpose()?;

    Ok(LlmInferActionArgs {
        prompt,
        system,
        prefer,
        max_tokens,
        temperature,
    })
}

fn parse_route_preference(raw: &str) -> Result<LlmRoute> {
    match raw.to_ascii_lowercase().as_str() {
        "local" => Ok(LlmRoute::Local),
        "remote" => Ok(LlmRoute::Remote),
        other => Err(anyhow!(
            "invalid llm.infer args.prefer `{}` (supported: local, remote)",
            other
        )),
    }
}

fn normalize_base_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn read_env_u64(key: &str, default: u64) -> Result<u64> {
    match env::var(key) {
        Ok(value) => value
            .parse::<u64>()
            .with_context(|| format!("invalid integer for {key}: {value}")),
        Err(_) => Ok(default),
    }
}

fn read_env_u64_optional(key: &str) -> Result<Option<u64>> {
    match env::var(key) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                trimmed
                    .parse::<u64>()
                    .map(Some)
                    .with_context(|| format!("invalid integer for {key}: {trimmed}"))
            }
        }
        Err(_) => Ok(None),
    }
}

fn read_env_f64(key: &str, default: f64) -> Result<f64> {
    match env::var(key) {
        Ok(value) => value
            .parse::<f64>()
            .with_context(|| format!("invalid float for {key}: {value}")),
        Err(_) => Ok(default),
    }
}

fn read_env_bool(key: &str, default: bool) -> bool {
    match env::var(key) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        ),
        Err(_) => default,
    }
}

fn read_env_csv(key: &str) -> Vec<String> {
    let Ok(raw) = env::var(key) else {
        return Vec::new();
    };
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn read_env_secret(value_key: &str, reference_key: &str) -> Result<Option<String>> {
    let resolver = EnvFileSecretResolver;
    resolve_secret_value(
        env::var(value_key).ok(),
        env::var(reference_key).ok(),
        &resolver,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        parse_route_preference, policy_scope_for_action, LlmConfig, LlmEndpointConfig, LlmMode,
    };
    use serde_json::json;
    use std::time::Duration;

    fn test_config(mode: LlmMode, with_local: bool, with_remote: bool) -> LlmConfig {
        LlmConfig {
            mode,
            timeout: Duration::from_secs(2),
            max_prompt_bytes: 32_000,
            max_output_bytes: 64_000,
            local: with_local.then(|| LlmEndpointConfig {
                base_url: "http://localhost:11434/v1".to_string(),
                model: "local-model".to_string(),
                api_key: None,
            }),
            remote: with_remote.then(|| LlmEndpointConfig {
                base_url: "https://api.remote/v1".to_string(),
                model: "remote-model".to_string(),
                api_key: Some("k".to_string()),
            }),
            remote_egress_enabled: false,
            remote_host_allowlist: Vec::new(),
            remote_token_budget_per_run: None,
            remote_cost_per_1k_tokens_usd: 0.0,
        }
    }

    #[test]
    fn local_first_defaults_to_local_scope() {
        let cfg = test_config(LlmMode::LocalFirst, true, true);
        let scope = policy_scope_for_action(&json!({"prompt":"hello"}), &cfg).expect("scope");
        assert_eq!(scope, "local:local-model");
    }

    #[test]
    fn local_first_honors_remote_preference() {
        let cfg = test_config(LlmMode::LocalFirst, true, true);
        let scope = policy_scope_for_action(&json!({"prompt":"hello","prefer":"remote"}), &cfg)
            .expect("scope");
        assert_eq!(scope, "remote:remote-model");
    }

    #[test]
    fn local_only_rejects_remote_preference() {
        let cfg = test_config(LlmMode::LocalOnly, true, true);
        let err = policy_scope_for_action(&json!({"prompt":"hello","prefer":"remote"}), &cfg)
            .expect_err("must reject");
        assert!(err.to_string().contains("local_only"));
    }

    #[test]
    fn parse_preference_rejects_invalid() {
        let err = parse_route_preference("edge").expect_err("invalid route");
        assert!(err.to_string().contains("supported"));
    }

    #[test]
    fn remote_route_blocked_when_egress_disabled() {
        let mut cfg = test_config(LlmMode::LocalFirst, true, true);
        cfg.remote_egress_enabled = false;
        cfg.remote_host_allowlist = vec!["api.remote".to_string()];
        let endpoint = cfg.remote.as_ref().expect("remote endpoint");

        let err = super::enforce_remote_egress_policy(super::LlmRoute::Remote, endpoint, &cfg)
            .expect_err("must block");
        assert!(err.to_string().contains("LLM_REMOTE_EGRESS_ENABLED"));
    }

    #[test]
    fn remote_route_blocked_when_host_not_allowlisted() {
        let mut cfg = test_config(LlmMode::LocalFirst, true, true);
        cfg.remote_egress_enabled = true;
        cfg.remote_host_allowlist = vec!["example.com".to_string()];
        let endpoint = cfg.remote.as_ref().expect("remote endpoint");

        let err = super::enforce_remote_egress_policy(super::LlmRoute::Remote, endpoint, &cfg)
            .expect_err("must block");
        assert!(err.to_string().contains("not allowlisted"));
    }

    #[test]
    fn remote_scope_can_still_be_resolved_for_policy_when_egress_disabled() {
        let cfg = test_config(LlmMode::LocalFirst, true, true);
        let scope = policy_scope_for_action(&json!({"prompt":"hello","prefer":"remote"}), &cfg)
            .expect("scope");
        assert_eq!(scope, "remote:remote-model");
    }
}
