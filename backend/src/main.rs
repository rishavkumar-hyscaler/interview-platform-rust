#[tokio::main]
async fn main() -> anyhow::Result<()> {
    platform_api::infrastructure::load_environment()?;
    platform_api::infrastructure::init_tracing();
    if std::env::args().any(|arg| arg == "--migrate") {
        let pool = platform_api::infrastructure::connect_for_migrations().await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        return Ok(());
    }
    platform_api::invitations::InvitationCipher::from_environment().map_err(|_| {
        anyhow::anyhow!("Set INVITATION_ENCRYPTION_KEY to a stable 64-character hex key in .env")
    })?;
    let pool = platform_api::infrastructure::connect().await?;
    let unsafe_role: bool = sqlx::query_scalar(
        "SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname=current_user",
    )
    .fetch_one(&pool)
    .await?;
    anyhow::ensure!(
        !unsafe_role,
        "API must use a non-superuser role without BYPASSRLS"
    );
    let address = std::env::var("BIND_ADDRESS").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    tracing::info!(%address, "API listening");
    let origin: axum::http::HeaderValue = std::env::var("FRONTEND_ORIGIN")
        .unwrap_or_else(|_| "http://localhost:5173".into())
        .parse()?;
    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(origin)
        .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderName::from_static("x-request-id"),
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
    let signal = async move {
        tokio::select! {
            _ = shutdown_signal() => { let _ = shutdown.send(true); }
            _ = shutdown.closed() => {}
        }
    };
    // Both futures run in this executable; no separate worker build/process.
    let (result, (), ()) = tokio::join!(server, worker, signal);
    pool.close().await;
    result?;
    Ok(())
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
