use anyhow::{anyhow, Context, Result};
use core::{resolve_secret_value, CachedSecretResolver, CliSecretResolver};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{cmp::Ordering, collections::HashSet, env, sync::OnceLock, time::Duration};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmRequestClass {
    Interactive,
    Batch,
}

impl LlmRequestClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Batch => "batch",
        }
    }

    fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "interactive" => Ok(Self::Interactive),
            "batch" => Ok(Self::Batch),
            other => Err(anyhow!(
                "invalid llm request class `{}` (supported: interactive, batch)",
                other
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmLargeInputPolicy {
    Direct,
    SummarizeFirst,
    ChunkAndRetrieve,
    EscalateRemote,
}

impl LlmLargeInputPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::SummarizeFirst => "summarize_first",
            Self::ChunkAndRetrieve => "chunk_and_retrieve",
            Self::EscalateRemote => "escalate_remote",
        }
    }

    fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "summarize_first" => Ok(Self::SummarizeFirst),
            "direct" => Ok(Self::Direct),
            "chunk_and_retrieve" | "chunk_retrieve" => Ok(Self::ChunkAndRetrieve),
            "escalate_remote" | "remote" => Ok(Self::EscalateRemote),
            other => Err(anyhow!(
                "invalid LLM_LARGE_INPUT_POLICY `{}` (supported: direct, summarize_first, chunk_and_retrieve, escalate_remote)",
                other
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmRemoteEgressClass {
    NeverLeavesPrem,
    RedactedOnly,
    CloudAllowed,
}

impl LlmRemoteEgressClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NeverLeavesPrem => "never_leaves_prem",
            Self::RedactedOnly => "redacted_only",
            Self::CloudAllowed => "cloud_allowed",
        }
    }

    fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "cloud_allowed" => Ok(Self::CloudAllowed),
            "redacted_only" => Ok(Self::RedactedOnly),
            "never_leaves_prem" => Ok(Self::NeverLeavesPrem),
            other => Err(anyhow!(
                "invalid LLM_REMOTE_EGRESS_CLASS `{}` (supported: cloud_allowed, redacted_only, never_leaves_prem)",
                other
            )),
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
    pub max_input_bytes: usize,
    pub max_prompt_bytes: usize,
    pub max_output_bytes: usize,
    pub large_input_threshold_bytes: usize,
    pub large_input_policy: LlmLargeInputPolicy,
    pub large_input_summary_target_bytes: usize,
    pub context_retrieval_top_k: usize,
    pub context_retrieval_max_bytes: usize,
    pub context_retrieval_chunk_bytes: usize,
    pub local: Option<LlmEndpointConfig>,
    pub remote: Option<LlmEndpointConfig>,
    pub remote_egress_enabled: bool,
    pub remote_egress_class: LlmRemoteEgressClass,
    pub remote_host_allowlist: Vec<String>,
    pub remote_token_budget_per_run: Option<u64>,
    pub remote_token_budget_per_tenant: Option<u64>,
    pub remote_token_budget_per_agent: Option<u64>,
    pub remote_token_budget_per_model: Option<u64>,
    pub remote_token_budget_window_secs: u64,
    pub remote_token_budget_soft_alert_threshold_pct: Option<u8>,
    pub remote_cost_per_1k_tokens_usd: f64,
}

#[derive(Debug, Clone)]
struct LlmInferActionArgs {
    prompt: String,
    system: Option<String>,
    prefer: Option<LlmRoute>,
    request_class: LlmRequestClass,
    large_input_policy: Option<LlmLargeInputPolicy>,
    redacted: bool,
    context_documents: Vec<LlmContextDocument>,
    context_query: Option<String>,
    context_top_k: Option<usize>,
    context_max_bytes: Option<usize>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
}

#[derive(Debug, Clone)]
struct LlmContextDocument {
    id: String,
    text: String,
}

#[derive(Debug, Clone)]
struct LlmPromptPlan {
    prompt: String,
    prefer: Option<LlmRoute>,
    request_class: LlmRequestClass,
    large_input_policy: LlmLargeInputPolicy,
    large_input_applied: bool,
    large_input_reason_code: String,
    prompt_bytes_original: usize,
    prompt_bytes_effective: usize,
    retrieval_candidate_documents: usize,
    retrieval_selected_documents: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmGatewayDecision {
    pub version: String,
    pub mode: String,
    pub request_class: String,
    pub queue_lane: String,
    pub selected_route: String,
    pub reason_code: String,
    pub prefer: Option<String>,
    pub large_input_policy: String,
    pub large_input_applied: bool,
    pub large_input_reason_code: String,
    pub prompt_bytes_original: usize,
    pub prompt_bytes_effective: usize,
    pub retrieval_candidate_documents: usize,
    pub retrieval_selected_documents: usize,
    pub local_available: bool,
    pub remote_available: bool,
    pub remote_egress_class: String,
    pub remote_host: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmInferResult {
    pub route: String,
    pub model: String,
    pub gateway: LlmGatewayDecision,
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
        let max_input_bytes = read_env_u64("LLM_MAX_INPUT_BYTES", 262_144)? as usize;
        let max_prompt_bytes = read_env_u64("LLM_MAX_PROMPT_BYTES", 32_000)? as usize;
        let max_output_bytes = read_env_u64("LLM_MAX_OUTPUT_BYTES", 64_000)? as usize;
        if max_input_bytes < max_prompt_bytes {
            return Err(anyhow!(
                "LLM_MAX_INPUT_BYTES ({max_input_bytes}) must be >= LLM_MAX_PROMPT_BYTES ({max_prompt_bytes})"
            ));
        }
        let large_input_threshold_bytes =
            read_env_u64("LLM_LARGE_INPUT_THRESHOLD_BYTES", 12_000)? as usize;
        let large_input_policy = LlmLargeInputPolicy::parse(
            env::var("LLM_LARGE_INPUT_POLICY")
                .unwrap_or_else(|_| "summarize_first".to_string())
                .as_str(),
        )?;
        let large_input_summary_target_bytes = read_env_u64(
            "LLM_LARGE_INPUT_SUMMARY_TARGET_BYTES",
            (max_prompt_bytes / 2).max(1) as u64,
        )? as usize;
        let context_retrieval_top_k = read_env_u64("LLM_CONTEXT_RETRIEVAL_TOP_K", 6)? as usize;
        let context_retrieval_max_bytes =
            read_env_u64("LLM_CONTEXT_RETRIEVAL_MAX_BYTES", max_prompt_bytes as u64)? as usize;
        let context_retrieval_chunk_bytes =
            read_env_u64("LLM_CONTEXT_RETRIEVAL_CHUNK_BYTES", 2048)? as usize;

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
            max_input_bytes,
            max_prompt_bytes,
            max_output_bytes,
            large_input_threshold_bytes,
            large_input_policy,
            large_input_summary_target_bytes: large_input_summary_target_bytes.max(1),
            context_retrieval_top_k: context_retrieval_top_k.max(1),
            context_retrieval_max_bytes: context_retrieval_max_bytes.max(256),
            context_retrieval_chunk_bytes: context_retrieval_chunk_bytes.max(256),
            local,
            remote,
            remote_egress_enabled: read_env_bool("LLM_REMOTE_EGRESS_ENABLED", false),
            remote_egress_class: LlmRemoteEgressClass::parse(
                env::var("LLM_REMOTE_EGRESS_CLASS")
                    .unwrap_or_else(|_| "cloud_allowed".to_string())
                    .as_str(),
            )?,
            remote_host_allowlist: read_env_csv("LLM_REMOTE_HOST_ALLOWLIST"),
            remote_token_budget_per_run: read_env_u64_optional("LLM_REMOTE_TOKEN_BUDGET_PER_RUN")?,
            remote_token_budget_per_tenant: read_env_u64_optional(
                "LLM_REMOTE_TOKEN_BUDGET_PER_TENANT",
            )?,
            remote_token_budget_per_agent: read_env_u64_optional(
                "LLM_REMOTE_TOKEN_BUDGET_PER_AGENT",
            )?,
            remote_token_budget_per_model: read_env_u64_optional(
                "LLM_REMOTE_TOKEN_BUDGET_PER_MODEL",
            )?,
            remote_token_budget_window_secs: read_env_u64(
                "LLM_REMOTE_TOKEN_BUDGET_WINDOW_SECS",
                86_400,
            )?
            .max(1),
            remote_token_budget_soft_alert_threshold_pct: read_env_u8_optional(
                "LLM_REMOTE_TOKEN_BUDGET_SOFT_ALERT_THRESHOLD_PCT",
            )?,
            remote_cost_per_1k_tokens_usd: read_env_f64("LLM_REMOTE_COST_PER_1K_TOKENS_USD", 0.0)?,
        })
    }
}

pub fn policy_scope_for_action(args: &Value, config: &LlmConfig) -> Result<String> {
    let parsed = parse_action_args(args, config.max_input_bytes)?;
    let prompt_plan = build_prompt_plan(&parsed, config)?;
    let route = select_route_with_reason(config, prompt_plan.prefer)?.route;
    let endpoint = endpoint_for_route(config, route)?;
    Ok(format!("{}:{}", route.as_str(), endpoint.model))
}

pub async fn execute_llm_infer(args: &Value, config: &LlmConfig) -> Result<LlmInferResult> {
    let parsed = parse_action_args(args, config.max_input_bytes)?;
    let prompt_plan = build_prompt_plan(&parsed, config)?;
    let route_decision = select_route_with_reason(config, prompt_plan.prefer)?;
    let route = route_decision.route;
    let endpoint = endpoint_for_route(config, route)?;
    let remote_host = enforce_remote_egress_policy(route, endpoint, config, &parsed)?;
    let gateway = build_gateway_decision(config, route_decision, remote_host, &prompt_plan);

    let mut messages = Vec::with_capacity(2);
    if let Some(system) = parsed.system.clone() {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: system,
        });
    }
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: prompt_plan.prompt,
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
        gateway,
        response_text: text,
        prompt_tokens: payload.usage.as_ref().and_then(|u| u.prompt_tokens),
        completion_tokens: payload.usage.as_ref().and_then(|u| u.completion_tokens),
        total_tokens: payload.usage.and_then(|u| u.total_tokens),
    })
}

#[derive(Debug, Clone, Copy)]
struct RouteDecision {
    route: LlmRoute,
    reason_code: &'static str,
}

fn select_route_with_reason(config: &LlmConfig, prefer: Option<LlmRoute>) -> Result<RouteDecision> {
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
            Ok(RouteDecision {
                route: LlmRoute::Local,
                reason_code: "mode_local_only",
            })
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
            Ok(RouteDecision {
                route: LlmRoute::Remote,
                reason_code: "mode_remote_only",
            })
        }
        LlmMode::LocalFirst => {
            if matches!(prefer, Some(LlmRoute::Remote)) && remote_available {
                return Ok(RouteDecision {
                    route: LlmRoute::Remote,
                    reason_code: "prefer_remote_local_first",
                });
            }
            if local_available {
                return Ok(RouteDecision {
                    route: LlmRoute::Local,
                    reason_code: "local_first_default_local",
                });
            }
            if remote_available {
                return Ok(RouteDecision {
                    route: LlmRoute::Remote,
                    reason_code: "local_unavailable_remote_fallback",
                });
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
    args: &LlmInferActionArgs,
) -> Result<Option<String>> {
    if !matches!(route, LlmRoute::Remote) {
        return Ok(None);
    }
    if matches!(
        config.remote_egress_class,
        LlmRemoteEgressClass::NeverLeavesPrem
    ) {
        return Err(anyhow!(
            "llm.infer remote route blocked: LLM_REMOTE_EGRESS_CLASS=never_leaves_prem"
        ));
    }
    if matches!(
        config.remote_egress_class,
        LlmRemoteEgressClass::RedactedOnly
    ) && !args.redacted
    {
        return Err(anyhow!(
            "llm.infer remote route blocked: LLM_REMOTE_EGRESS_CLASS=redacted_only requires args.redacted=true"
        ));
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
        Ok(Some(host.to_string()))
    } else {
        Err(anyhow!(
            "llm.infer remote host `{}` is not allowlisted",
            host
        ))
    }
}

fn build_gateway_decision(
    config: &LlmConfig,
    route_decision: RouteDecision,
    remote_host: Option<String>,
    prompt_plan: &LlmPromptPlan,
) -> LlmGatewayDecision {
    LlmGatewayDecision {
        version: "m14d.v1".to_string(),
        mode: config.mode.as_str().to_string(),
        request_class: prompt_plan.request_class.as_str().to_string(),
        queue_lane: prompt_plan.request_class.as_str().to_string(),
        selected_route: route_decision.route.as_str().to_string(),
        reason_code: route_decision.reason_code.to_string(),
        prefer: prompt_plan.prefer.map(|route| route.as_str().to_string()),
        large_input_policy: prompt_plan.large_input_policy.as_str().to_string(),
        large_input_applied: prompt_plan.large_input_applied,
        large_input_reason_code: prompt_plan.large_input_reason_code.clone(),
        prompt_bytes_original: prompt_plan.prompt_bytes_original,
        prompt_bytes_effective: prompt_plan.prompt_bytes_effective,
        retrieval_candidate_documents: prompt_plan.retrieval_candidate_documents,
        retrieval_selected_documents: prompt_plan.retrieval_selected_documents,
        local_available: config.local.is_some(),
        remote_available: config.remote.is_some(),
        remote_egress_class: config.remote_egress_class.as_str().to_string(),
        remote_host,
    }
}

fn build_prompt_plan(parsed: &LlmInferActionArgs, config: &LlmConfig) -> Result<LlmPromptPlan> {
    let prompt_bytes_original = parsed.prompt.len();
    let mut prompt = parsed.prompt.clone();
    let mut prefer = parsed.prefer;
    let large_input_policy = parsed
        .large_input_policy
        .unwrap_or(config.large_input_policy);
    let mut large_input_applied = false;
    let mut large_input_reason_code = "direct".to_string();
    let mut retrieval_candidate_documents = 0usize;
    let mut retrieval_selected_documents = 0usize;

    let is_large_input = prompt_bytes_original > config.large_input_threshold_bytes;
    if is_large_input {
        match large_input_policy {
            LlmLargeInputPolicy::Direct => {
                large_input_reason_code = "large_input_direct".to_string();
            }
            LlmLargeInputPolicy::SummarizeFirst => {
                prompt =
                    summarize_text_for_budget(&prompt, config.large_input_summary_target_bytes);
                large_input_applied = true;
                large_input_reason_code = "large_input_summarize_first".to_string();
            }
            LlmLargeInputPolicy::ChunkAndRetrieve => {
                let retrieval = build_retrieved_prompt(parsed, config)?;
                prompt = retrieval.prompt;
                retrieval_candidate_documents = retrieval.candidate_documents;
                retrieval_selected_documents = retrieval.selected_documents;
                large_input_applied = true;
                large_input_reason_code = retrieval.reason_code;
            }
            LlmLargeInputPolicy::EscalateRemote => {
                prefer = Some(LlmRoute::Remote);
                large_input_applied = true;
                large_input_reason_code = "large_input_escalate_remote".to_string();
            }
        }
    } else if !parsed.context_documents.is_empty() {
        let retrieval = build_retrieved_prompt(parsed, config)?;
        prompt = retrieval.prompt;
        retrieval_candidate_documents = retrieval.candidate_documents;
        retrieval_selected_documents = retrieval.selected_documents;
        large_input_applied = true;
        large_input_reason_code = "context_retrieval_applied".to_string();
    }

    if prompt.len() > config.max_prompt_bytes {
        prompt = summarize_text_for_budget(&prompt, config.max_prompt_bytes);
        large_input_applied = true;
        if large_input_reason_code == "direct" || large_input_reason_code == "large_input_direct" {
            large_input_reason_code = "prompt_trimmed_to_budget".to_string();
        }
    }
    if prompt.trim().is_empty() {
        return Err(anyhow!(
            "llm.infer prompt became empty after large-input policy processing"
        ));
    }

    Ok(LlmPromptPlan {
        prompt_bytes_effective: prompt.len(),
        prompt,
        prefer,
        request_class: parsed.request_class,
        large_input_policy,
        large_input_applied,
        large_input_reason_code,
        prompt_bytes_original,
        retrieval_candidate_documents,
        retrieval_selected_documents,
    })
}

#[derive(Debug, Clone)]
struct RetrievalOutput {
    prompt: String,
    candidate_documents: usize,
    selected_documents: usize,
    reason_code: String,
}

fn build_retrieved_prompt(
    parsed: &LlmInferActionArgs,
    config: &LlmConfig,
) -> Result<RetrievalOutput> {
    let context_top_k = parsed
        .context_top_k
        .unwrap_or(config.context_retrieval_top_k)
        .clamp(1, 32);
    let context_max_bytes = parsed
        .context_max_bytes
        .unwrap_or(config.context_retrieval_max_bytes)
        .max(256);
    let context_query = parsed
        .context_query
        .as_deref()
        .unwrap_or(parsed.prompt.as_str());

    if parsed.context_documents.is_empty() {
        let chunks = split_text_chunks(&parsed.prompt, config.context_retrieval_chunk_bytes);
        let candidate_documents = chunks.len();
        let selected =
            rank_and_select_documents(context_query, &chunks, context_top_k, context_max_bytes);
        let prompt = build_retrieval_prompt(
            &truncate_text_for_budget(context_query, 1024),
            selected.as_slice(),
            context_max_bytes,
        );
        return Ok(RetrievalOutput {
            prompt,
            candidate_documents,
            selected_documents: selected.len(),
            reason_code: "large_input_chunk_retrieve_prompt".to_string(),
        });
    }

    let candidate_documents = parsed.context_documents.len();
    let selected = rank_and_select_documents(
        context_query,
        parsed.context_documents.as_slice(),
        context_top_k,
        context_max_bytes,
    );
    let prompt = build_retrieval_prompt(
        &truncate_text_for_budget(context_query, 1024),
        selected.as_slice(),
        context_max_bytes,
    );
    Ok(RetrievalOutput {
        prompt,
        candidate_documents,
        selected_documents: selected.len(),
        reason_code: "large_input_chunk_retrieve_documents".to_string(),
    })
}

fn split_text_chunks(text: &str, chunk_bytes: usize) -> Vec<LlmContextDocument> {
    if text.is_empty() {
        return Vec::new();
    }
    let chunk_bytes = chunk_bytes.max(256);
    let mut output = Vec::new();
    let mut start = 0usize;
    while start < text.len() {
        let candidate_end = (start + chunk_bytes).min(text.len());
        let end = clamp_utf8_boundary(text, candidate_end);
        if end <= start {
            break;
        }
        output.push(LlmContextDocument {
            id: format!("chunk-{}", output.len() + 1),
            text: text[start..end].to_string(),
        });
        start = end;
    }
    output
}

fn rank_and_select_documents(
    query: &str,
    documents: &[LlmContextDocument],
    top_k: usize,
    max_bytes: usize,
) -> Vec<LlmContextDocument> {
    let query_tokens = tokenize(query);
    let mut scored = documents
        .iter()
        .enumerate()
        .map(|(index, doc)| {
            let score = token_overlap_score(&query_tokens, doc.text.as_str());
            (index, score, doc)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
            .then_with(|| {
                left.2
                    .text
                    .len()
                    .cmp(&right.2.text.len())
                    .then(Ordering::Equal)
            })
    });

    let mut selected = Vec::new();
    let mut used_bytes = 0usize;
    for (_, _, doc) in scored.into_iter().take(top_k) {
        if used_bytes >= max_bytes {
            break;
        }
        let remaining = max_bytes.saturating_sub(used_bytes);
        let excerpt = truncate_text_for_budget(doc.text.as_str(), remaining.max(64));
        if excerpt.is_empty() {
            continue;
        }
        used_bytes = used_bytes.saturating_add(excerpt.len());
        selected.push(LlmContextDocument {
            id: doc.id.clone(),
            text: excerpt,
        });
    }

    if selected.is_empty() && !documents.is_empty() {
        let first = &documents[0];
        selected.push(LlmContextDocument {
            id: first.id.clone(),
            text: truncate_text_for_budget(first.text.as_str(), max_bytes.max(64)),
        });
    }
    selected
}

fn build_retrieval_prompt(
    query: &str,
    selected: &[LlmContextDocument],
    max_bytes: usize,
) -> String {
    let mut output = String::new();
    output.push_str("Query:\n");
    output.push_str(query);
    output.push_str("\n\nRelevant context:\n");
    for doc in selected {
        output.push_str("---- ");
        output.push_str(doc.id.as_str());
        output.push_str(" ----\n");
        output.push_str(doc.text.as_str());
        output.push('\n');
    }
    truncate_text_for_budget(output.as_str(), max_bytes.max(256))
}

fn summarize_text_for_budget(text: &str, budget_bytes: usize) -> String {
    if text.len() <= budget_bytes {
        return text.to_string();
    }
    let budget_bytes = budget_bytes.max(64);
    let head_budget = ((budget_bytes as f64) * 0.65) as usize;
    let tail_budget = ((budget_bytes as f64) * 0.25) as usize;
    let head = truncate_text_for_budget(text, head_budget.max(16));
    let tail = truncate_text_from_end_for_budget(text, tail_budget.max(16));
    let mut output = String::new();
    output.push_str("[summary-trimmed]\n");
    output.push_str(head.as_str());
    output.push_str("\n...\n");
    output.push_str(tail.as_str());
    truncate_text_for_budget(output.as_str(), budget_bytes)
}

fn clamp_utf8_boundary(value: &str, mut idx: usize) -> usize {
    idx = idx.min(value.len());
    while idx > 0 && !value.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn truncate_text_for_budget(value: &str, budget_bytes: usize) -> String {
    if budget_bytes == 0 || value.is_empty() {
        return String::new();
    }
    if value.len() <= budget_bytes {
        return value.to_string();
    }
    let end = clamp_utf8_boundary(value, budget_bytes);
    value[..end].to_string()
}

fn truncate_text_from_end_for_budget(value: &str, budget_bytes: usize) -> String {
    if budget_bytes == 0 || value.is_empty() {
        return String::new();
    }
    if value.len() <= budget_bytes {
        return value.to_string();
    }
    let start = clamp_utf8_boundary(value, value.len().saturating_sub(budget_bytes));
    value[start..].to_string()
}

fn tokenize(value: &str) -> HashSet<String> {
    value
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(str::trim)
        .filter(|token| token.len() > 2)
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn token_overlap_score(query_tokens: &HashSet<String>, text: &str) -> usize {
    if query_tokens.is_empty() {
        return 0;
    }
    let doc_tokens = tokenize(text);
    query_tokens.intersection(&doc_tokens).count()
}

fn parse_action_args(args: &Value, max_input_bytes: usize) -> Result<LlmInferActionArgs> {
    let prompt = args
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("llm.infer args.prompt is required"))?
        .to_string();
    if prompt.len() > max_input_bytes {
        return Err(anyhow!(
            "llm.infer prompt exceeded {} bytes",
            max_input_bytes
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
    let request_class = args
        .get("request_class")
        .or_else(|| args.get("queue_class"))
        .and_then(Value::as_str)
        .map(LlmRequestClass::parse)
        .transpose()?
        .unwrap_or(LlmRequestClass::Interactive);
    let large_input_policy = args
        .get("large_input_policy")
        .and_then(Value::as_str)
        .map(LlmLargeInputPolicy::parse)
        .transpose()?;
    let redacted = args
        .get("redacted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let context_documents = parse_context_documents(args.get("context_documents"));
    let context_query = args
        .get("context_query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let context_top_k = args
        .get("context_top_k")
        .and_then(Value::as_u64)
        .map(|value| value.clamp(1, 32) as usize);
    let context_max_bytes = args
        .get("context_max_bytes")
        .and_then(Value::as_u64)
        .map(|value| value.clamp(256, 512 * 1024) as usize);

    Ok(LlmInferActionArgs {
        prompt,
        system,
        prefer,
        request_class,
        large_input_policy,
        redacted,
        context_documents,
        context_query,
        context_top_k,
        context_max_bytes,
        max_tokens,
        temperature,
    })
}

fn parse_context_documents(value: Option<&Value>) -> Vec<LlmContextDocument> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        return Vec::new();
    };

    items
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            let text = item
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|content| !content.is_empty())?
                .to_string();
            let id = item
                .get("id")
                .or_else(|| item.get("path"))
                .or_else(|| item.get("source"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("doc-{}", idx + 1));
            Some(LlmContextDocument { id, text })
        })
        .collect()
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

fn read_env_u8_optional(key: &str) -> Result<Option<u8>> {
    match env::var(key) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                let parsed = trimmed
                    .parse::<u8>()
                    .with_context(|| format!("invalid integer for {key}: {value}"))?;
                if parsed == 0 || parsed > 100 {
                    return Err(anyhow!(
                        "{key} must be between 1 and 100 when set (got {parsed})"
                    ));
                }
                Ok(Some(parsed))
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
    let resolver = shared_secret_resolver();
    resolve_secret_value(
        env::var(value_key).ok(),
        env::var(reference_key).ok(),
        resolver,
    )
}

fn shared_secret_resolver() -> &'static CachedSecretResolver<CliSecretResolver> {
    static RESOLVER: OnceLock<CachedSecretResolver<CliSecretResolver>> = OnceLock::new();
    RESOLVER.get_or_init(|| CachedSecretResolver::from_env_with(CliSecretResolver::from_env()))
}

#[cfg(test)]
mod tests {
    use super::{
        build_prompt_plan, parse_action_args, parse_route_preference, policy_scope_for_action,
        LlmConfig, LlmEndpointConfig, LlmLargeInputPolicy, LlmMode, LlmRemoteEgressClass,
        LlmRequestClass,
    };
    use serde_json::json;
    use std::time::Duration;

    fn test_config(mode: LlmMode, with_local: bool, with_remote: bool) -> LlmConfig {
        LlmConfig {
            mode,
            timeout: Duration::from_secs(2),
            max_input_bytes: 262_144,
            max_prompt_bytes: 32_000,
            max_output_bytes: 64_000,
            large_input_threshold_bytes: 12_000,
            large_input_policy: LlmLargeInputPolicy::SummarizeFirst,
            large_input_summary_target_bytes: 8_000,
            context_retrieval_top_k: 6,
            context_retrieval_max_bytes: 32_000,
            context_retrieval_chunk_bytes: 2_048,
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
            remote_egress_class: LlmRemoteEgressClass::CloudAllowed,
            remote_host_allowlist: Vec::new(),
            remote_token_budget_per_run: None,
            remote_token_budget_per_tenant: None,
            remote_token_budget_per_agent: None,
            remote_token_budget_per_model: None,
            remote_token_budget_window_secs: 86_400,
            remote_token_budget_soft_alert_threshold_pct: None,
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
        let args = parse_action_args(
            &json!({"prompt":"hello","prefer":"remote"}),
            cfg.max_input_bytes,
        )
        .expect("parse args");

        let err =
            super::enforce_remote_egress_policy(super::LlmRoute::Remote, endpoint, &cfg, &args)
                .expect_err("must block");
        assert!(err.to_string().contains("LLM_REMOTE_EGRESS_ENABLED"));
    }

    #[test]
    fn remote_route_blocked_when_host_not_allowlisted() {
        let mut cfg = test_config(LlmMode::LocalFirst, true, true);
        cfg.remote_egress_enabled = true;
        cfg.remote_host_allowlist = vec!["example.com".to_string()];
        let endpoint = cfg.remote.as_ref().expect("remote endpoint");
        let args = parse_action_args(
            &json!({"prompt":"hello","prefer":"remote"}),
            cfg.max_input_bytes,
        )
        .expect("parse args");

        let err =
            super::enforce_remote_egress_policy(super::LlmRoute::Remote, endpoint, &cfg, &args)
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

    #[test]
    fn remote_route_blocked_when_egress_class_is_never_leaves_prem() {
        let mut cfg = test_config(LlmMode::RemoteOnly, false, true);
        cfg.remote_egress_enabled = true;
        cfg.remote_egress_class = LlmRemoteEgressClass::NeverLeavesPrem;
        cfg.remote_host_allowlist = vec!["api.remote".to_string()];
        let endpoint = cfg.remote.as_ref().expect("remote endpoint");
        let args =
            parse_action_args(&json!({"prompt":"hello"}), cfg.max_input_bytes).expect("args");

        let err =
            super::enforce_remote_egress_policy(super::LlmRoute::Remote, endpoint, &cfg, &args)
                .expect_err("must block");
        assert!(err.to_string().contains("never_leaves_prem"));
    }

    #[test]
    fn remote_route_blocked_for_redacted_only_when_redacted_flag_missing() {
        let mut cfg = test_config(LlmMode::RemoteOnly, false, true);
        cfg.remote_egress_enabled = true;
        cfg.remote_egress_class = LlmRemoteEgressClass::RedactedOnly;
        cfg.remote_host_allowlist = vec!["api.remote".to_string()];
        let endpoint = cfg.remote.as_ref().expect("remote endpoint");
        let args =
            parse_action_args(&json!({"prompt":"hello"}), cfg.max_input_bytes).expect("args");

        let err =
            super::enforce_remote_egress_policy(super::LlmRoute::Remote, endpoint, &cfg, &args)
                .expect_err("must block");
        assert!(err.to_string().contains("args.redacted=true"));
    }

    #[test]
    fn remote_route_allows_redacted_only_when_redacted_flag_is_set() {
        let mut cfg = test_config(LlmMode::RemoteOnly, false, true);
        cfg.remote_egress_enabled = true;
        cfg.remote_egress_class = LlmRemoteEgressClass::RedactedOnly;
        cfg.remote_host_allowlist = vec!["api.remote".to_string()];
        let endpoint = cfg.remote.as_ref().expect("remote endpoint");
        let args = parse_action_args(
            &json!({"prompt":"hello","redacted":true}),
            cfg.max_input_bytes,
        )
        .expect("args");

        let host =
            super::enforce_remote_egress_policy(super::LlmRoute::Remote, endpoint, &cfg, &args)
                .expect("must allow");
        assert_eq!(host.as_deref(), Some("api.remote"));
    }

    #[test]
    fn large_input_policy_escalate_remote_changes_policy_scope() {
        let mut cfg = test_config(LlmMode::LocalFirst, true, true);
        cfg.large_input_threshold_bytes = 8;
        cfg.large_input_policy = LlmLargeInputPolicy::EscalateRemote;
        let scope = policy_scope_for_action(
            &json!({
                "prompt":"this is definitely larger than eight bytes"
            }),
            &cfg,
        )
        .expect("scope");
        assert_eq!(scope, "remote:remote-model");
    }

    #[test]
    fn parse_action_args_reads_request_class_and_context_docs() {
        let args = parse_action_args(
            &json!({
                "prompt":"summarize",
                "request_class":"batch",
                "context_documents":[
                    {"id":"src/main.rs","text":"fn main() { println!(\"hi\"); }"},
                    {"path":"README.md","text":"SecureAgnt docs"}
                ],
                "context_query":"main function"
            }),
            1024,
        )
        .expect("parsed args");
        assert_eq!(args.request_class, LlmRequestClass::Batch);
        assert_eq!(args.context_documents.len(), 2);
        assert_eq!(args.context_documents[0].id, "src/main.rs");
        assert_eq!(args.context_documents[1].id, "README.md");
    }

    #[test]
    fn build_prompt_plan_applies_chunk_retrieval_for_large_input() {
        let mut cfg = test_config(LlmMode::LocalFirst, true, true);
        cfg.large_input_threshold_bytes = 32;
        cfg.large_input_policy = LlmLargeInputPolicy::ChunkAndRetrieve;
        let parsed = parse_action_args(
            &json!({
                "prompt":"Need the function that starts the app",
                "context_documents":[
                    {"id":"src/lib.rs","text":"pub fn start_app() { /* boot */ }"},
                    {"id":"src/auth.rs","text":"pub fn issue_token() {}"},
                    {"id":"src/db.rs","text":"pub fn query_runs() {}"}
                ],
                "context_query":"start app function",
                "request_class":"interactive"
            }),
            8192,
        )
        .expect("args");
        let plan = build_prompt_plan(&parsed, &cfg).expect("plan");
        assert!(plan.large_input_applied);
        assert!(plan.prompt.contains("Relevant context"));
        assert_eq!(plan.retrieval_candidate_documents, 3);
        assert!(plan.retrieval_selected_documents >= 1);
    }
}
