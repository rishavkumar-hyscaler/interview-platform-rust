use std::{future::Future, time::Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    platform_api::infrastructure::load_environment()?;
    platform_api::infrastructure::init_tracing();
    if std::env::args().any(|arg| arg == "--migrate") {
        let pool = platform_api::infrastructure::connect_for_migrations().await?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await?;
        return Ok(());
    }
    platform_api::invitations::InvitationCipher::from_environment().map_err(|_| {
        anyhow::anyhow!("Set INVITATION_ENCRYPTION_KEY to a stable 64-character hex key in .env")
    })?;
    let pool = platform_api::infrastructure::connect().await?;
    let unsafe_role: bool = sqlx::query_scalar(
        r#"
        SELECT rolsuper
        OR rolbypassrls
        FROM pg_roles
        WHERE rolname = current_user
        "#,
    )
    .fetch_one(&pool)
    .await?;
    anyhow::ensure!(
        !unsafe_role,
        "API must use a non-superuser role without BYPASSRLS"
    );
    let address = match std::env::var("BIND_ADDRESS") {
        Ok(value) => value,
        Err(_) => "127.0.0.1:8080".to_string(),
    };
    let listener = tokio::net::TcpListener::bind(&address).await?;
    tracing::info!(%address, "API listening");
    let origins = match std::env::var("FRONTEND_ORIGIN") {
        Ok(value) => value,
        Err(_) => "http://localhost:5173".to_string(),
    };
    let origins = origins
        .split(',')
        .map(|origin| origin.trim().parse::<axum::http::HeaderValue>())
        .collect::<Result<Vec<_>, _>>()?;
    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderName::from_static("x-request-id"),
            axum::http::HeaderName::from_static("x-evidence-source"),
            axum::http::HeaderName::from_static("x-evidence-capture"),
            axum::http::HeaderName::from_static("x-capture-surface"),
            axum::http::HeaderName::from_static("x-evidence-captured-at"),
        ])
        .expose_headers([axum::http::HeaderName::from_static("x-request-id")]);
    let (shutdown, receiver) = tokio::sync::watch::channel(false);
    let worker = platform_api::worker::run(pool.clone(), receiver.clone());
    let server_shutdown = shutdown.clone();
    let mut server_receiver = receiver;
    let server = axum::serve(listener, platform_api::router(pool.clone()).layer(cors))
        .with_graceful_shutdown(async move {
            let _ = server_receiver.changed().await;
        });
    let server = async move {
        let result = server.await;
        // Also stop background jobs if the HTTP server fails.
        let _ = server_shutdown.send(true);
        result
    };
    // Keep shutdown bounded: live SSE connections can otherwise drain forever.
    let result = {
        let services = async {
            let (result, ()) = tokio::join!(server, worker);
            result
        };
        tokio::pin!(services);
        tokio::select! {
            result = &mut services => result,
            _ = shutdown_signal() => {
                tracing::info!("Shutdown requested; press Ctrl+C again to exit immediately");
                let _ = shutdown.send(true);
                finish_shutdown(&mut services, shutdown_signal(), Duration::from_secs(5)).await
            }
        }
    };
    // A stuck request/task may still hold a connection. Do not wait forever here either.
    if tokio::time::timeout(Duration::from_secs(1), pool.close())
        .await
        .is_err()
    {
        tracing::warn!("Database cleanup timed out; exiting");
    }
    result?;
    Ok(())
}

async fn finish_shutdown(
    services: impl Future<Output = std::io::Result<()>>,
    force: impl Future<Output = ()>,
    grace: Duration,
) -> std::io::Result<()> {
    tokio::select! {
        result = services => result,
        _ = force => {
            tracing::warn!("Second shutdown signal received; exiting immediately");
            Ok(())
        }
        _ = tokio::time::sleep(grace) => {
            tracing::warn!("Shutdown grace period expired; closing remaining connections");
            Ok(())
        }
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::{pending, ready};

    #[tokio::test]
    async fn open_stream_cannot_block_shutdown_indefinitely() {
        tokio::time::timeout(
            Duration::from_secs(1),
            finish_shutdown(pending(), pending(), Duration::from_millis(10)),
        )
        .await
        .unwrap()
        .unwrap();
    }

    #[tokio::test]
    async fn second_signal_skips_the_grace_period() {
        tokio::time::timeout(
            Duration::from_secs(1),
            finish_shutdown(pending(), ready(()), Duration::from_secs(60)),
        )
        .await
        .unwrap()
        .unwrap();
    }

    #[tokio::test]
    async fn completed_server_preserves_its_error() {
        let result = finish_shutdown(
            ready(Err(std::io::Error::other("server failed"))),
            pending(),
            Duration::from_secs(1),
        )
        .await;
        assert!(result.is_err());
    }
}
