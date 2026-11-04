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

pub async fn tenant_transaction(
    pool: &PgPool,
    tenant: Uuid,
) -> Result<Transaction<'_, Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

/// Short transactional jobs: crashes roll back work and acknowledgement together.
/// Savepoints retain retry metadata; concurrent workers skip locked jobs.
pub async fn process_expiry_job(pool: &PgPool) -> Result<bool, sqlx::Error> {
    use sqlx::{Acquire, Row};
    let mut tx = pool.begin().await?;
    let Some(job) = sqlx::query("SELECT id,organization_id,session_id,attempts FROM background_jobs WHERE status='pending' AND available_at<=now() ORDER BY available_at,id FOR UPDATE SKIP LOCKED LIMIT 1")
        .fetch_optional(&mut *tx).await? else { tx.commit().await?; return Ok(false); };
    let id: Uuid = job.get("id");
    let org: Uuid = job.get("organization_id");
    let session: Uuid = job.get("session_id");
    let attempts: i32 = job.get("attempts");
    sqlx::query("SELECT set_config('app.tenant_id',$1,true)")
        .bind(org.to_string())
        .execute(&mut *tx)
        .await?;
    let mut work = tx.begin().await?;
    let result = async {
        let expired = sqlx::query("UPDATE interview_sessions SET state='expired' WHERE id=$1 AND state IN ('invited','ready','in_progress') AND least(expires_at,coalesce(deadline_at,expires_at))<=now()")
            .bind(session).execute(&mut *work).await?.rows_affected();
        if expired == 1 {
            sqlx::query("INSERT INTO audit_events(id,organization_id,entity_id,action,metadata) VALUES($1,$2,$3,'session.expired','{\"actor\":\"worker\"}')")
                .bind(Uuid::new_v4()).bind(org).bind(session).execute(&mut *work).await?;
        }
        Ok::<(),sqlx::Error>(())
    }.await;
    match result {
        Ok(()) => {
            work.commit().await?;
            sqlx::query("UPDATE background_jobs SET status='completed',attempts=attempts+1,last_error=NULL WHERE id=$1").bind(id).execute(&mut *tx).await?;
        }
        Err(error) => {
            tracing::error!(job_id=%id, %error, "session expiry failed");
            work.rollback().await?;
            sqlx::query("UPDATE background_jobs SET attempts=attempts+1,status=CASE WHEN attempts>=4 THEN 'failed' ELSE 'pending' END,available_at=now()+($2*interval '10 seconds'),last_error='Session expiry failed; inspect worker/database logs' WHERE id=$1")
                .bind(id).bind((attempts+1).min(5)).execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    Ok(true)
}
