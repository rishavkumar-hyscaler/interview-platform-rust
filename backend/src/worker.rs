//! In-process background jobs sharing the API's pool and shutdown signal.
use sqlx::PgPool;
use tokio::sync::watch;

pub async fn run(pool: PgPool, shutdown: watch::Receiver<bool>) {
    let generation_pool = pool.clone();
    let generation_shutdown = shutdown.clone();
    tokio::join!(
        expiry_loop(pool, shutdown),
        generation_loop(generation_pool, generation_shutdown)
    );
}
async fn generation_loop(pool: PgPool, mut shutdown: watch::Receiver<bool>) {
    let provider = crate::generation::provider::AiProvider::from_environment().ok();
    if provider.is_none() {
        tracing::info!("AI generation not configured; expiry jobs remain active");
    }
    loop {
        if *shutdown.borrow() {
            break;
        }
        if let Some(provider) = &provider {
            tokio::select! {
                _ = shutdown.changed() => break,
                result = crate::generation::process_next(&pool, provider) => match result {
                    Ok(true) => continue,
                    Ok(false) => {},
                    Err(_) => tracing::error!("question generation polling failed"),
                }
            }
        }
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {},
        }
    }
}
async fn expiry_loop(pool: PgPool, mut shutdown: watch::Receiver<bool>) {
    tracing::info!("background jobs ready; processing durable session expiry jobs");
    loop {
        if *shutdown.borrow() {
            break;
        }
        // Complete the short job transaction before observing shutdown. Never
        // acknowledge a job whose domain transaction has not committed.
        let processed = match crate::infrastructure::process_expiry_job(&pool).await {
            Ok(processed) => processed,
            Err(error) => {
                tracing::error!(%error, "expiry job polling failed");
                false
            }
        };
        if processed {
            continue;
        }
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {},
        }
    }
    tracing::info!("background jobs stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn already_stopped_worker_does_not_access_database() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused@localhost/unused")
            .unwrap();
        let (_sender, receiver) = watch::channel(true);
        tokio::time::timeout(std::time::Duration::from_secs(1), run(pool, receiver))
            .await
            .unwrap();
    }
}
