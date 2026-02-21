use sqlx::{sqlite::SqlitePoolOptions, Row};

#[test]
fn sqlite_solo_lite_run_lifecycle_smoke() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;

        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await?;
        sqlx::migrate!("../migrations/sqlite").run(&pool).await?;

        let tenant_id = "solo";
        let agent_id = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        let user_id = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
        let run_id = "cccccccc-cccc-cccc-cccc-cccccccccccc";
        let step_id = "dddddddd-dddd-dddd-dddd-dddddddddddd";
        let audit_id = "eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee";

        sqlx::query(
            r#"
            INSERT INTO agents (id, tenant_id, name, status)
            VALUES (?1, ?2, ?3, 'active')
            "#,
        )
        .bind(agent_id)
        .bind(tenant_id)
        .bind("sqlite-agent")
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO users (id, tenant_id, external_subject, display_name, status)
            VALUES (?1, ?2, ?3, ?4, 'active')
            "#,
        )
        .bind(user_id)
        .bind(tenant_id)
        .bind("sqlite-user")
        .bind("SQLite User")
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO runs (
                id,
                tenant_id,
                agent_id,
                triggered_by_user_id,
                recipe_id,
                status,
                input_json,
                requested_capabilities,
                granted_capabilities,
                started_at
            )
            VALUES (?1, ?2, ?3, ?4, 'show_notes_v1', 'running', '{"text":"hello"}', '[]', '[]', CURRENT_TIMESTAMP)
            "#,
        )
        .bind(run_id)
        .bind(tenant_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&pool)
        .await?;

        sqlx::query(
            r##"
            INSERT INTO steps (id, run_id, tenant_id, agent_id, user_id, name, status, input_json, started_at)
            VALUES (?1, ?2, ?3, ?4, ?5, 'skill.invoke', 'running', '{"text":"hello"}', CURRENT_TIMESTAMP)
            "##,
        )
        .bind(step_id)
        .bind(run_id)
        .bind(tenant_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&pool)
        .await?;

        sqlx::query(
            r##"
            UPDATE steps
            SET status = 'succeeded',
                output_json = '{"markdown":"# Summary"}',
                finished_at = CURRENT_TIMESTAMP
            WHERE id = ?1
            "##,
        )
        .bind(step_id)
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            UPDATE runs
            SET status = 'succeeded',
                finished_at = CURRENT_TIMESTAMP
            WHERE id = ?1
            "#,
        )
        .bind(run_id)
        .execute(&pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO audit_events (
                id,
                run_id,
                step_id,
                tenant_id,
                agent_id,
                user_id,
                actor,
                event_type,
                payload_json
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'worker', 'run.succeeded', '{"ok":true}')
            "#,
        )
        .bind(audit_id)
        .bind(run_id)
        .bind(step_id)
        .bind(tenant_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&pool)
        .await?;

        let audit_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM audit_events
            WHERE tenant_id = ?1
              AND run_id = ?2
            "#,
        )
        .bind(tenant_id)
        .bind(run_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(audit_count, 1);

        let summary = sqlx::query(
            r#"
            SELECT
                SUM(CASE WHEN status = 'queued' THEN 1 ELSE 0 END) AS queued_runs,
                SUM(CASE WHEN status = 'running' THEN 1 ELSE 0 END) AS running_runs,
                SUM(CASE WHEN status = 'succeeded' THEN 1 ELSE 0 END) AS succeeded_runs_window,
                SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) AS failed_runs_window
            FROM runs
            WHERE tenant_id = ?1
            "#,
        )
        .bind(tenant_id)
        .fetch_one(&pool)
        .await?;

        assert_eq!(summary.get::<i64, _>("queued_runs"), 0);
        assert_eq!(summary.get::<i64, _>("running_runs"), 0);
        assert_eq!(summary.get::<i64, _>("succeeded_runs_window"), 1);
        assert_eq!(summary.get::<i64, _>("failed_runs_window"), 0);

        Ok(())
    })
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
