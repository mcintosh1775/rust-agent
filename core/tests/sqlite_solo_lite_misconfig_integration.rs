use core::DbPool;
use std::{env, fs, path::PathBuf};
use uuid::Uuid;

#[test]
fn sqlite_migrate_fails_closed_on_migration_state_mismatch(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let db_path = temp_path("sqlite-mismatch.sqlite3");
        let database_url = format!("sqlite://{}", db_path.display());
        let pool = DbPool::connect(&database_url, 1).await?;

        let sqlite = match &pool {
            DbPool::Sqlite(sqlite) => sqlite,
            DbPool::Postgres(_) => return Err("expected sqlite pool".into()),
        };

        sqlx::query("DROP TABLE IF EXISTS _sqlx_migrations")
            .execute(sqlite)
            .await?;
        sqlx::query(
            r#"
            CREATE TABLE _sqlx_migrations (
                version BIGINT PRIMARY KEY,
                description TEXT NOT NULL,
                installed_on TEXT NOT NULL,
                success BOOLEAN NOT NULL,
                checksum BLOB NOT NULL,
                execution_time BIGINT NOT NULL
            )
            "#,
        )
        .execute(sqlite)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO _sqlx_migrations (
                version,
                description,
                installed_on,
                success,
                checksum,
                execution_time
            )
            VALUES (1, 'init', CURRENT_TIMESTAMP, 1, x'00', 0)
            "#,
        )
        .execute(sqlite)
        .await?;

        let migrate_result = pool.migrate().await;
        assert!(
            migrate_result.is_err(),
            "expected sqlite migration failure on mismatched migration state"
        );
        let err_text = match migrate_result {
            Ok(()) => {
                return Err("sqlite migration mismatch should fail closed".into());
            }
            Err(err) => format!("{err:#}"),
        };
        assert!(
            err_text.contains("failed to run SQLite migrations")
                || err_text.contains("_sqlx_migrations")
                || err_text.contains("checksum"),
            "unexpected migration mismatch error: {err_text}"
        );

        cleanup_path(&db_path);
        Ok(())
    })
}

#[cfg(unix)]
#[test]
fn sqlite_connect_fails_closed_on_unwritable_path() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    run_async(async {
        let root = temp_path("sqlite-unwritable-root");
        fs::create_dir_all(&root)?;

        let readonly_dir = root.join("readonly");
        fs::create_dir_all(&readonly_dir)?;
        let mut perms = fs::metadata(&readonly_dir)?.permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&readonly_dir, perms)?;

        let database_url = format!(
            "sqlite://{}",
            readonly_dir.join("blocked.sqlite3").display()
        );
        let connect_result = DbPool::connect(&database_url, 1).await;
        assert!(
            connect_result.is_err(),
            "expected sqlite connect failure when db path parent is unwritable"
        );
        let err_text = match connect_result {
            Ok(_) => return Err("unwritable sqlite path should fail closed".into()),
            Err(err) => format!("{err:#}"),
        };
        assert!(
            err_text.contains("failed to connect to SQLite")
                || err_text.contains("unable to open database file")
                || err_text.contains("permission denied"),
            "unexpected sqlite unwritable-path error: {err_text}"
        );

        let mut restore_perms = fs::metadata(&readonly_dir)?.permissions();
        restore_perms.set_mode(0o755);
        fs::set_permissions(&readonly_dir, restore_perms)?;
        cleanup_path(&root);
        Ok(())
    })
}

fn temp_path(suffix: &str) -> PathBuf {
    env::temp_dir().join(format!("secureagnt_{}_{}", suffix, Uuid::new_v4()))
}

fn cleanup_path(path: &PathBuf) {
    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

fn run_async<F>(future: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>,
{
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(future)
}
