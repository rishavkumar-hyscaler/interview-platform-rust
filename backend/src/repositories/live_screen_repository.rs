use crate::{Result, error};
use uuid::Uuid;
pub(crate) async fn authorize(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner: Uuid,
    session: Uuid,
) -> Result<()> {
    let active: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM interview_sessions
            WHERE id = $1
                AND interviewer_id = $2
                AND state = 'in_progress'
                AND deadline_at > now()
        )
        "#,
    )
    .bind(session)
    .bind(owner)
    .fetch_one(&mut **tx)
    .await?;
    if !active {
        return Err(error::conflict(
            "Live screen is available only during an active interview",
        ));
    }
    Ok(())
}
