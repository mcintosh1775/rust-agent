use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
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

#[cfg(test)]
mod tests {
    use super::{
        default_required_files, load_agent_context_snapshot, normalize_required_files,
        AgentContextLoadError, AgentContextLoaderConfig,
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
}
