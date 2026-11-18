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
        .compact()
        .with_ansi(false)
        .with_target(false)
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
    let url = std::env::var("DATABASE_URL")?;
    connect_url(&url).await
}

async fn connect_url(url: &str) -> anyhow::Result<PgPool> {
    let max_connections = positive_setting("DB_MAX_CONNECTIONS", 10)?;
    let acquire_timeout = positive_setting("DB_ACQUIRE_TIMEOUT_SECONDS", 5)?;
    Ok(PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(std::time::Duration::from_secs(u64::from(acquire_timeout)))
        .connect(url)
        .await?)
}

fn positive_setting(name: &str, default: u32) -> anyhow::Result<u32> {
    let value = match std::env::var(name) {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => return Ok(default),
        Err(_) => anyhow::bail!("{name} must contain a positive integer"),
    };
    match value.parse::<u32>() {
        Ok(number) if number > 0 => Ok(number),
        _ => anyhow::bail!("{name} must contain a positive integer"),
    }
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
