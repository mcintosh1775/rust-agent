use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::{env, fs, process::Command};

const LEGACY_AEGIS_SECRET_GATE_DEPRECATION_DATE: &str = "2026-06-30";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
        let secure_value = env::var("SECUREAGNT_SECRET_ENABLE_CLOUD_CLI").ok();
        let legacy_value = env::var("AEGIS_SECRET_ENABLE_CLOUD_CLI").ok();
        if secure_value.is_none() && legacy_value.is_some() {
            eprintln!(
                "warning: AEGIS_SECRET_ENABLE_CLOUD_CLI is deprecated and will be removed after {}; use SECUREAGNT_SECRET_ENABLE_CLOUD_CLI instead",
                LEGACY_AEGIS_SECRET_GATE_DEPRECATION_DATE
            );
        }
        Self {
            enable_cloud_cli_backends: secure_value
                .as_deref()
                .or(legacy_value.as_deref())
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
    let (path, field) = match key.split_once('#') {
        Some((path, field)) => (path.trim(), Some(field.trim())),
        None => (key.trim(), None),
    };
    let output = run_cli(
        "vault",
        &["kv", "get", "-format=json", path],
        "vault secret fetch failed",
    )?;
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
    run_cli(
        "aws",
        &[
            "secretsmanager",
            "get-secret-value",
            "--secret-id",
            key,
            "--query",
            "SecretString",
            "--output",
            "text",
        ],
        "aws secrets manager fetch failed",
    )
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
    run_cli(
        "az",
        &[
            "keyvault", "secret", "show", "--id", key, "--query", "value", "-o", "tsv",
        ],
        "azure key vault fetch failed",
    )
}

fn parse_gcp_secret_key(key: &str) -> Result<(String, String, String)> {
    let normalized = key.trim().trim_matches('/');
    if normalized.contains("/secrets/") && normalized.contains("/versions/") {
        let parts: Vec<&str> = normalized.split('/').collect();
        if parts.len() >= 6 {
            let project = parts.get(1).copied().unwrap_or_default();
            let secret = parts.get(3).copied().unwrap_or_default();
            let version = parts.get(5).copied().unwrap_or_default();
            if !project.is_empty() && !secret.is_empty() && !version.is_empty() {
                return Ok((project.to_string(), secret.to_string(), version.to_string()));
            }
        }
    }

    let parts: Vec<&str> = normalized.split(':').collect();
    if parts.len() == 3 {
        let project = parts[0].trim();
        let secret = parts[1].trim();
        let version = parts[2].trim();
        if !project.is_empty() && !secret.is_empty() && !version.is_empty() {
            return Ok((project.to_string(), secret.to_string(), version.to_string()));
        }
    }

    Err(anyhow!(
        "gcp secret key must be `project:secret:version` or `projects/<project>/secrets/<secret>/versions/<version>`"
    ))
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

fn parse_env_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        parse_gcp_secret_key, resolve_secret_value, CliSecretResolver, SecretBackend,
        SecretReference, SecretResolver,
    };

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
        let legacy_key = "AEGIS_SECRET_ENABLE_CLOUD_CLI";
        let prior_secure = std::env::var(secure_key).ok();
        let prior_legacy = std::env::var(legacy_key).ok();
        std::env::remove_var(legacy_key);
        std::env::set_var(secure_key, "1");

        let resolver = CliSecretResolver::from_env();
        assert!(resolver.enable_cloud_cli_backends);

        match prior_secure {
            Some(value) => std::env::set_var(secure_key, value),
            None => std::env::remove_var(secure_key),
        }
        match prior_legacy {
            Some(value) => std::env::set_var(legacy_key, value),
            None => std::env::remove_var(legacy_key),
        }
    }

    #[test]
    fn legacy_cloud_gate_env_is_respected_when_secure_unset() {
        let secure_key = "SECUREAGNT_SECRET_ENABLE_CLOUD_CLI";
        let legacy_key = "AEGIS_SECRET_ENABLE_CLOUD_CLI";
        let prior_secure = std::env::var(secure_key).ok();
        let prior_legacy = std::env::var(legacy_key).ok();
        std::env::remove_var(secure_key);
        std::env::set_var(legacy_key, "1");

        let resolver = CliSecretResolver::from_env();
        assert!(resolver.enable_cloud_cli_backends);

        match prior_secure {
            Some(value) => std::env::set_var(secure_key, value),
            None => std::env::remove_var(secure_key),
        }
        match prior_legacy {
            Some(value) => std::env::set_var(legacy_key, value),
            None => std::env::remove_var(legacy_key),
        }
    }
}
