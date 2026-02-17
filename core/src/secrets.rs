use anyhow::{anyhow, Context, Result};
use std::{env, fs};

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

#[derive(Debug, Default, Clone, Copy)]
pub struct EnvFileSecretResolver;

impl SecretResolver for EnvFileSecretResolver {
    fn resolve(&self, reference: &SecretReference) -> Result<String> {
        match reference.backend {
            SecretBackend::Env => env::var(reference.key.as_str())
                .with_context(|| format!("missing environment secret `{}`", reference.key)),
            SecretBackend::File => fs::read_to_string(reference.key.as_str())
                .map(|value| value.trim().to_string())
                .with_context(|| format!("failed reading secret file `{}`", reference.key)),
            unsupported => Err(anyhow!(
                "secret backend `{}` is not configured in this build",
                unsupported.as_str()
            )),
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

#[cfg(test)]
mod tests {
    use super::{resolve_secret_value, EnvFileSecretResolver, SecretBackend, SecretReference};

    #[test]
    fn parse_secret_reference_supports_known_backends() {
        let parsed = SecretReference::parse("vault:kv/data/app/slack").expect("parse");
        assert_eq!(parsed.backend, SecretBackend::Vault);
        assert_eq!(parsed.key, "kv/data/app/slack");
    }

    #[test]
    fn parse_secret_reference_rejects_unknown_backend() {
        let err = SecretReference::parse("docker:secret/path").expect_err("must fail");
        assert!(err.to_string().contains("unsupported secret backend"));
    }

    #[test]
    fn resolve_secret_value_prefers_reference() {
        let key = "AEGIS_SECRET_TEST_ENV";
        std::env::set_var(key, "from-env");
        let resolver = EnvFileSecretResolver;
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
}
