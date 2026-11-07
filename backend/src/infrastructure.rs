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
    use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};
    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let logs = tracing_subscriber::fmt::layer()
        .json()
        .with_filter(filter);
    let subscriber = tracing_subscriber::registry();
    #[cfg(feature = "tokio-console")]
    let subscriber = subscriber.with(
        console_subscriber::ConsoleLayer::builder()
            .server_addr(([127, 0, 0, 1], 6669))
            .spawn(),
    );
    subscriber.with(logs).init();
    #[cfg(feature = "tokio-console")]
    tracing::info!(
        address = "127.0.0.1:6669",
        "Tokio Console telemetry enabled"
    );
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

// Preserve existing callers while domain expiry lives in the service layer.
pub use crate::services::expiry_service::{finalize_due_session, process_expiry_job};
