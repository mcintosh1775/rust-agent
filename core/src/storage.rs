use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackend {
    Postgres,
    Sqlite,
}

impl StorageBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Sqlite => "sqlite",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageBackendError {
    pub database_url: String,
    pub reason: String,
}

impl fmt::Display for StorageBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "failed to detect storage backend from DATABASE_URL `{}`: {}",
            self.database_url, self.reason
        )
    }
}

impl std::error::Error for StorageBackendError {}

pub fn detect_storage_backend(database_url: &str) -> Result<StorageBackend, StorageBackendError> {
    let trimmed = database_url.trim();
    if trimmed.is_empty() {
        return Err(StorageBackendError {
            database_url: database_url.to_string(),
            reason: "empty value".to_string(),
        });
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("postgres://") || lower.starts_with("postgresql://") {
        return Ok(StorageBackend::Postgres);
    }
    if lower.starts_with("sqlite:") {
        return Ok(StorageBackend::Sqlite);
    }

    Err(StorageBackendError {
        database_url: database_url.to_string(),
        reason: "unsupported URL scheme (expected postgres://, postgresql://, or sqlite:)"
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{detect_storage_backend, StorageBackend};

    #[test]
    fn detect_storage_backend_supports_postgres() {
        assert_eq!(
            detect_storage_backend("postgres://localhost:5432/agentdb").unwrap(),
            StorageBackend::Postgres
        );
        assert_eq!(
            detect_storage_backend("postgresql://localhost:5432/agentdb").unwrap(),
            StorageBackend::Postgres
        );
    }

    #[test]
    fn detect_storage_backend_supports_sqlite() {
        assert_eq!(
            detect_storage_backend("sqlite://var/solo-lite/secureagnt.sqlite3").unwrap(),
            StorageBackend::Sqlite
        );
        assert_eq!(
            detect_storage_backend("sqlite::memory:").unwrap(),
            StorageBackend::Sqlite
        );
    }

    #[test]
    fn detect_storage_backend_rejects_unknown_scheme() {
        let error = detect_storage_backend("mysql://localhost/agentdb").unwrap_err();
        assert!(error.reason.contains("unsupported URL scheme"));
    }
}
