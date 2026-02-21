use anyhow::{anyhow, Context, Result};
use core::{
    get_llm_gateway_cache_entry, prune_llm_gateway_cache_namespace,
    release_llm_gateway_admission_lease, resolve_secret_value,
    try_acquire_llm_gateway_admission_lease, upsert_llm_gateway_cache_entry, CachedSecretResolver,
    CliSecretResolver, LlmGatewayAdmissionLeaseAcquireParams, LlmGatewayAdmissionLeaseRecord,
    NewLlmGatewayCacheEntry,
};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{
    cmp::Ordering as CmpOrdering,
    collections::{HashMap, HashSet},
    env,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use uuid::Uuid;

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
    pub admission_enabled: bool,
    pub admission_interactive_max_inflight: usize,
    pub admission_batch_max_inflight: usize,
    pub cache_enabled: bool,
    pub cache_ttl_secs: u64,
    pub cache_max_entries: usize,
    pub distributed_enabled: bool,
    pub distributed_fail_open: bool,
    pub distributed_owner: String,
    pub distributed_admission_enabled: bool,
    pub distributed_admission_lease_ms: u64,
    pub distributed_cache_enabled: bool,
    pub distributed_cache_namespace_max_entries: usize,
    pub verifier_enabled: bool,
    pub verifier_min_score_pct: u8,
    pub verifier_escalate_remote: bool,
    pub verifier_min_response_chars: usize,
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
    verifier_required: bool,
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

#[derive(Debug, Clone)]
struct LlmExecutionOutcome {
    route: LlmRoute,
    model: String,
    response_text: String,
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    total_tokens: Option<u32>,
    admission_status: String,
    cache_status: String,
    cache_key_sha256: Option<String>,
    verifier_enabled: bool,
    verifier_score_pct: Option<u8>,
    verifier_threshold_pct: Option<u8>,
    verifier_escalated: bool,
    verifier_reason_code: Option<String>,
    route_reason_code: String,
    remote_host: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedCompletion {
    response_text: String,
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    total_tokens: Option<u32>,
}

#[derive(Debug)]
struct CacheEntry {
    value: CachedCompletion,
    inserted_at: Instant,
}

#[derive(Debug, Default)]
struct LlmResponseCache {
    entries: HashMap<String, CacheEntry>,
}

#[derive(Debug, Default)]
struct LlmAdmissionCounters {
    interactive_inflight: AtomicUsize,
    batch_inflight: AtomicUsize,
}

#[derive(Debug)]
struct LlmAdmissionGuard {
    release: AdmissionRelease,
}

#[derive(Debug)]
enum AdmissionRelease {
    None,
    Local {
        request_class: LlmRequestClass,
    },
    Distributed {
        pool: PgPool,
        lease: LlmGatewayAdmissionLeaseRecord,
    },
}

#[derive(Debug)]
struct LlmAdmissionAcquireOutcome {
    guard: LlmAdmissionGuard,
    status: String,
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
    pub admission_status: String,
    pub cache_status: String,
    pub cache_key_sha256: Option<String>,
    pub verifier_enabled: bool,
    pub verifier_score_pct: Option<u8>,
    pub verifier_threshold_pct: Option<u8>,
    pub verifier_escalated: bool,
    pub verifier_reason_code: Option<String>,
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

#[derive(Debug, Clone, Serialize)]
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

impl Drop for LlmAdmissionGuard {
    fn drop(&mut self) {
        match std::mem::replace(&mut self.release, AdmissionRelease::None) {
            AdmissionRelease::None => {}
            AdmissionRelease::Local { request_class } => {
                let counters = admission_counters();
                match request_class {
                    LlmRequestClass::Interactive => {
                        counters.interactive_inflight.fetch_sub(1, Ordering::SeqCst);
                    }
                    LlmRequestClass::Batch => {
                        counters.batch_inflight.fetch_sub(1, Ordering::SeqCst);
                    }
                }
            }
            AdmissionRelease::Distributed { pool, lease } => {
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    handle.spawn(async move {
                        let _ = release_llm_gateway_admission_lease(&pool, &lease).await;
                    });
                }
            }
        }
    }
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
        let admission_interactive_max_inflight =
            read_env_u64("LLM_ADMISSION_INTERACTIVE_MAX_INFLIGHT", 8)? as usize;
        let admission_batch_max_inflight =
            read_env_u64("LLM_ADMISSION_BATCH_MAX_INFLIGHT", 2)? as usize;
        let distributed_enabled = read_env_bool("LLM_DISTRIBUTED_ENABLED", false);
        let distributed_owner = env::var("LLM_DISTRIBUTED_OWNER")
            .ok()
            .and_then(|value| non_empty_trimmed(value.as_str()).map(ToString::to_string))
            .unwrap_or_else(|| {
                let host = env::var("HOSTNAME")
                    .ok()
                    .and_then(|value| non_empty_trimmed(value.as_str()).map(ToString::to_string))
                    .unwrap_or_else(|| "worker".to_string());
                format!("{host}-{}", std::process::id())
            });
        let verifier_min_score_pct =
            read_env_u64("LLM_VERIFIER_MIN_SCORE_PCT", 65)?.clamp(1, 100) as u8;
        let verifier_min_response_chars =
            read_env_u64("LLM_VERIFIER_MIN_RESPONSE_CHARS", 48)? as usize;

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
            admission_enabled: read_env_bool("LLM_ADMISSION_ENABLED", true),
            admission_interactive_max_inflight: admission_interactive_max_inflight.max(1),
            admission_batch_max_inflight: admission_batch_max_inflight.max(1),
            cache_enabled: read_env_bool("LLM_CACHE_ENABLED", false),
            cache_ttl_secs: read_env_u64("LLM_CACHE_TTL_SECS", 300)?,
            cache_max_entries: read_env_u64("LLM_CACHE_MAX_ENTRIES", 1024)? as usize,
            distributed_enabled,
            distributed_fail_open: read_env_bool("LLM_DISTRIBUTED_FAIL_OPEN", true),
            distributed_owner,
            distributed_admission_enabled: read_env_bool(
                "LLM_DISTRIBUTED_ADMISSION_ENABLED",
                distributed_enabled,
            ),
            distributed_admission_lease_ms: read_env_u64(
                "LLM_DISTRIBUTED_ADMISSION_LEASE_MS",
                30_000,
            )?
            .max(250),
            distributed_cache_enabled: read_env_bool(
                "LLM_DISTRIBUTED_CACHE_ENABLED",
                distributed_enabled,
            ),
            distributed_cache_namespace_max_entries: read_env_u64(
                "LLM_DISTRIBUTED_CACHE_NAMESPACE_MAX_ENTRIES",
                4096,
            )? as usize,
            verifier_enabled: read_env_bool("LLM_VERIFIER_ENABLED", false),
            verifier_min_score_pct,
            verifier_escalate_remote: read_env_bool("LLM_VERIFIER_ESCALATE_REMOTE", true),
            verifier_min_response_chars: verifier_min_response_chars.max(8),
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

pub async fn execute_llm_infer(
    args: &Value,
    config: &LlmConfig,
    cache_namespace: Option<&str>,
    db_pool: Option<&PgPool>,
) -> Result<LlmInferResult> {
    let parsed = parse_action_args(args, config.max_input_bytes)?;
    let admission = acquire_admission(&parsed, config, cache_namespace, db_pool).await?;
    let prompt_plan = build_prompt_plan(&parsed, config)?;
    let outcome = run_llm_with_controls(
        &parsed,
        &prompt_plan,
        config,
        cache_namespace,
        db_pool,
        admission.status,
    )
    .await?;
    drop(admission.guard);
    let gateway = build_gateway_decision(config, &outcome, &prompt_plan);

    Ok(LlmInferResult {
        route: outcome.route.as_str().to_string(),
        model: outcome.model,
        gateway,
        response_text: outcome.response_text,
        prompt_tokens: outcome.prompt_tokens,
        completion_tokens: outcome.completion_tokens,
        total_tokens: outcome.total_tokens,
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

async fn run_llm_with_controls(
    parsed: &LlmInferActionArgs,
    prompt_plan: &LlmPromptPlan,
    config: &LlmConfig,
    cache_namespace: Option<&str>,
    db_pool: Option<&PgPool>,
    admission_status: String,
) -> Result<LlmExecutionOutcome> {
    let route_decision = select_route_with_reason(config, prompt_plan.prefer)?;
    let mut route = route_decision.route;
    let mut route_reason_code = route_decision.reason_code.to_string();
    let mut verifier_score_pct = None;
    let mut verifier_escalated = false;
    let mut verifier_reason_code = None;

    let (mut completion, mut cache_status, mut cache_key_sha256, mut remote_host) =
        execute_route_completion(route, parsed, prompt_plan, config, cache_namespace, db_pool)
            .await?;

    let verifier_enabled = config.verifier_enabled || parsed.verifier_required;
    if verifier_enabled {
        let score = score_response(
            prompt_plan.prompt.as_str(),
            completion.response_text.as_str(),
            config.verifier_min_response_chars,
        );
        verifier_score_pct = Some(score);
        if route == LlmRoute::Local && score < config.verifier_min_score_pct {
            if config.verifier_escalate_remote && config.remote.is_some() {
                match execute_route_completion(
                    LlmRoute::Remote,
                    parsed,
                    prompt_plan,
                    config,
                    cache_namespace,
                    db_pool,
                )
                .await
                {
                    Ok((
                        remote_completion,
                        remote_cache_status,
                        remote_cache_key,
                        remote_host_v,
                    )) => {
                        completion = remote_completion;
                        cache_status = remote_cache_status;
                        cache_key_sha256 = remote_cache_key;
                        remote_host = remote_host_v;
                        route = LlmRoute::Remote;
                        route_reason_code = "verifier_escalated_remote_low_score".to_string();
                        verifier_escalated = true;
                        verifier_reason_code = Some("low_score_remote_escalated".to_string());
                    }
                    Err(_) => {
                        verifier_reason_code =
                            Some("low_score_remote_escalation_failed".to_string());
                    }
                }
            } else {
                verifier_reason_code = Some("low_score_no_remote_escalation".to_string());
            }
        }
    }

    Ok(LlmExecutionOutcome {
        route,
        model: endpoint_for_route(config, route)?.model.clone(),
        response_text: completion.response_text,
        prompt_tokens: completion.prompt_tokens,
        completion_tokens: completion.completion_tokens,
        total_tokens: completion.total_tokens,
        admission_status,
        cache_status,
        cache_key_sha256,
        verifier_enabled,
        verifier_score_pct,
        verifier_threshold_pct: verifier_enabled.then_some(config.verifier_min_score_pct),
        verifier_escalated,
        verifier_reason_code,
        route_reason_code,
        remote_host,
    })
}

async fn execute_route_completion(
    route: LlmRoute,
    parsed: &LlmInferActionArgs,
    prompt_plan: &LlmPromptPlan,
    config: &LlmConfig,
    cache_namespace: Option<&str>,
    db_pool: Option<&PgPool>,
) -> Result<(CachedCompletion, String, Option<String>, Option<String>)> {
    let endpoint = endpoint_for_route(config, route)?;
    let remote_host = enforce_remote_egress_policy(route, endpoint, config, parsed)?;
    let messages = build_messages(parsed.system.clone(), prompt_plan.prompt.clone());
    let cache_namespace = cache_namespace.unwrap_or("global");
    let mut distributed_local_fallback = false;

    if config.cache_enabled
        && config.cache_ttl_secs > 0
        && config.distributed_enabled
        && config.distributed_cache_enabled
    {
        if let Some(pool) = db_pool {
            let cache_key = compute_cache_key(
                cache_namespace,
                route,
                endpoint.model.as_str(),
                &messages,
                parsed.max_tokens,
                parsed.temperature,
            );
            match get_llm_gateway_cache_entry(pool, cache_namespace, cache_key.as_str()).await {
                Ok(Some(hit)) => {
                    let parsed: CachedCompletion = serde_json::from_value(hit.response_json)
                        .with_context(|| {
                            "invalid llm gateway cache payload (expected CachedCompletion)"
                        })?;
                    return Ok((
                        parsed,
                        "distributed_hit".to_string(),
                        Some(cache_key),
                        remote_host,
                    ));
                }
                Ok(None) => {
                    let completion =
                        request_chat_completion(endpoint, &messages, parsed, config, route).await?;
                    if let Ok(response_json) = serde_json::to_value(&completion) {
                        let _ = upsert_llm_gateway_cache_entry(
                            pool,
                            &NewLlmGatewayCacheEntry {
                                cache_key_sha256: cache_key.clone(),
                                namespace: cache_namespace.to_string(),
                                route: route.as_str().to_string(),
                                model: endpoint.model.clone(),
                                response_json,
                                ttl: Duration::from_secs(config.cache_ttl_secs),
                            },
                        )
                        .await;
                    }
                    let _ = prune_llm_gateway_cache_namespace(
                        pool,
                        cache_namespace,
                        config.distributed_cache_namespace_max_entries as i64,
                    )
                    .await;
                    return Ok((
                        completion,
                        "distributed_miss".to_string(),
                        Some(cache_key),
                        remote_host,
                    ));
                }
                Err(err) => {
                    if !config.distributed_fail_open {
                        return Err(err).with_context(|| {
                            "llm.infer distributed cache lookup failed and fail-open is disabled"
                        });
                    }
                    distributed_local_fallback = true;
                }
            }
        } else if !config.distributed_fail_open {
            return Err(anyhow!(
                "llm.infer distributed cache is enabled but no database pool was provided"
            ));
        } else {
            distributed_local_fallback = true;
        }
    }

    if config.cache_enabled && config.cache_ttl_secs > 0 {
        let cache_key = compute_cache_key(
            cache_namespace,
            route,
            endpoint.model.as_str(),
            &messages,
            parsed.max_tokens,
            parsed.temperature,
        );
        if let Some(hit) = cache_lookup(
            cache_key.as_str(),
            Duration::from_secs(config.cache_ttl_secs),
        ) {
            let status = if distributed_local_fallback {
                "distributed_fail_open_local_hit"
            } else {
                "hit"
            };
            return Ok((hit, status.to_string(), Some(cache_key), remote_host));
        }
        let completion =
            request_chat_completion(endpoint, &messages, parsed, config, route).await?;
        cache_insert(
            cache_key.clone(),
            completion.clone(),
            config.cache_max_entries.max(1),
        );
        let status = if distributed_local_fallback {
            "distributed_fail_open_local_miss"
        } else {
            "miss"
        };
        return Ok((completion, status.to_string(), Some(cache_key), remote_host));
    }

    let completion = request_chat_completion(endpoint, &messages, parsed, config, route).await?;
    let cache_status = if config.cache_enabled {
        if distributed_local_fallback {
            "distributed_fail_open_local_bypass_ttl0"
        } else {
            "bypass_ttl0"
        }
    } else {
        "disabled"
    };
    Ok((completion, cache_status.to_string(), None, remote_host))
}

fn build_messages(system: Option<String>, prompt: String) -> Vec<ChatMessage> {
    let mut messages = Vec::with_capacity(2);
    if let Some(system) = system {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: system,
        });
    }
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: prompt,
    });
    messages
}

async fn request_chat_completion(
    endpoint: &LlmEndpointConfig,
    messages: &[ChatMessage],
    parsed: &LlmInferActionArgs,
    config: &LlmConfig,
    route: LlmRoute,
) -> Result<CachedCompletion> {
    let request = ChatCompletionRequest {
        model: endpoint.model.clone(),
        messages: messages.to_vec(),
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

    Ok(CachedCompletion {
        response_text: text,
        prompt_tokens: payload.usage.as_ref().and_then(|u| u.prompt_tokens),
        completion_tokens: payload.usage.as_ref().and_then(|u| u.completion_tokens),
        total_tokens: payload.usage.and_then(|u| u.total_tokens),
    })
}

async fn acquire_admission(
    parsed: &LlmInferActionArgs,
    config: &LlmConfig,
    cache_namespace: Option<&str>,
    db_pool: Option<&PgPool>,
) -> Result<LlmAdmissionAcquireOutcome> {
    if !config.admission_enabled {
        return Ok(LlmAdmissionAcquireOutcome {
            guard: LlmAdmissionGuard {
                release: AdmissionRelease::None,
            },
            status: "disabled".to_string(),
        });
    }

    let distributed_enabled = config.distributed_enabled && config.distributed_admission_enabled;
    if distributed_enabled {
        if let Some(pool) = db_pool {
            let max_inflight = match parsed.request_class {
                LlmRequestClass::Interactive => config.admission_interactive_max_inflight.max(1),
                LlmRequestClass::Batch => config.admission_batch_max_inflight.max(1),
            };
            let namespace = cache_namespace.unwrap_or("global").to_string();
            let lane = parsed.request_class.as_str().to_string();
            let lease = try_acquire_llm_gateway_admission_lease(
                pool,
                &LlmGatewayAdmissionLeaseAcquireParams {
                    namespace,
                    lane,
                    max_inflight: max_inflight as i32,
                    lease_id: Uuid::new_v4(),
                    lease_owner: config.distributed_owner.clone(),
                    lease_for: Duration::from_millis(config.distributed_admission_lease_ms),
                },
            )
            .await;
            match lease {
                Ok(Some(lease)) => {
                    return Ok(LlmAdmissionAcquireOutcome {
                        guard: LlmAdmissionGuard {
                            release: AdmissionRelease::Distributed {
                                pool: pool.clone(),
                                lease,
                            },
                        },
                        status: "distributed_admitted".to_string(),
                    });
                }
                Ok(None) => {
                    return Err(anyhow!(
                        "llm.infer admission denied: request_class={} saturated (max_inflight={})",
                        parsed.request_class.as_str(),
                        max_inflight
                    ));
                }
                Err(err) => {
                    if !config.distributed_fail_open {
                        return Err(err)
                            .with_context(|| "llm.infer distributed admission backend failed");
                    }
                }
            }
        } else if !config.distributed_fail_open {
            return Err(anyhow!(
                "llm.infer distributed admission is enabled but no database pool was provided"
            ));
        }
    }

    let guard = acquire_local_admission(parsed, config)?;
    let status = if distributed_enabled {
        "distributed_fail_open_local"
    } else {
        "admitted"
    };
    Ok(LlmAdmissionAcquireOutcome {
        guard,
        status: status.to_string(),
    })
}

fn acquire_local_admission(
    parsed: &LlmInferActionArgs,
    config: &LlmConfig,
) -> Result<LlmAdmissionGuard> {
    let counters = admission_counters();
    let (counter, max_inflight) = match parsed.request_class {
        LlmRequestClass::Interactive => (
            &counters.interactive_inflight,
            config.admission_interactive_max_inflight.max(1),
        ),
        LlmRequestClass::Batch => (
            &counters.batch_inflight,
            config.admission_batch_max_inflight.max(1),
        ),
    };
    if try_reserve_slot(counter, max_inflight) {
        Ok(LlmAdmissionGuard {
            release: AdmissionRelease::Local {
                request_class: parsed.request_class,
            },
        })
    } else {
        Err(anyhow!(
            "llm.infer admission denied: request_class={} saturated (max_inflight={})",
            parsed.request_class.as_str(),
            max_inflight
        ))
    }
}

fn try_reserve_slot(counter: &AtomicUsize, max_inflight: usize) -> bool {
    loop {
        let current = counter.load(Ordering::SeqCst);
        if current >= max_inflight {
            return false;
        }
        if counter
            .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return true;
        }
    }
}

fn score_response(prompt: &str, response: &str, min_chars: usize) -> u8 {
    let mut score: i32 = 100;
    let response_trimmed = response.trim().to_ascii_lowercase();
    if response_trimmed.len() < min_chars {
        score -= 35;
    }
    for phrase in [
        "i'm not sure",
        "i am not sure",
        "i do not know",
        "i don't know",
        "cannot assist",
    ] {
        if response_trimmed.contains(phrase) {
            score -= 20;
            break;
        }
    }
    let prompt_lc = prompt.to_ascii_lowercase();
    if prompt_lc.contains("json") {
        let starts_json = response_trimmed.starts_with('{') || response_trimmed.starts_with('[');
        if !starts_json {
            score -= 25;
        }
    }
    if prompt.len() > 512 && response_trimmed.len() < min_chars.saturating_mul(2) {
        score -= 15;
    }
    score.clamp(0, 100) as u8
}

fn compute_cache_key(
    namespace: &str,
    route: LlmRoute,
    model: &str,
    messages: &[ChatMessage],
    max_tokens: Option<u32>,
    temperature: Option<f32>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(namespace.as_bytes());
    hasher.update(b"|");
    hasher.update(route.as_str().as_bytes());
    hasher.update(b"|");
    hasher.update(model.as_bytes());
    hasher.update(b"|");
    for msg in messages {
        hasher.update(msg.role.as_bytes());
        hasher.update(b":");
        hasher.update(msg.content.as_bytes());
        hasher.update(b"|");
    }
    hasher.update(max_tokens.unwrap_or(0).to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(
        temperature
            .map(|value| format!("{value:.4}"))
            .unwrap_or_default()
            .as_bytes(),
    );
    format!("{:x}", hasher.finalize())
}

fn cache_lookup(cache_key: &str, ttl: Duration) -> Option<CachedCompletion> {
    let mut cache = response_cache().lock().expect("llm cache lock poisoned");
    let entry = cache.entries.get(cache_key)?;
    if entry.inserted_at.elapsed() > ttl {
        cache.entries.remove(cache_key);
        return None;
    }
    Some(entry.value.clone())
}

fn cache_insert(cache_key: String, value: CachedCompletion, max_entries: usize) {
    let mut cache = response_cache().lock().expect("llm cache lock poisoned");
    cache.entries.insert(
        cache_key,
        CacheEntry {
            value,
            inserted_at: Instant::now(),
        },
    );
    if cache.entries.len() <= max_entries {
        return;
    }
    if let Some(oldest_key) = cache
        .entries
        .iter()
        .min_by_key(|(_, entry)| entry.inserted_at)
        .map(|(key, _)| key.clone())
    {
        cache.entries.remove(oldest_key.as_str());
    }
}

fn response_cache() -> &'static Mutex<LlmResponseCache> {
    static CACHE: OnceLock<Mutex<LlmResponseCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(LlmResponseCache::default()))
}

fn admission_counters() -> &'static LlmAdmissionCounters {
    static COUNTERS: OnceLock<LlmAdmissionCounters> = OnceLock::new();
    COUNTERS.get_or_init(LlmAdmissionCounters::default)
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
    outcome: &LlmExecutionOutcome,
    prompt_plan: &LlmPromptPlan,
) -> LlmGatewayDecision {
    LlmGatewayDecision {
        version: "m14f.v1".to_string(),
        mode: config.mode.as_str().to_string(),
        request_class: prompt_plan.request_class.as_str().to_string(),
        queue_lane: prompt_plan.request_class.as_str().to_string(),
        selected_route: outcome.route.as_str().to_string(),
        reason_code: outcome.route_reason_code.clone(),
        prefer: prompt_plan.prefer.map(|route| route.as_str().to_string()),
        large_input_policy: prompt_plan.large_input_policy.as_str().to_string(),
        large_input_applied: prompt_plan.large_input_applied,
        large_input_reason_code: prompt_plan.large_input_reason_code.clone(),
        prompt_bytes_original: prompt_plan.prompt_bytes_original,
        prompt_bytes_effective: prompt_plan.prompt_bytes_effective,
        retrieval_candidate_documents: prompt_plan.retrieval_candidate_documents,
        retrieval_selected_documents: prompt_plan.retrieval_selected_documents,
        admission_status: outcome.admission_status.clone(),
        cache_status: outcome.cache_status.clone(),
        cache_key_sha256: outcome.cache_key_sha256.clone(),
        verifier_enabled: outcome.verifier_enabled,
        verifier_score_pct: outcome.verifier_score_pct,
        verifier_threshold_pct: outcome.verifier_threshold_pct,
        verifier_escalated: outcome.verifier_escalated,
        verifier_reason_code: outcome.verifier_reason_code.clone(),
        local_available: config.local.is_some(),
        remote_available: config.remote.is_some(),
        remote_egress_class: config.remote_egress_class.as_str().to_string(),
        remote_host: outcome.remote_host.clone(),
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
                    .then(CmpOrdering::Equal)
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
    let verifier_required = args
        .get("verify")
        .or_else(|| args.get("verifier_required"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

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
        verifier_required,
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
        acquire_admission, build_prompt_plan, cache_insert, cache_lookup, compute_cache_key,
        parse_action_args, parse_route_preference, policy_scope_for_action, score_response,
        CachedCompletion, ChatMessage, LlmConfig, LlmEndpointConfig, LlmLargeInputPolicy, LlmMode,
        LlmRemoteEgressClass, LlmRequestClass, LlmRoute,
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
            admission_enabled: true,
            admission_interactive_max_inflight: 8,
            admission_batch_max_inflight: 2,
            cache_enabled: false,
            cache_ttl_secs: 300,
            cache_max_entries: 1024,
            distributed_enabled: false,
            distributed_fail_open: true,
            distributed_owner: "test-worker".to_string(),
            distributed_admission_enabled: false,
            distributed_admission_lease_ms: 30_000,
            distributed_cache_enabled: false,
            distributed_cache_namespace_max_entries: 4096,
            verifier_enabled: false,
            verifier_min_score_pct: 65,
            verifier_escalate_remote: true,
            verifier_min_response_chars: 48,
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

    #[test]
    fn admission_denies_when_batch_lane_is_saturated() {
        let mut cfg = test_config(LlmMode::LocalFirst, true, true);
        cfg.admission_enabled = true;
        cfg.admission_batch_max_inflight = 1;

        let parsed = parse_action_args(
            &json!({"prompt":"batch work","request_class":"batch"}),
            cfg.max_input_bytes,
        )
        .expect("parsed args");
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let first = rt
            .block_on(acquire_admission(&parsed, &cfg, None, None))
            .expect("first admission");
        let second = rt
            .block_on(acquire_admission(&parsed, &cfg, None, None))
            .expect_err("must deny second");
        assert!(second.to_string().contains("admission denied"));
        drop(first.guard);

        let third = rt
            .block_on(acquire_admission(&parsed, &cfg, None, None))
            .expect("slot released");
        drop(third.guard);
    }

    #[test]
    fn cache_roundtrip_hits_before_ttl_expiry() {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
        }];
        let key = compute_cache_key(
            "tenant:test",
            LlmRoute::Local,
            "local-model",
            &messages,
            None,
            None,
        );
        cache_insert(
            key.clone(),
            CachedCompletion {
                response_text: "cached".to_string(),
                prompt_tokens: Some(1),
                completion_tokens: Some(1),
                total_tokens: Some(2),
            },
            1024,
        );
        let hit = cache_lookup(&key, Duration::from_secs(60)).expect("cache hit");
        assert_eq!(hit.response_text, "cached");
    }

    #[test]
    fn verifier_score_penalizes_uncertain_short_response() {
        let score = score_response("Return JSON with the answer", "I don't know.", 48);
        assert!(score < 65);
    }
}
