use sqlx::{PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use uuid::Uuid;

pub fn load_environment() -> anyhow::Result<()> {
    match dotenvy::dotenv() {
        Ok(_) => Ok(()),
        Err(error) if error.not_found() => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .json()
        .init();
}

pub async fn connect() -> anyhow::Result<PgPool> {
    let url = std::env::var("RUNTIME_DATABASE_URL").or_else(|_| std::env::var("DATABASE_URL"))?;
    connect_url(&url).await
}

pub async fn connect_for_migrations() -> anyhow::Result<PgPool> {
    let url = std::env::var("MIGRATION_DATABASE_URL").or_else(|_| std::env::var("DATABASE_URL"))?;
    connect_url(&url).await
}

async fn connect_url(url: &str) -> anyhow::Result<PgPool> {
    Ok(PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(url)
        .await?)
}

pub async fn interviewer_transaction(
    pool: &PgPool,
    tenant: Uuid,
) -> Result<Transaction<'_, Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.interviewer_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

/// Short transactional jobs: crashes roll back work and acknowledgement together.
/// Savepoints retain retry metadata; concurrent workers skip locked jobs.
pub async fn process_expiry_job(pool: &PgPool) -> Result<bool, sqlx::Error> {
    let interviewers: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE role='interviewer' ORDER BY id")
            .fetch_all(pool)
            .await?;
    for interviewer in interviewers {
        let mut tx = interviewer_transaction(pool, interviewer).await?;
        let session: Option<Uuid> = sqlx::query_scalar("SELECT id FROM interview_sessions WHERE state IN ('invited','ready','in_progress') AND least(expires_at,coalesce(deadline_at,expires_at))<=now() ORDER BY expires_at,id FOR UPDATE SKIP LOCKED LIMIT 1")
            .fetch_optional(&mut *tx).await?;
        if let Some(session) = session {
            finalize_due_session(&mut tx, interviewer, session).await?;
            tx.commit().await?;
            return Ok(true);
        }
        tx.commit().await?;
    }
    Ok(false)
}

/// Finalize once under a row lock; late answer writes cannot race submission.
pub async fn finalize_due_session(
    tx: &mut Transaction<'_, Postgres>,
    interviewer: Uuid,
    session: Uuid,
) -> Result<(), sqlx::Error> {
    use sqlx::Row;
    sqlx::query("SELECT id FROM interview_sessions WHERE id=$1 FOR UPDATE")
        .bind(session)
        .fetch_optional(&mut **tx)
        .await?;
    let row=sqlx::query("UPDATE interview_sessions SET submitted_at=CASE WHEN state='in_progress' THEN least(expires_at,coalesce(deadline_at,expires_at)) ELSE submitted_at END,state=CASE WHEN state='in_progress' THEN 'submitted' ELSE 'expired' END WHERE id=$1 AND state IN ('invited','ready','in_progress') AND least(expires_at,coalesce(deadline_at,expires_at))<=clock_timestamp() RETURNING state")
        .bind(session).fetch_optional(&mut **tx).await?;
    if let Some(row) = row {
        let state: String = row.get("state");
        sqlx::query("INSERT INTO audit_events(id,interviewer_id,entity_id,action,metadata) VALUES($1,$2,$3,$4,'{\"actor\":\"system\",\"reason\":\"deadline\"}')")
            .bind(Uuid::new_v4()).bind(interviewer).bind(session).bind(if state=="submitted" {"session.timed_out"} else {"session.expired"}).execute(&mut **tx).await?;
    }
    Ok(())
}
