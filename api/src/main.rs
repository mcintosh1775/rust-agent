use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use std::env;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let bind_addr = env::var("API_BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let database_url = env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let run_migrations = env::var("API_RUN_MIGRATIONS")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&database_url)
        .await
        .context("failed to connect api to Postgres")?;

    if run_migrations {
        sqlx::migrate!("../migrations")
            .run(&pool)
            .await
            .context("failed to run api migrations")?;
        info!("api migrations applied");
    }

    let listener = TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("failed binding API listener to {bind_addr}"))?;

    info!(bind = %bind_addr, "api started");

    axum::serve(listener, api::app_router(pool))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("api server error")?;

    Ok(())
}

async fn shutdown_signal() {
    if tokio::signal::ctrl_c().await.is_ok() {
        info!("api shutdown signal received");
    }
}

fn init_tracing() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
