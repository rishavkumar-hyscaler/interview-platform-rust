//! Explicit additive upgrade for this development-only consolidated baseline.
use sha2::{Digest, Sha384};

pub async fn upgrade_rag() -> anyhow::Result<()> {
    let pool = crate::infrastructure::connect_for_migrations().await?;
    let baseline = include_str!("../migrations/202611040001_initial_schema.sql");
    let checksum = Sha384::digest(baseline.as_bytes()).to_vec();
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(202611040001)")
        .execute(&mut *tx)
        .await?;
    let rows: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        r#"
        SELECT version,
            checksum
        FROM _sqlx_migrations
        WHERE success = true
        ORDER BY version
        "#,
    )
    .fetch_all(&mut *tx)
    .await?;
    anyhow::ensure!(
        rows.len() == 1 && rows[0].0 == 202611040001,
        "Expected the single development baseline; use --migrate for a fresh database"
    );
    if rows[0].1 == checksum {
        tx.commit().await?;
        tracing::info!("RAG schema already installed");
        return Ok(());
    }
    let previous = hex::decode(
        "11a847f0f36fe02ad4d1e2d532a99c8a6642d6a328b4e8b374a9f5423222f77ee3ae505575feb0a3c3b160c3b9ddfd33",
    )?;
    anyhow::ensure!(
        rows[0].1 == previous,
        "Database baseline differs; refusing to rewrite its migration history"
    );
    let (_, ddl) = baseline
        .split_once(
            "-- Persistent post-scoring evidence retrieval (development baseline extension).",
        )
        .ok_or_else(|| anyhow::anyhow!("RAG schema section missing"))?;
    sqlx::raw_sql(ddl)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        r#"
        UPDATE _sqlx_migrations
        SET checksum = $1
        WHERE version = 202611040001
        "#,
    )
    .bind(checksum)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    tracing::info!("Added evidence_embeddings and embedding_jobs; existing records preserved");
    Ok(())
}
