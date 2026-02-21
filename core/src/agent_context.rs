use chrono_tz::Tz;
use cron::Schedule;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
    str::FromStr,
};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

const DEFAULT_REQUIRED_FILES: [&str; 7] = [
    "AGENTS.md",
    "TOOLS.md",
    "IDENTITY.md",
    "SOUL.md",
    "USER.md",
    "MEMORY.md",
    "HEARTBEAT.md",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentContextMutability {
    Immutable,
    HumanPrimary,
    AgentManaged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeartbeatIntentKind {
    Interval,
    Cron,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatTriggerCandidate {
    pub kind: HeartbeatIntentKind,
    pub recipe_id: String,
    pub interval_seconds: Option<i64>,
    pub cron_expression: Option<String>,
    pub timezone: Option<String>,
    pub max_inflight_runs: i32,
    pub jitter_seconds: i32,
    pub line: usize,
    pub source_line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatCompileIssue {
    pub line: usize,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatCompileReport {
    pub candidates: Vec<HeartbeatTriggerCandidate>,
    pub issues: Vec<HeartbeatCompileIssue>,
}

#[derive(Debug, Clone)]
pub struct AgentContextLoaderConfig {
    pub root_dir: PathBuf,
    pub required_files: Vec<String>,
    pub max_file_bytes: usize,
    pub max_total_bytes: usize,
    pub max_dynamic_files_per_dir: usize,
}

impl AgentContextLoaderConfig {
    pub fn with_defaults(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
            required_files: default_required_files(),
            max_file_bytes: 64 * 1024,
            max_total_bytes: 256 * 1024,
            max_dynamic_files_per_dir: 8,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentContextFile {
    pub slot: String,
    pub relative_path: String,
    pub sha256: String,
    pub bytes: usize,
    pub content: String,
}

impl AgentContextFile {
    fn metadata_json(&self) -> Value {
        json!({
            "slot": self.slot,
            "relative_path": self.relative_path,
            "sha256": self.sha256,
            "bytes": self.bytes,
        })
    }

    fn skill_json(&self) -> Value {
        json!({
            "slot": self.slot,
            "relative_path": self.relative_path,
            "sha256": self.sha256,
            "bytes": self.bytes,
            "content": self.content,
        })
    }
}

#[derive(Debug, Clone)]
pub struct AgentContextSnapshot {
    pub source_dir: PathBuf,
    pub loaded_at: OffsetDateTime,
    pub required_files: Vec<AgentContextFile>,
    pub memory_files: Vec<AgentContextFile>,
    pub session_files: Vec<AgentContextFile>,
    pub missing_required_files: Vec<String>,
    pub warnings: Vec<String>,
}

impl AgentContextSnapshot {
    pub fn loaded_file_count(&self) -> usize {
        self.required_files.len() + self.memory_files.len() + self.session_files.len()
    }

    pub fn total_loaded_bytes(&self) -> usize {
        self.required_files
            .iter()
            .chain(self.memory_files.iter())
            .chain(self.session_files.iter())
            .map(|item| item.bytes)
            .sum()
    }

    pub fn aggregate_sha256(&self) -> String {
        let mut entries = self
            .required_files
            .iter()
            .chain(self.memory_files.iter())
            .chain(self.session_files.iter())
            .map(|item| {
                format!(
                    "{}:{}:{}:{}",
                    item.slot, item.relative_path, item.sha256, item.bytes
                )
            })
            .collect::<Vec<_>>();
        entries.sort();
        let digest_input = entries.join("\n");
        format!("{:x}", Sha256::digest(digest_input.as_bytes()))
    }

    pub fn summary_json(&self) -> Value {
        json!({
            "source_dir": self.source_dir,
            "loaded_at": self.loaded_at,
            "required_file_count": self.required_files.len(),
            "memory_file_count": self.memory_files.len(),
            "session_file_count": self.session_files.len(),
            "loaded_file_count": self.loaded_file_count(),
            "total_loaded_bytes": self.total_loaded_bytes(),
            "missing_required_files": self.missing_required_files,
            "warnings": self.warnings,
            "aggregate_sha256": self.aggregate_sha256(),
            "required_files": self.required_files.iter().map(AgentContextFile::metadata_json).collect::<Vec<_>>(),
            "memory_files": self.memory_files.iter().map(AgentContextFile::metadata_json).collect::<Vec<_>>(),
            "session_files": self.session_files.iter().map(AgentContextFile::metadata_json).collect::<Vec<_>>(),
        })
    }

    pub fn summary_digest_sha256(&self) -> Result<String, serde_json::Error> {
        let canonical_bytes = serde_json::to_vec(&self.summary_json())?;
        Ok(format!("{:x}", Sha256::digest(canonical_bytes)))
    }

    pub fn skill_context_json(&self) -> Value {
        json!({
            "schema_version": "v1",
            "source_dir": self.source_dir,
            "loaded_at": self.loaded_at,
            "aggregate_sha256": self.aggregate_sha256(),
            "missing_required_files": self.missing_required_files,
            "warnings": self.warnings,
            "required_files": self.required_files.iter().map(AgentContextFile::skill_json).collect::<Vec<_>>(),
            "memory_files": self.memory_files.iter().map(AgentContextFile::skill_json).collect::<Vec<_>>(),
            "session_files": self.session_files.iter().map(AgentContextFile::skill_json).collect::<Vec<_>>(),
        })
    }

    pub fn required_file_content(&self, relative_path: &str) -> Option<&str> {
        self.required_files
            .iter()
            .find(|file| file.relative_path.eq_ignore_ascii_case(relative_path))
            .map(|file| file.content.as_str())
    }
}

#[derive(Debug, Error)]
pub enum AgentContextLoadError {
    #[error("agent context config is invalid: {message}")]
    InvalidConfig { message: String },
    #[error("agent context not found in any search path")]
    NotFound { searched_paths: Vec<PathBuf> },
    #[error("agent context IO error at `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub fn default_required_files() -> Vec<String> {
    DEFAULT_REQUIRED_FILES
        .iter()
        .map(|entry| entry.to_string())
        .collect()
}

pub fn load_agent_context_snapshot(
    config: &AgentContextLoaderConfig,
    tenant_id: &str,
    agent_id: Uuid,
) -> Result<AgentContextSnapshot, AgentContextLoadError> {
    validate_loader_config(config)?;
    let source_dir = resolve_source_dir(config, tenant_id, agent_id)?;

    let mut warnings = Vec::new();
    let mut missing_required_files = Vec::new();
    let mut loaded_bytes = 0usize;

    let mut required_files = Vec::new();
    for required in &config.required_files {
        let required_path = safe_join_relative(&source_dir, required)?;
        let Some(file) = load_context_file(
            &source_dir,
            required_path.as_path(),
            required.clone(),
            config,
            &mut loaded_bytes,
            &mut warnings,
        )?
        else {
            missing_required_files.push(required.clone());
            continue;
        };
        required_files.push(file);
    }

    let memory_files = load_dynamic_context_files(
        &source_dir,
        "memory",
        "md",
        "memory",
        config,
        &mut loaded_bytes,
        &mut warnings,
    )?;
    let session_files = load_dynamic_context_files(
        &source_dir,
        "sessions",
        "jsonl",
        "session",
        config,
        &mut loaded_bytes,
        &mut warnings,
    )?;

    Ok(AgentContextSnapshot {
        source_dir,
        loaded_at: OffsetDateTime::now_utc(),
        required_files,
        memory_files,
        session_files,
        missing_required_files,
        warnings,
    })
}

fn validate_loader_config(config: &AgentContextLoaderConfig) -> Result<(), AgentContextLoadError> {
    if config.max_file_bytes == 0 {
        return Err(AgentContextLoadError::InvalidConfig {
            message: "max_file_bytes must be > 0".to_string(),
        });
    }
    if config.max_total_bytes == 0 {
        return Err(AgentContextLoadError::InvalidConfig {
            message: "max_total_bytes must be > 0".to_string(),
        });
    }
    if config.max_dynamic_files_per_dir == 0 {
        return Err(AgentContextLoadError::InvalidConfig {
            message: "max_dynamic_files_per_dir must be > 0".to_string(),
        });
    }
    for required in &config.required_files {
        validate_relative_path(required)?;
    }
    Ok(())
}

fn resolve_source_dir(
    config: &AgentContextLoaderConfig,
    tenant_id: &str,
    agent_id: Uuid,
) -> Result<PathBuf, AgentContextLoadError> {
    let tenant_agent = config
        .root_dir
        .join(tenant_id.trim())
        .join(agent_id.to_string());
    let flat_agent = config.root_dir.join(agent_id.to_string());
    let searched_paths = vec![tenant_agent.clone(), flat_agent.clone()];

    if tenant_agent.is_dir() {
        return Ok(tenant_agent);
    }
    if flat_agent.is_dir() {
        return Ok(flat_agent);
    }
    Err(AgentContextLoadError::NotFound { searched_paths })
}

fn validate_relative_path(path: &str) -> Result<(), AgentContextLoadError> {
    if path.trim().is_empty() {
        return Err(AgentContextLoadError::InvalidConfig {
            message: "required file entry cannot be empty".to_string(),
        });
    }
    let rel = Path::new(path);
    if rel.is_absolute() {
        return Err(AgentContextLoadError::InvalidConfig {
            message: format!("required file `{path}` cannot be absolute"),
        });
    }
    for component in rel.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(AgentContextLoadError::InvalidConfig {
                message: format!("required file `{path}` contains invalid path components"),
            });
        }
    }
    Ok(())
}

fn safe_join_relative(base: &Path, rel: &str) -> Result<PathBuf, AgentContextLoadError> {
    validate_relative_path(rel)?;
    Ok(base.join(rel))
}

fn load_dynamic_context_files(
    source_dir: &Path,
    subdir: &str,
    extension: &str,
    slot_prefix: &str,
    config: &AgentContextLoaderConfig,
    loaded_bytes: &mut usize,
    warnings: &mut Vec<String>,
) -> Result<Vec<AgentContextFile>, AgentContextLoadError> {
    let dynamic_dir = source_dir.join(subdir);
    if !dynamic_dir.is_dir() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(&dynamic_dir).map_err(|source| AgentContextLoadError::Io {
        path: dynamic_dir.clone(),
        source,
    })?;
    let mut files = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
        })
        .collect::<Vec<_>>();
    files.sort();
    files.reverse();

    let mut loaded = Vec::new();
    for path in files.into_iter().take(config.max_dynamic_files_per_dir) {
        let rel = path
            .strip_prefix(source_dir)
            .unwrap_or(path.as_path())
            .to_string_lossy()
            .to_string();
        let slot = format!("{slot_prefix}:{rel}");
        if let Some(file) = load_context_file(
            source_dir,
            path.as_path(),
            slot,
            config,
            loaded_bytes,
            warnings,
        )? {
            loaded.push(file);
        }
    }
    Ok(loaded)
}

fn load_context_file(
    source_dir: &Path,
    full_path: &Path,
    slot: String,
    config: &AgentContextLoaderConfig,
    loaded_bytes: &mut usize,
    warnings: &mut Vec<String>,
) -> Result<Option<AgentContextFile>, AgentContextLoadError> {
    if !full_path.exists() {
        return Ok(None);
    }
    if !full_path.is_file() {
        warnings.push(format!(
            "context path `{}` is not a regular file; skipped",
            full_path.display()
        ));
        return Ok(None);
    }

    let metadata = fs::metadata(full_path).map_err(|source| AgentContextLoadError::Io {
        path: full_path.to_path_buf(),
        source,
    })?;
    let file_len = metadata.len() as usize;
    if file_len > config.max_file_bytes {
        warnings.push(format!(
            "context file `{}` exceeds max_file_bytes {}; skipped",
            full_path.display(),
            config.max_file_bytes
        ));
        return Ok(None);
    }
    if loaded_bytes.saturating_add(file_len) > config.max_total_bytes {
        warnings.push(format!(
            "loading `{}` would exceed max_total_bytes {}; skipped",
            full_path.display(),
            config.max_total_bytes
        ));
        return Ok(None);
    }

    let bytes = fs::read(full_path).map_err(|source| AgentContextLoadError::Io {
        path: full_path.to_path_buf(),
        source,
    })?;
    let content = match String::from_utf8(bytes.clone()) {
        Ok(value) => value,
        Err(_) => {
            warnings.push(format!(
                "context file `{}` is not valid UTF-8; skipped",
                full_path.display()
            ));
            return Ok(None);
        }
    };
    *loaded_bytes = loaded_bytes.saturating_add(bytes.len());
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let relative_path = full_path
        .strip_prefix(source_dir)
        .unwrap_or(full_path)
        .to_string_lossy()
        .to_string();

    Ok(Some(AgentContextFile {
        slot,
        relative_path,
        sha256,
        bytes: bytes.len(),
        content,
    }))
}

pub fn normalize_required_files(values: &[String]) -> Vec<String> {
    let mut dedup = BTreeSet::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        dedup.insert(trimmed.to_string());
    }
    dedup.into_iter().collect()
}

pub fn classify_mutability(relative_path: &str) -> Option<AgentContextMutability> {
    let normalized = normalize_context_relative_path(relative_path)?;
    match normalized.as_str() {
        "AGENTS.md" | "TOOLS.md" | "IDENTITY.md" | "SOUL.md" => {
            Some(AgentContextMutability::Immutable)
        }
        "USER.md" | "HEARTBEAT.md" | "BOOTSTRAP.md" => Some(AgentContextMutability::HumanPrimary),
        "MEMORY.md" => Some(AgentContextMutability::AgentManaged),
        _ if normalized.starts_with("memory/") && normalized.ends_with(".md") => {
            Some(AgentContextMutability::AgentManaged)
        }
        _ if normalized.starts_with("sessions/") && normalized.ends_with(".jsonl") => {
            Some(AgentContextMutability::AgentManaged)
        }
        _ => None,
    }
}

pub fn compile_heartbeat_markdown(markdown: &str) -> HeartbeatCompileReport {
    const MAX_CANDIDATES: usize = 128;
    let mut candidates = Vec::new();
    let mut issues = Vec::new();

    for (idx, raw_line) in markdown.lines().enumerate() {
        let line_number = idx + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let normalized = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .unwrap_or(trimmed);
        if normalized.is_empty() {
            continue;
        }
        if candidates.len() >= MAX_CANDIDATES {
            issues.push(HeartbeatCompileIssue {
                line: line_number,
                code: "candidate_limit_exceeded".to_string(),
                message: format!(
                    "maximum heartbeat candidate limit ({MAX_CANDIDATES}) exceeded; remaining lines skipped"
                ),
            });
            break;
        }

        let tokens = tokenize_heartbeat_line(normalized);
        if tokens.is_empty() {
            continue;
        }
        let directive = tokens[0].to_ascii_lowercase();
        let parsed = match directive.as_str() {
            "every" | "interval" => {
                parse_interval_heartbeat_line(tokens.as_slice(), line_number, normalized)
            }
            "cron" => parse_cron_heartbeat_line(tokens.as_slice(), line_number, normalized),
            _ => Err(HeartbeatCompileIssue {
                line: line_number,
                code: "unsupported_directive".to_string(),
                message: format!("unsupported heartbeat directive `{directive}`"),
            }),
        };

        match parsed {
            Ok(candidate) => candidates.push(candidate),
            Err(issue) => issues.push(issue),
        }
    }

    HeartbeatCompileReport { candidates, issues }
}

fn parse_interval_heartbeat_line(
    tokens: &[String],
    line: usize,
    source_line: &str,
) -> Result<HeartbeatTriggerCandidate, HeartbeatCompileIssue> {
    if tokens.len() < 3 {
        return Err(HeartbeatCompileIssue {
            line,
            code: "invalid_interval_syntax".to_string(),
            message:
                "interval syntax: every <seconds> recipe=<recipe_id> [max_inflight=<n>] [jitter=<sec>]"
                    .to_string(),
        });
    }

    let seconds = tokens[1]
        .parse::<i64>()
        .map_err(|_| HeartbeatCompileIssue {
            line,
            code: "invalid_interval_seconds".to_string(),
            message: "interval seconds must be a positive integer".to_string(),
        })?;
    if !(60..=31_536_000).contains(&seconds) {
        return Err(HeartbeatCompileIssue {
            line,
            code: "interval_out_of_bounds".to_string(),
            message: "interval seconds must be between 60 and 31536000".to_string(),
        });
    }

    let opts = parse_heartbeat_options(&tokens[2..], line)?;
    let recipe_id = opts.recipe_id.ok_or_else(|| HeartbeatCompileIssue {
        line,
        code: "missing_recipe".to_string(),
        message: "heartbeat directive requires recipe=<recipe_id>".to_string(),
    })?;

    Ok(HeartbeatTriggerCandidate {
        kind: HeartbeatIntentKind::Interval,
        recipe_id,
        interval_seconds: Some(seconds),
        cron_expression: None,
        timezone: None,
        max_inflight_runs: opts.max_inflight_runs,
        jitter_seconds: opts.jitter_seconds,
        line,
        source_line: source_line.to_string(),
    })
}

fn parse_cron_heartbeat_line(
    tokens: &[String],
    line: usize,
    source_line: &str,
) -> Result<HeartbeatTriggerCandidate, HeartbeatCompileIssue> {
    if tokens.len() < 3 {
        return Err(HeartbeatCompileIssue {
            line,
            code: "invalid_cron_syntax".to_string(),
            message:
                "cron syntax: cron \"<expr>\" recipe=<recipe_id> [timezone=<iana>] [max_inflight=<n>] [jitter=<sec>]".to_string(),
        });
    }
    let cron_expression = tokens[1].trim().to_string();
    if cron_expression.is_empty() {
        return Err(HeartbeatCompileIssue {
            line,
            code: "invalid_cron_expression".to_string(),
            message: "cron expression must not be empty".to_string(),
        });
    }
    Schedule::from_str(cron_expression.as_str()).map_err(|err| HeartbeatCompileIssue {
        line,
        code: "invalid_cron_expression".to_string(),
        message: format!("cron parse error: {err}"),
    })?;

    let opts = parse_heartbeat_options(&tokens[2..], line)?;
    let recipe_id = opts.recipe_id.ok_or_else(|| HeartbeatCompileIssue {
        line,
        code: "missing_recipe".to_string(),
        message: "heartbeat directive requires recipe=<recipe_id>".to_string(),
    })?;
    let timezone = opts.timezone.unwrap_or_else(|| "UTC".to_string());
    Tz::from_str(timezone.as_str()).map_err(|_| HeartbeatCompileIssue {
        line,
        code: "invalid_timezone".to_string(),
        message: format!("timezone `{timezone}` is not a valid IANA timezone"),
    })?;

    Ok(HeartbeatTriggerCandidate {
        kind: HeartbeatIntentKind::Cron,
        recipe_id,
        interval_seconds: None,
        cron_expression: Some(cron_expression),
        timezone: Some(timezone),
        max_inflight_runs: opts.max_inflight_runs,
        jitter_seconds: opts.jitter_seconds,
        line,
        source_line: source_line.to_string(),
    })
}

#[derive(Debug, Default)]
struct HeartbeatOptions {
    recipe_id: Option<String>,
    timezone: Option<String>,
    max_inflight_runs: i32,
    jitter_seconds: i32,
}

fn parse_heartbeat_options(
    tokens: &[String],
    line: usize,
) -> Result<HeartbeatOptions, HeartbeatCompileIssue> {
    let mut opts = HeartbeatOptions {
        max_inflight_runs: 1,
        jitter_seconds: 0,
        ..HeartbeatOptions::default()
    };
    for token in tokens {
        let Some((raw_key, raw_value)) = token.split_once('=') else {
            return Err(HeartbeatCompileIssue {
                line,
                code: "invalid_option".to_string(),
                message: format!("option `{token}` must use key=value format"),
            });
        };
        let key = raw_key.trim().to_ascii_lowercase();
        let value = raw_value.trim();
        if value.is_empty() {
            return Err(HeartbeatCompileIssue {
                line,
                code: "invalid_option".to_string(),
                message: format!("option `{token}` has an empty value"),
            });
        }
        match key.as_str() {
            "recipe" | "recipe_id" => {
                opts.recipe_id = Some(value.to_string());
            }
            "timezone" | "tz" => {
                opts.timezone = Some(value.to_string());
            }
            "max_inflight" | "max_inflight_runs" => {
                let parsed = value.parse::<i32>().map_err(|_| HeartbeatCompileIssue {
                    line,
                    code: "invalid_max_inflight".to_string(),
                    message: format!("max_inflight must be an integer, got `{value}`"),
                })?;
                if !(1..=1000).contains(&parsed) {
                    return Err(HeartbeatCompileIssue {
                        line,
                        code: "max_inflight_out_of_bounds".to_string(),
                        message: "max_inflight must be between 1 and 1000".to_string(),
                    });
                }
                opts.max_inflight_runs = parsed;
            }
            "jitter" | "jitter_seconds" => {
                let parsed = value.parse::<i32>().map_err(|_| HeartbeatCompileIssue {
                    line,
                    code: "invalid_jitter".to_string(),
                    message: format!("jitter must be an integer, got `{value}`"),
                })?;
                if !(0..=3600).contains(&parsed) {
                    return Err(HeartbeatCompileIssue {
                        line,
                        code: "jitter_out_of_bounds".to_string(),
                        message: "jitter must be between 0 and 3600 seconds".to_string(),
                    });
                }
                opts.jitter_seconds = parsed;
            }
            _ => {
                return Err(HeartbeatCompileIssue {
                    line,
                    code: "unknown_option".to_string(),
                    message: format!("unsupported heartbeat option `{key}`"),
                });
            }
        }
    }
    Ok(opts)
}

fn tokenize_heartbeat_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in line.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    out.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn normalize_context_relative_path(relative_path: &str) -> Option<String> {
    let trimmed = relative_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        return None;
    }

    let mut normalized_parts = Vec::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => {
                let value = part.to_string_lossy();
                if value.is_empty() {
                    return None;
                }
                normalized_parts.push(value.to_string());
            }
            _ => return None,
        }
    }

    if normalized_parts.is_empty() {
        return None;
    }
    Some(normalized_parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::{
        classify_mutability, compile_heartbeat_markdown, default_required_files,
        load_agent_context_snapshot, normalize_required_files, AgentContextLoadError,
        AgentContextLoaderConfig, AgentContextMutability,
    };
    use std::{fs, path::PathBuf};
    use uuid::Uuid;

    fn make_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "secureagnt_agent_context_test_{}_{}",
            name,
            Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn load_snapshot_prefers_tenant_agent_directory() {
        let root = make_temp_dir("tenant_pref");
        let tenant = "tenant_a";
        let agent_id = Uuid::new_v4();
        let source = root.join(tenant).join(agent_id.to_string());
        fs::create_dir_all(source.join("memory")).expect("create memory dir");
        fs::create_dir_all(source.join("sessions")).expect("create sessions dir");
        fs::write(source.join("SOUL.md"), "soul").expect("write SOUL");
        fs::write(source.join("AGENTS.md"), "ops").expect("write AGENTS");
        fs::write(source.join("memory/2026-02-20.md"), "daily").expect("write daily");
        fs::write(
            source.join("sessions/session-a.jsonl"),
            "{\"event\":\"x\"}\n",
        )
        .expect("write session");

        let config = AgentContextLoaderConfig {
            root_dir: root.clone(),
            required_files: vec!["SOUL.md".to_string(), "AGENTS.md".to_string()],
            max_file_bytes: 4096,
            max_total_bytes: 4096,
            max_dynamic_files_per_dir: 4,
        };
        let snapshot = load_agent_context_snapshot(&config, tenant, agent_id).expect("load");
        assert_eq!(snapshot.source_dir, source);
        assert_eq!(snapshot.missing_required_files.len(), 0);
        assert_eq!(snapshot.required_files.len(), 2);
        assert_eq!(snapshot.memory_files.len(), 1);
        assert_eq!(snapshot.session_files.len(), 1);
        assert!(snapshot.aggregate_sha256().len() >= 64);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn load_snapshot_uses_flat_agent_fallback() {
        let root = make_temp_dir("flat_fallback");
        let tenant = "tenant_missing";
        let agent_id = Uuid::new_v4();
        let source = root.join(agent_id.to_string());
        fs::create_dir_all(&source).expect("create source");
        fs::write(source.join("SOUL.md"), "soul").expect("write SOUL");

        let config = AgentContextLoaderConfig {
            root_dir: root.clone(),
            required_files: vec!["SOUL.md".to_string()],
            max_file_bytes: 4096,
            max_total_bytes: 4096,
            max_dynamic_files_per_dir: 2,
        };
        let snapshot = load_agent_context_snapshot(&config, tenant, agent_id).expect("load");
        assert_eq!(snapshot.source_dir, source);
        assert_eq!(snapshot.required_files.len(), 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn load_snapshot_returns_not_found_when_context_directory_absent() {
        let root = make_temp_dir("not_found");
        let err = load_agent_context_snapshot(
            &AgentContextLoaderConfig {
                root_dir: root.clone(),
                required_files: default_required_files(),
                max_file_bytes: 4096,
                max_total_bytes: 8192,
                max_dynamic_files_per_dir: 4,
            },
            "single",
            Uuid::new_v4(),
        )
        .expect_err("expected not found");
        match err {
            AgentContextLoadError::NotFound { searched_paths } => {
                assert_eq!(searched_paths.len(), 2);
            }
            other => panic!("unexpected error: {other:?}"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn normalize_required_files_deduplicates_and_trims() {
        let normalized = normalize_required_files(&[
            " SOUL.md ".to_string(),
            "SOUL.md".to_string(),
            "".to_string(),
            "USER.md".to_string(),
        ]);
        assert_eq!(
            normalized,
            vec!["SOUL.md".to_string(), "USER.md".to_string()]
        );
    }

    #[test]
    fn classify_mutability_returns_expected_levels() {
        assert_eq!(
            classify_mutability("AGENTS.md"),
            Some(AgentContextMutability::Immutable)
        );
        assert_eq!(
            classify_mutability("SOUL.md"),
            Some(AgentContextMutability::Immutable)
        );
        assert_eq!(
            classify_mutability("USER.md"),
            Some(AgentContextMutability::HumanPrimary)
        );
        assert_eq!(
            classify_mutability("HEARTBEAT.md"),
            Some(AgentContextMutability::HumanPrimary)
        );
        assert_eq!(
            classify_mutability("BOOTSTRAP.md"),
            Some(AgentContextMutability::HumanPrimary)
        );
        assert_eq!(
            classify_mutability("MEMORY.md"),
            Some(AgentContextMutability::AgentManaged)
        );
        assert_eq!(
            classify_mutability("memory/2026-02-20.md"),
            Some(AgentContextMutability::AgentManaged)
        );
        assert_eq!(
            classify_mutability("sessions/2026-02-20.jsonl"),
            Some(AgentContextMutability::AgentManaged)
        );
        assert_eq!(classify_mutability("../SOUL.md"), None);
    }

    #[test]
    fn compile_heartbeat_markdown_parses_interval_and_cron_lines() {
        let report = compile_heartbeat_markdown(
            r#"
# schedule
- every 900 recipe=show_notes_v1 max_inflight=2 jitter=5
- cron "0 * * * * *" recipe=nightly_rollup timezone=UTC max_inflight=1
"#,
        );
        assert_eq!(report.issues.len(), 0);
        assert_eq!(report.candidates.len(), 2);
        assert_eq!(report.candidates[0].interval_seconds, Some(900));
        assert_eq!(report.candidates[0].recipe_id, "show_notes_v1");
        assert_eq!(
            report.candidates[1].cron_expression.as_deref(),
            Some("0 * * * * *")
        );
        assert_eq!(report.candidates[1].timezone.as_deref(), Some("UTC"));
    }

    #[test]
    fn compile_heartbeat_markdown_reports_invalid_lines() {
        let report = compile_heartbeat_markdown(
            r#"
every 30 recipe=too_fast
cron "bad cron" recipe=x
every 900 max_inflight=2
"#,
        );
        assert_eq!(report.candidates.len(), 0);
        assert_eq!(report.issues.len(), 3);
        assert_eq!(report.issues[0].code, "interval_out_of_bounds");
        assert_eq!(report.issues[1].code, "invalid_cron_expression");
        assert_eq!(report.issues[2].code, "missing_recipe");
    }
}
