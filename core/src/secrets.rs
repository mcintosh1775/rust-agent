use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::{
    collections::HashMap,
    env, fs,
    process::Command,
    sync::Mutex,
    time::{Duration, Instant},
};

const DEFAULT_SECRET_CACHE_TTL_SECS: u64 = 30;
const DEFAULT_SECRET_CACHE_MAX_ENTRIES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretBackend {
    Env,
    File,
    Vault,
    AwsSecretsManager,
    GcpSecretManager,
    AzureKeyVault,
}

impl SecretBackend {
    fn as_str(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::File => "file",
            Self::Vault => "vault",
            Self::AwsSecretsManager => "aws-sm",
            Self::GcpSecretManager => "gcp-sm",
            Self::AzureKeyVault => "azure-kv",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretReference {
    pub backend: SecretBackend,
    pub key: String,
}

impl SecretReference {
    pub fn parse(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        let (scheme, key) = trimmed
            .split_once(':')
            .ok_or_else(|| anyhow!("secret reference must be scheme-prefixed (e.g., env:VAR)"))?;
        let backend = match scheme.trim().to_ascii_lowercase().as_str() {
            "env" => SecretBackend::Env,
            "file" => SecretBackend::File,
            "vault" => SecretBackend::Vault,
            "aws-sm" => SecretBackend::AwsSecretsManager,
            "gcp-sm" => SecretBackend::GcpSecretManager,
            "azure-kv" => SecretBackend::AzureKeyVault,
            other => {
                return Err(anyhow!(
                    "unsupported secret backend `{other}` (supported: env, file, vault, aws-sm, gcp-sm, azure-kv)"
                ))
            }
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(anyhow!("secret reference key/path must not be empty"));
        }

        Ok(Self {
            backend,
            key: key.to_string(),
        })
    }

    fn cache_key(&self) -> String {
        format!("{}:{}", self.backend.as_str(), self.key)
    }
}

pub trait SecretResolver {
    fn resolve(&self, reference: &SecretReference) -> Result<String>;
}

#[derive(Debug, Clone, Copy)]
pub struct CliSecretResolver {
    pub enable_cloud_cli_backends: bool,
}

impl CliSecretResolver {
    pub fn from_env() -> Self {
        Self {
            enable_cloud_cli_backends: env::var("SECUREAGNT_SECRET_ENABLE_CLOUD_CLI")
                .ok()
                .as_deref()
                .map(parse_env_bool)
                .unwrap_or(false),
        }
    }

    fn ensure_cloud_enabled(&self, backend: SecretBackend) -> Result<()> {
        if self.enable_cloud_cli_backends {
            Ok(())
        } else {
            Err(anyhow!(
                "secret backend `{}` is disabled; set SECUREAGNT_SECRET_ENABLE_CLOUD_CLI=1 to enable CLI adapters",
                backend.as_str()
            ))
        }
    }
}

#[derive(Debug)]
struct CachedSecretValue {
    value: String,
    expires_at: Instant,
}

#[derive(Debug)]
pub struct CachedSecretResolver<R: SecretResolver> {
    inner: R,
    ttl: Duration,
    max_entries: usize,
    cache: Mutex<HashMap<String, CachedSecretValue>>,
}

impl<R: SecretResolver> CachedSecretResolver<R> {
    pub fn new(inner: R, ttl: Duration, max_entries: usize) -> Self {
        Self {
            inner,
            ttl,
            max_entries: max_entries.max(1),
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn from_env_with(inner: R) -> Self {
        let ttl_secs = env::var("SECUREAGNT_SECRET_CACHE_TTL_SECS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_SECRET_CACHE_TTL_SECS);
        let max_entries = env::var("SECUREAGNT_SECRET_CACHE_MAX_ENTRIES")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(DEFAULT_SECRET_CACHE_MAX_ENTRIES);
        Self::new(inner, Duration::from_secs(ttl_secs), max_entries)
    }

    fn lookup_cached(&self, cache_key: &str, now: Instant) -> Option<String> {
        let Ok(mut guard) = self.cache.lock() else {
            return None;
        };
        if let Some(entry) = guard.get(cache_key) {
            if now < entry.expires_at {
                return Some(entry.value.clone());
            }
        }
        guard.remove(cache_key);
        None
    }

    fn store_cached(&self, cache_key: String, value: String, now: Instant) {
        let Ok(mut guard) = self.cache.lock() else {
            return;
        };
        guard.retain(|_, entry| now < entry.expires_at);
        if guard.len() >= self.max_entries {
            if let Some(evict_key) = guard.keys().next().cloned() {
                guard.remove(evict_key.as_str());
            }
        }
        guard.insert(
            cache_key,
            CachedSecretValue {
                value,
                expires_at: now + self.ttl,
            },
        );
    }
}

impl SecretResolver for CliSecretResolver {
    fn resolve(&self, reference: &SecretReference) -> Result<String> {
        match reference.backend {
            SecretBackend::Env => env::var(reference.key.as_str())
                .with_context(|| format!("missing environment secret `{}`", reference.key)),
            SecretBackend::File => fs::read_to_string(reference.key.as_str())
                .map(|value| value.trim().to_string())
                .with_context(|| format!("failed reading secret file `{}`", reference.key)),
            SecretBackend::Vault => {
                self.ensure_cloud_enabled(reference.backend)?;
                resolve_with_vault_cli(reference.key.as_str())
            }
            SecretBackend::AwsSecretsManager => {
                self.ensure_cloud_enabled(reference.backend)?;
                resolve_with_aws_sm_cli(reference.key.as_str())
            }
            SecretBackend::GcpSecretManager => {
                self.ensure_cloud_enabled(reference.backend)?;
                resolve_with_gcp_sm_cli(reference.key.as_str())
            }
            SecretBackend::AzureKeyVault => {
                self.ensure_cloud_enabled(reference.backend)?;
                resolve_with_azure_kv_cli(reference.key.as_str())
            }
        }
    }
}

impl<R: SecretResolver> SecretResolver for CachedSecretResolver<R> {
    fn resolve(&self, reference: &SecretReference) -> Result<String> {
        if self.ttl.is_zero() {
            return self.inner.resolve(reference);
        }

        let now = Instant::now();
        let cache_key = reference.cache_key();
        if let Some(value) = self.lookup_cached(cache_key.as_str(), now) {
            return Ok(value);
        }

        let value = self.inner.resolve(reference)?;
        self.store_cached(cache_key, value.clone(), now);
        Ok(value)
    }
}

pub fn resolve_secret_value<R: SecretResolver>(
    direct_value: Option<String>,
    reference_value: Option<String>,
    resolver: &R,
) -> Result<Option<String>> {
    if let Some(reference_raw) = reference_value {
        let reference = SecretReference::parse(reference_raw.as_str())?;
        return resolver.resolve(&reference).map(Some);
    }

    Ok(direct_value.and_then(|value| {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }))
}

fn resolve_with_vault_cli(key: &str) -> Result<String> {
    let (base_key, params) = split_key_and_params(key)?;
    ensure_only_known_params(&params, &["version"])?;
    let version = params
        .get("version")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty());
    let (path, field) = match base_key.split_once('#') {
        Some((path, field)) => (path.trim(), Some(field.trim())),
        None => (base_key.trim(), None),
    };
    let mut args = vec![
        "kv".to_string(),
        "get".to_string(),
        "-format=json".to_string(),
    ];
    if let Some(version) = version {
        args.push(format!("-version={version}"));
    }
    args.push(path.to_string());
    let output = run_cli_owned("vault", args, "vault secret fetch failed")?;
    let parsed: Value = serde_json::from_str(output.as_str())
        .with_context(|| "failed decoding vault JSON output")?;
    let data = parsed
        .get("data")
        .and_then(|v| v.get("data"))
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("vault response missing data.data object"))?;

    if let Some(field) = field {
        let value = data
            .get(field)
            .ok_or_else(|| anyhow!("vault secret missing field `{field}`"))?;
        return value_from_json(value);
    }

    if data.len() == 1 {
        let value = data.values().next().expect("len checked");
        return value_from_json(value);
    }

    Err(anyhow!(
        "vault secret has multiple fields; use vault:path#field to select one"
    ))
}

fn resolve_with_aws_sm_cli(key: &str) -> Result<String> {
    let (secret_id, params) = split_key_and_params(key)?;
    ensure_only_known_params(&params, &["version_id", "version_stage"])?;
    let version_id = params
        .get("version_id")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty());
    let version_stage = params
        .get("version_stage")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty());
    if version_id.is_some() && version_stage.is_some() {
        return Err(anyhow!(
            "aws secret reference must include at most one of `version_id` or `version_stage`"
        ));
    }

    let mut args = vec![
        "secretsmanager".to_string(),
        "get-secret-value".to_string(),
        "--secret-id".to_string(),
        secret_id.to_string(),
        "--query".to_string(),
        "SecretString".to_string(),
        "--output".to_string(),
        "text".to_string(),
    ];
    if let Some(version_id) = version_id {
        args.push("--version-id".to_string());
        args.push(version_id.to_string());
    }
    if let Some(version_stage) = version_stage {
        args.push("--version-stage".to_string());
        args.push(version_stage.to_string());
    }

    run_cli_owned("aws", args, "aws secrets manager fetch failed")
}

fn resolve_with_gcp_sm_cli(key: &str) -> Result<String> {
    let (project, secret, version) = parse_gcp_secret_key(key)?;
    run_cli(
        "gcloud",
        &[
            "secrets",
            "versions",
            "access",
            version.as_str(),
            "--secret",
            secret.as_str(),
            "--project",
            project.as_str(),
            "--quiet",
        ],
        "gcp secret manager fetch failed",
    )
}

fn resolve_with_azure_kv_cli(key: &str) -> Result<String> {
    let (base_id, params) = split_key_and_params(key)?;
    ensure_only_known_params(&params, &["version"])?;
    let mut secret_id = base_id.to_string();
    if let Some(version) = params
        .get("version")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        secret_id = format!("{}/{}", secret_id.trim_end_matches('/'), version);
    }
    run_cli(
        "az",
        &[
            "keyvault",
            "secret",
            "show",
            "--id",
            secret_id.as_str(),
            "--query",
            "value",
            "-o",
            "tsv",
        ],
        "azure key vault fetch failed",
    )
}

fn parse_gcp_secret_key(key: &str) -> Result<(String, String, String)> {
    let (base_key, params) = split_key_and_params(key)?;
    ensure_only_known_params(&params, &["version"])?;
    let normalized = base_key.trim().trim_matches('/');
    let query_version = params
        .get("version")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty());
    if normalized.contains("/secrets/") && normalized.contains("/versions/") {
        let parts: Vec<&str> = normalized.split('/').collect();
        if parts.len() >= 6 {
            let project = parts.get(1).copied().unwrap_or_default();
            let secret = parts.get(3).copied().unwrap_or_default();
            let version = parts.get(5).copied().unwrap_or_default();
            if !project.is_empty() && !secret.is_empty() && !version.is_empty() {
                if let Some(query_version) = query_version {
                    if query_version != version {
                        return Err(anyhow!(
                            "gcp secret reference version mismatch between path and query parameter"
                        ));
                    }
                }
                return Ok((project.to_string(), secret.to_string(), version.to_string()));
            }
        }
    }

    let parts: Vec<&str> = normalized.split(':').collect();
    if parts.len() == 3 || (parts.len() == 2 && query_version.is_some()) {
        let project = parts[0].trim();
        let secret = parts[1].trim();
        let version = if parts.len() == 3 {
            parts[2].trim()
        } else {
            query_version.unwrap_or_default().trim()
        };
        if parts.len() == 3 {
            if let Some(query_version) = query_version {
                if query_version != version {
                    return Err(anyhow!(
                        "gcp secret reference version mismatch between key and query parameter"
                    ));
                }
            }
        }
        if !project.is_empty() && !secret.is_empty() && !version.is_empty() {
            return Ok((project.to_string(), secret.to_string(), version.to_string()));
        }
    }

    Err(anyhow!(
        "gcp secret key must be `project:secret:version` or `projects/<project>/secrets/<secret>/versions/<version>`"
    ))
}

fn split_key_and_params(raw: &str) -> Result<(&str, HashMap<String, String>)> {
    let trimmed = raw.trim();
    let (base, query) = match trimmed.split_once('?') {
        Some((base, query)) => (base.trim(), Some(query.trim())),
        None => (trimmed, None),
    };
    if base.is_empty() {
        return Err(anyhow!("secret reference key/path must not be empty"));
    }

    let mut params = HashMap::new();
    if let Some(query) = query {
        if query.is_empty() {
            return Err(anyhow!("secret reference query string must not be empty"));
        }
        for pair in query.split('&').filter(|value| !value.trim().is_empty()) {
            let (raw_key, raw_value) = pair
                .split_once('=')
                .ok_or_else(|| anyhow!("invalid secret query parameter `{pair}`; expected k=v"))?;
            let key = raw_key.trim().to_ascii_lowercase();
            let value = raw_value.trim();
            if key.is_empty() || value.is_empty() {
                return Err(anyhow!(
                    "invalid secret query parameter `{pair}`; key and value must be non-empty"
                ));
            }
            params.insert(key, value.to_string());
        }
    }

    Ok((base, params))
}

fn ensure_only_known_params(params: &HashMap<String, String>, allowed: &[&str]) -> Result<()> {
    for key in params.keys() {
        if !allowed
            .iter()
            .any(|allowed_key| allowed_key == &key.as_str())
        {
            return Err(anyhow!(
                "unsupported secret query parameter `{key}`; allowed: {}",
                allowed.join(", ")
            ));
        }
    }
    Ok(())
}

fn value_from_json(value: &Value) -> Result<String> {
    match value {
        Value::String(s) => Ok(s.trim().to_string()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        other => serde_json::to_string(other).with_context(|| "failed encoding JSON secret value"),
    }
}

fn run_cli(program: &str, args: &[&str], context: &str) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed launching `{program}` CLI"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("{context}: {}", stderr.trim()));
    }
    let stdout = String::from_utf8(output.stdout).with_context(|| "CLI output was not UTF-8")?;
    let trimmed = stdout.trim().to_string();
    if trimmed.is_empty() {
        return Err(anyhow!("{context}: CLI returned empty secret value"));
    }
    Ok(trimmed)
}

fn run_cli_owned(program: &str, args: Vec<String>, context: &str) -> Result<String> {
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run_cli(program, borrowed.as_slice(), context)
}

fn parse_env_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        parse_gcp_secret_key, resolve_secret_value, resolve_with_aws_sm_cli,
        resolve_with_azure_kv_cli, resolve_with_vault_cli, split_key_and_params,
        CachedSecretResolver, CliSecretResolver, SecretBackend, SecretReference, SecretResolver,
    };
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::{
        env, fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
        sync::{Mutex, OnceLock},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    struct CountingResolver {
        calls: AtomicUsize,
    }

    impl CountingResolver {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl SecretResolver for CountingResolver {
        fn resolve(&self, _reference: &SecretReference) -> anyhow::Result<String> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(format!("value-{call}"))
        }
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[cfg(unix)]
    fn with_mock_cli<F>(program: &str, script_body: &str, test_fn: F)
    where
        F: FnOnce(PathBuf),
    {
        let _guard = env_lock().lock().expect("lock");
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let temp_dir = env::temp_dir().join(format!("secureagnt-secret-cli-test-{unique}"));
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        let script_path = temp_dir.join(program);
        fs::write(&script_path, script_body).expect("write mock cli script");
        let mut perms = fs::metadata(&script_path)
            .expect("script metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).expect("chmod mock cli script");

        let prior_path = env::var("PATH").ok();
        let new_path = match &prior_path {
            Some(value) if !value.trim().is_empty() => format!("{}:{value}", temp_dir.display()),
            _ => temp_dir.display().to_string(),
        };
        env::set_var("PATH", new_path);

        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| test_fn(temp_dir.clone())));

        match prior_path {
            Some(value) => env::set_var("PATH", value),
            None => env::remove_var("PATH"),
        }
        let _ = fs::remove_dir_all(&temp_dir);

        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    }

    #[test]
    fn parse_secret_reference_supports_known_backends() {
        let parsed = SecretReference::parse("vault:kv/data/app/slack#token").expect("parse");
        assert_eq!(parsed.backend, SecretBackend::Vault);
        assert_eq!(parsed.key, "kv/data/app/slack#token");
    }

    #[test]
    fn parse_secret_reference_rejects_unknown_backend() {
        let err = SecretReference::parse("docker:secret/path").expect_err("must fail");
        assert!(err.to_string().contains("unsupported secret backend"));
    }

    #[test]
    fn parse_gcp_key_supports_short_and_full_formats() {
        let short = parse_gcp_secret_key("proj-a:secret-a:latest").expect("short");
        assert_eq!(short.0, "proj-a");
        let full = parse_gcp_secret_key("projects/p/secrets/s/versions/latest").expect("full");
        assert_eq!(full.0, "p");
        assert_eq!(full.1, "s");
        assert_eq!(full.2, "latest");
    }

    #[test]
    fn parse_gcp_key_supports_query_version_pin() {
        let parsed = parse_gcp_secret_key("proj-a:secret-a?version=7").expect("query version");
        assert_eq!(parsed.0, "proj-a");
        assert_eq!(parsed.1, "secret-a");
        assert_eq!(parsed.2, "7");
    }

    #[test]
    fn parse_gcp_key_rejects_mismatched_query_version_pin() {
        let err = parse_gcp_secret_key("proj-a:secret-a:6?version=7").expect_err("must fail");
        assert!(err.to_string().contains("version mismatch"));
    }

    #[test]
    fn parse_gcp_key_rejects_unknown_query_param() {
        let err = parse_gcp_secret_key("proj-a:secret-a?foo=bar").expect_err("must fail");
        assert!(err
            .to_string()
            .contains("unsupported secret query parameter"));
    }

    #[test]
    fn split_key_and_params_parses_version_and_stage() {
        let (base, params) =
            split_key_and_params("my/secret?version_id=abc&version_stage=prod").expect("params");
        assert_eq!(base, "my/secret");
        assert_eq!(params.get("version_id").map(String::as_str), Some("abc"));
        assert_eq!(
            params.get("version_stage").map(String::as_str),
            Some("prod")
        );
    }

    #[test]
    fn split_key_and_params_rejects_missing_equals() {
        let err = split_key_and_params("my/secret?version").expect_err("must fail");
        assert!(err.to_string().contains("expected k=v"));
    }

    #[test]
    fn resolve_secret_value_prefers_reference() {
        let key = "SECUREAGNT_SECRET_TEST_ENV";
        std::env::set_var(key, "from-env");
        let resolver = CliSecretResolver {
            enable_cloud_cli_backends: false,
        };
        let resolved = resolve_secret_value(
            Some("direct-value".to_string()),
            Some(format!("env:{key}")),
            &resolver,
        )
        .expect("resolve")
        .expect("value");
        assert_eq!(resolved, "from-env");
        std::env::remove_var(key);
    }

    #[test]
    fn cloud_backends_fail_closed_when_disabled() {
        let resolver = CliSecretResolver {
            enable_cloud_cli_backends: false,
        };
        let reference =
            SecretReference::parse("vault:kv/data/secureagnt#token").expect("reference");
        let err = resolver.resolve(&reference).expect_err("must fail closed");
        assert!(err.to_string().contains("disabled"));
    }

    #[test]
    fn secureagnt_cloud_gate_env_is_respected() {
        let secure_key = "SECUREAGNT_SECRET_ENABLE_CLOUD_CLI";
        let prior_secure = std::env::var(secure_key).ok();
        std::env::set_var(secure_key, "1");

        let resolver = CliSecretResolver::from_env();
        assert!(resolver.enable_cloud_cli_backends);

        match prior_secure {
            Some(value) => std::env::set_var(secure_key, value),
            None => std::env::remove_var(secure_key),
        }
    }

    #[test]
    fn cached_secret_resolver_returns_cached_value_before_ttl() {
        let inner = CountingResolver::new();
        let cached = CachedSecretResolver::new(inner, Duration::from_millis(200), 8);
        let reference = SecretReference::parse("env:SECUREAGNT_NOT_USED").expect("reference");

        let first = cached.resolve(&reference).expect("first");
        let second = cached.resolve(&reference).expect("second");

        assert_eq!(first, "value-1");
        assert_eq!(second, "value-1");
        assert_eq!(cached.inner.calls(), 1);
    }

    #[test]
    fn cached_secret_resolver_refreshes_after_ttl_expiry() {
        let inner = CountingResolver::new();
        let cached = CachedSecretResolver::new(inner, Duration::from_millis(20), 8);
        let reference = SecretReference::parse("env:SECUREAGNT_NOT_USED").expect("reference");

        let first = cached.resolve(&reference).expect("first");
        std::thread::sleep(Duration::from_millis(30));
        let second = cached.resolve(&reference).expect("second");

        assert_eq!(first, "value-1");
        assert_eq!(second, "value-2");
        assert_eq!(cached.inner.calls(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn vault_cli_adapter_supports_version_pin_and_field_selection() {
        with_mock_cli(
            "vault",
            r#"#!/bin/sh
printf '%s\n' "$@" > "$MOCK_ARGS_FILE"
cat <<'JSON'
{"data":{"data":{"token":"vault-v3","extra":"x"}}}
JSON
"#,
            |temp_dir| {
                let args_file = temp_dir.join("vault.args");
                env::set_var("MOCK_ARGS_FILE", &args_file);
                let value =
                    resolve_with_vault_cli("kv/data/app#token?version=3").expect("vault resolve");
                env::remove_var("MOCK_ARGS_FILE");

                assert_eq!(value, "vault-v3");
                let args = fs::read_to_string(args_file).expect("read vault args");
                assert!(args.contains("-version=3"));
                assert!(args.contains("kv/data/app"));
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn aws_cli_adapter_surfaces_provider_errors() {
        with_mock_cli(
            "aws",
            r#"#!/bin/sh
echo "provider denied access" >&2
exit 2
"#,
            |_temp_dir| {
                let err = resolve_with_aws_sm_cli("prod/secureagnt/slack?version_stage=AWSCURRENT")
                    .expect_err("must fail");
                assert!(err.to_string().contains("aws secrets manager fetch failed"));
                assert!(err.to_string().contains("provider denied access"));
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn azure_cli_adapter_appends_version_segment() {
        with_mock_cli(
            "az",
            r#"#!/bin/sh
printf '%s\n' "$@" > "$MOCK_ARGS_FILE"
echo "azure-secret-value"
"#,
            |temp_dir| {
                let args_file = temp_dir.join("azure.args");
                env::set_var("MOCK_ARGS_FILE", &args_file);
                let value = resolve_with_azure_kv_cli(
                    "https://vault.example/secrets/secureagnt?version=abcd1234",
                )
                .expect("azure resolve");
                env::remove_var("MOCK_ARGS_FILE");

                assert_eq!(value, "azure-secret-value");
                let args = fs::read_to_string(args_file).expect("read azure args");
                assert!(args.contains("--id"));
                assert!(args.contains("https://vault.example/secrets/secureagnt/abcd1234"));
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn cached_cli_secret_resolver_picks_up_version_rollover_after_ttl() {
        with_mock_cli(
            "aws",
            r#"#!/bin/sh
cat "$MOCK_SECRET_FILE"
"#,
            |temp_dir| {
                let secret_file = temp_dir.join("secret.value");
                fs::write(&secret_file, "stage-v1\n").expect("write v1");
                env::set_var("MOCK_SECRET_FILE", &secret_file);

                let resolver = CachedSecretResolver::new(
                    CliSecretResolver {
                        enable_cloud_cli_backends: true,
                    },
                    Duration::from_millis(20),
                    8,
                );
                let reference =
                    SecretReference::parse("aws-sm:prod/secureagnt/slack?version_stage=AWSCURRENT")
                        .expect("reference");

                let first = resolver.resolve(&reference).expect("first resolve");
                assert_eq!(first, "stage-v1");

                fs::write(&secret_file, "stage-v2\n").expect("write v2");
                let cached = resolver.resolve(&reference).expect("cached resolve");
                assert_eq!(cached, "stage-v1");

                std::thread::sleep(Duration::from_millis(30));
                let refreshed = resolver.resolve(&reference).expect("refreshed resolve");
                assert_eq!(refreshed, "stage-v2");

                env::remove_var("MOCK_SECRET_FILE");
            },
        );
    }
}
