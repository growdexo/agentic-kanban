use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StartupRecoverySummary {
    pub id: Uuid,
    pub recovered_at: DateTime<Utc>,
    pub running_found: i64,
    pub reattached_count: i64,
    pub orphaned_count: i64,
    pub reattached_execution_process_ids: String,
    pub orphaned_execution_process_ids: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct CreateStartupRecoverySummary {
    pub running_found: i64,
    pub reattached_count: i64,
    pub orphaned_count: i64,
    pub reattached_execution_process_ids: Vec<Uuid>,
    pub orphaned_execution_process_ids: Vec<Uuid>,
}

impl StartupRecoverySummary {
    pub async fn create(
        pool: &SqlitePool,
        data: &CreateStartupRecoverySummary,
    ) -> Result<Self, sqlx::Error> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let reattached_execution_process_ids =
            serde_json::to_string(&data.reattached_execution_process_ids)
                .map_err(|err| sqlx::Error::Encode(Box::new(err)))?;
        let orphaned_execution_process_ids =
            serde_json::to_string(&data.orphaned_execution_process_ids)
                .map_err(|err| sqlx::Error::Encode(Box::new(err)))?;

        sqlx::query!(
            r#"INSERT INTO startup_recovery_summaries (
                    id,
                    recovered_at,
                    running_found,
                    reattached_count,
                    orphaned_count,
                    reattached_execution_process_ids,
                    orphaned_execution_process_ids,
                    created_at,
                    updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
            id,
            now,
            data.running_found,
            data.reattached_count,
            data.orphaned_count,
            reattached_execution_process_ids,
            orphaned_execution_process_ids,
            now,
            now
        )
        .execute(pool)
        .await?;

        sqlx::query_as!(
            StartupRecoverySummary,
            r#"SELECT
                    id as "id!: Uuid",
                    recovered_at as "recovered_at!: DateTime<Utc>",
                    running_found as "running_found!: i64",
                    reattached_count as "reattached_count!: i64",
                    orphaned_count as "orphaned_count!: i64",
                    reattached_execution_process_ids,
                    orphaned_execution_process_ids,
                    created_at as "created_at!: DateTime<Utc>",
                    updated_at as "updated_at!: DateTime<Utc>"
               FROM startup_recovery_summaries
               WHERE id = ?"#,
            id
        )
        .fetch_one(pool)
        .await
    }
}
