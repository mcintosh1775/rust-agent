use anyhow::{anyhow, Context, Result};
use nostr::{Keys, PublicKey, SecretKey, ToBech32};
use std::{env, fs, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NostrSignerMode {
    LocalKey,
    Nip46Signer,
}

impl NostrSignerMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalKey => "local_key",
            Self::Nip46Signer => "nip46_signer",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "local_key" | "local" => Ok(Self::LocalKey),
            "nip46_signer" | "nip46" => Ok(Self::Nip46Signer),
            other => Err(anyhow!(
                "invalid NOSTR_SIGNER_MODE `{}` (supported: local_key, nip46_signer)",
                other
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NostrSignerIdentity {
    pub mode: NostrSignerMode,
    pub public_key: String,
}

#[derive(Debug, Clone, Default)]
pub struct NostrSignerConfig {
    pub mode: NostrSignerMode,
    pub local_secret_key: Option<String>,
    pub local_secret_key_file: Option<PathBuf>,
    pub nip46_bunker_uri: Option<String>,
    pub nip46_public_key: Option<String>,
}

impl NostrSignerConfig {
    pub fn from_env() -> Result<Self> {
        let mode_value = env::var("NOSTR_SIGNER_MODE").unwrap_or_else(|_| "local_key".to_string());
        let mode = NostrSignerMode::parse(&mode_value)?;
        Ok(Self {
            mode,
            local_secret_key: env::var("NOSTR_SECRET_KEY").ok(),
            local_secret_key_file: env::var("NOSTR_SECRET_KEY_FILE").ok().map(PathBuf::from),
            nip46_bunker_uri: env::var("NOSTR_NIP46_BUNKER_URI").ok(),
            nip46_public_key: env::var("NOSTR_NIP46_PUBLIC_KEY")
                .ok()
                .or_else(|| env::var("NOSTR_PUBLIC_KEY").ok()),
        })
    }

    pub fn resolve_identity(&self) -> Result<Option<NostrSignerIdentity>> {
        match self.mode {
            NostrSignerMode::LocalKey => self.resolve_local_identity(),
            NostrSignerMode::Nip46Signer => self.resolve_nip46_identity(),
        }
    }

    fn resolve_local_identity(&self) -> Result<Option<NostrSignerIdentity>> {
        let Some(secret_key) = self.load_local_secret_key()? else {
            return Ok(None);
        };
        let keys = Keys::new(secret_key);
        let public_key = keys
            .public_key()
            .to_bech32()
            .context("failed to encode local signer public key")?;
        Ok(Some(NostrSignerIdentity {
            mode: NostrSignerMode::LocalKey,
            public_key,
        }))
    }

    fn resolve_nip46_identity(&self) -> Result<Option<NostrSignerIdentity>> {
        let bunker_uri = self
            .nip46_bunker_uri
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                anyhow!("NOSTR_NIP46_BUNKER_URI is required when NOSTR_SIGNER_MODE=nip46_signer")
            })?;

        let public_key = self
            .nip46_public_key
            .as_deref()
            .and_then(non_empty_trimmed)
            .map(normalize_public_key)
            .transpose()?
            .or_else(|| extract_bunker_pubkey(bunker_uri))
            .ok_or_else(|| {
                anyhow!(
                    "NIP-46 signer requires NOSTR_NIP46_PUBLIC_KEY or bunker URI containing the public key"
                )
            })?;

        Ok(Some(NostrSignerIdentity {
            mode: NostrSignerMode::Nip46Signer,
            public_key,
        }))
    }

    fn load_local_secret_key(&self) -> Result<Option<SecretKey>> {
        if let Some(value) = self.local_secret_key.as_deref().and_then(non_empty_trimmed) {
            return parse_secret_key(value).map(Some);
        }

        let Some(path) = &self.local_secret_key_file else {
            return Ok(None);
        };

        enforce_secret_file_permissions(path)?;
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read local key file `{}`", path.display()))?;
        let value = non_empty_trimmed(content.as_str())
            .ok_or_else(|| anyhow!("local key file `{}` is empty", path.display()))?;
        parse_secret_key(value).map(Some)
    }
}

fn parse_secret_key(value: &str) -> Result<SecretKey> {
    SecretKey::parse(value)
        .with_context(|| "failed to parse local Nostr secret key (expected nsec or hex format)")
}

fn normalize_public_key(value: &str) -> Result<String> {
    let parsed = PublicKey::parse(value)
        .with_context(|| "failed to parse Nostr public key (expected npub or hex format)")?;
    parsed
        .to_bech32()
        .context("failed to encode normalized Nostr public key")
}

fn extract_bunker_pubkey(uri: &str) -> Option<String> {
    let after_scheme = uri.split_once("://")?.1;
    let candidate = after_scheme
        .split(['/', '?', '#'])
        .next()
        .and_then(non_empty_trimmed)?;
    normalize_public_key(candidate).ok()
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[cfg(unix)]
fn enforce_secret_file_permissions(path: &PathBuf) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to stat local key file `{}`", path.display()))?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(anyhow!(
            "local key file `{}` is too permissive (mode {:o}); require owner-only permissions (0600)",
            path.display(),
            mode
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn enforce_secret_file_permissions(_path: &PathBuf) -> Result<()> {
    Ok(())
}

impl Default for NostrSignerMode {
    fn default() -> Self {
        Self::LocalKey
    }
}

#[cfg(test)]
mod tests {
    use super::{NostrSignerConfig, NostrSignerMode};
    use nostr::{Keys, ToBech32};

    #[test]
    fn default_mode_is_local_key() {
        assert_eq!(NostrSignerMode::default(), NostrSignerMode::LocalKey);
    }

    #[test]
    fn local_mode_without_key_is_optional() {
        let config = NostrSignerConfig {
            mode: NostrSignerMode::LocalKey,
            local_secret_key: None,
            local_secret_key_file: None,
            nip46_bunker_uri: None,
            nip46_public_key: None,
        };

        let identity = config
            .resolve_identity()
            .expect("local mode without key should not error");
        assert!(identity.is_none());
    }

    #[test]
    fn local_mode_derives_public_key_from_hex_secret() {
        let config = NostrSignerConfig {
            mode: NostrSignerMode::LocalKey,
            local_secret_key: Some(
                "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
            ),
            local_secret_key_file: None,
            nip46_bunker_uri: None,
            nip46_public_key: None,
        };

        let identity = config
            .resolve_identity()
            .expect("local key should parse")
            .expect("identity should be present");
        assert_eq!(identity.mode, NostrSignerMode::LocalKey);
        assert!(identity.public_key.starts_with("npub1"));
    }

    #[test]
    fn nip46_mode_requires_bunker_uri() {
        let config = NostrSignerConfig {
            mode: NostrSignerMode::Nip46Signer,
            local_secret_key: None,
            local_secret_key_file: None,
            nip46_bunker_uri: None,
            nip46_public_key: None,
        };

        let error = config
            .resolve_identity()
            .expect_err("nip46 mode must require bunker URI");
        assert!(error
            .to_string()
            .contains("NOSTR_NIP46_BUNKER_URI is required"));
    }

    #[test]
    fn nip46_mode_uses_explicit_public_key() {
        let keys = Keys::generate();
        let npub = keys
            .public_key()
            .to_bech32()
            .expect("generated pubkey should encode");
        let config = NostrSignerConfig {
            mode: NostrSignerMode::Nip46Signer,
            local_secret_key: None,
            local_secret_key_file: None,
            nip46_bunker_uri: Some("bunker://placeholder?relay=wss://relay.example".to_string()),
            nip46_public_key: Some(npub.clone()),
        };

        let identity = config
            .resolve_identity()
            .expect("explicit nip46 public key should parse")
            .expect("identity should be present");
        assert_eq!(identity.mode, NostrSignerMode::Nip46Signer);
        assert_eq!(identity.public_key, npub);
    }

    #[test]
    fn nip46_mode_extracts_pubkey_from_bunker_uri() {
        let keys = Keys::generate();
        let npub = keys
            .public_key()
            .to_bech32()
            .expect("generated pubkey should encode");
        let bunker_uri = format!(
            "{scheme}://{npub}?relay={relay}",
            scheme = "bunker",
            relay = "wss://relay.example"
        );

        let config = NostrSignerConfig {
            mode: NostrSignerMode::Nip46Signer,
            local_secret_key: None,
            local_secret_key_file: None,
            nip46_bunker_uri: Some(bunker_uri),
            nip46_public_key: None,
        };

        let identity = config
            .resolve_identity()
            .expect("bunker URI pubkey should parse")
            .expect("identity should be present");
        assert_eq!(identity.mode, NostrSignerMode::Nip46Signer);
        assert_eq!(identity.public_key, npub);
    }
}
