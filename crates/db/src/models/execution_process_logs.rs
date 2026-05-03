use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use ts_rs::TS;
use utils::log_msg::LogMsg;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendLogBatchOutcome {
    Appended,
    Truncated,
    AlreadyTruncated,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct ExecutionProcessLogs {
    pub execution_id: Uuid,
    pub logs: String, // JSONL format
    pub byte_size: i64,
    pub inserted_at: DateTime<Utc>,
}

impl ExecutionProcessLogs {
    /// Find logs by execution process ID
    pub async fn find_by_execution_id(
        pool: &SqlitePool,
        execution_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as!(
            ExecutionProcessLogs,
            r#"SELECT 
                execution_id as "execution_id!: Uuid",
                logs,
                byte_size,
                inserted_at as "inserted_at!: DateTime<Utc>"
               FROM execution_process_logs 
               WHERE execution_id = $1
               ORDER BY inserted_at ASC"#,
            execution_id
        )
        .fetch_all(pool)
        .await
    }

    /// Parse JSONL logs back into Vec<LogMsg>
    pub fn parse_logs(records: &[Self]) -> Result<Vec<LogMsg>, serde_json::Error> {
        let mut messages = Vec::new();
        for line in records.iter().flat_map(|record| record.logs.lines()) {
            if !line.trim().is_empty() {
                let msg: LogMsg = serde_json::from_str(line)?;
                messages.push(msg);
            }
        }
        Ok(messages)
    }

    /// Append JSONL logs for an execution process while enforcing its byte cap.
    ///
    /// The execution_processes row is touched inside the transaction before
    /// reading the counters so concurrent writers serialize on SQLite's write
    /// lock and cannot race past the cap.
    pub async fn append_log_batch_capped(
        pool: &SqlitePool,
        execution_id: Uuid,
        jsonl_batch: &str,
        max_bytes: i64,
    ) -> Result<AppendLogBatchOutcome, sqlx::Error> {
        if jsonl_batch.is_empty() {
            return Ok(AppendLogBatchOutcome::Appended);
        }

        let max_bytes = max_bytes.max(0);
        let mut tx = pool.begin().await?;

        sqlx::query(
            r#"UPDATE execution_processes
               SET log_bytes_written = log_bytes_written
               WHERE id = ?"#,
        )
        .bind(execution_id)
        .execute(&mut *tx)
        .await?;

        let Some((bytes_written, log_truncated)) = sqlx::query_as::<_, (i64, bool)>(
            r#"SELECT log_bytes_written, log_truncated
                   FROM execution_processes
                   WHERE id = ?"#,
        )
        .bind(execution_id)
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Err(sqlx::Error::RowNotFound);
        };

        if log_truncated {
            tx.commit().await?;
            return Ok(AppendLogBatchOutcome::AlreadyTruncated);
        }

        let batch_bytes = jsonl_batch.len() as i64;
        let remaining_bytes = max_bytes.saturating_sub(bytes_written);

        if batch_bytes <= remaining_bytes {
            Self::insert_batch(&mut tx, execution_id, jsonl_batch, batch_bytes).await?;
            sqlx::query(
                r#"UPDATE execution_processes
                   SET log_bytes_written = log_bytes_written + ?
                   WHERE id = ?"#,
            )
            .bind(batch_bytes)
            .bind(execution_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(AppendLogBatchOutcome::Appended);
        }

        let marker = truncation_marker(max_bytes);
        let capped_batch = capped_batch_with_marker(jsonl_batch, remaining_bytes, &marker);
        let capped_bytes = capped_batch.len() as i64;

        if !capped_batch.is_empty() {
            Self::insert_batch(&mut tx, execution_id, &capped_batch, capped_bytes).await?;
        }

        sqlx::query(
            r#"UPDATE execution_processes
               SET log_bytes_written = log_bytes_written + ?,
                   log_truncated = TRUE
               WHERE id = ?"#,
        )
        .bind(capped_bytes)
        .bind(execution_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(AppendLogBatchOutcome::Truncated)
    }

    async fn insert_batch(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        execution_id: Uuid,
        jsonl_batch: &str,
        byte_size: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"INSERT INTO execution_process_logs (execution_id, logs, byte_size, inserted_at)
               VALUES (?, ?, ?, datetime('now', 'subsec'))"#,
        )
        .bind(execution_id)
        .bind(jsonl_batch)
        .bind(byte_size)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}

fn truncation_marker(max_bytes: i64) -> String {
    let msg = LogMsg::Stderr(format!("Logs truncated at {max_bytes} bytes."));
    match serde_json::to_string(&msg) {
        Ok(json) => format!("{json}\n"),
        Err(_) => format!("{{\"stderr\":\"Logs truncated at {max_bytes} bytes.\"}}\n"),
    }
}

fn capped_batch_with_marker(batch: &str, remaining_bytes: i64, marker: &str) -> String {
    if remaining_bytes <= 0 {
        return String::new();
    }

    let marker_bytes = marker.len() as i64;
    if remaining_bytes < marker_bytes {
        return String::new();
    }

    let payload_budget = remaining_bytes - marker_bytes;
    let mut capped = String::new();
    let mut used = 0_i64;

    for line in batch.split_inclusive('\n') {
        let line_bytes = line.len() as i64;
        if used + line_bytes > payload_budget {
            break;
        }
        capped.push_str(line);
        used += line_bytes;
    }

    capped.push_str(marker);
    capped
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");

        sqlx::query(
            r#"CREATE TABLE execution_processes (
                id BLOB PRIMARY KEY,
                log_bytes_written INTEGER NOT NULL DEFAULT 0,
                log_truncated BOOLEAN NOT NULL DEFAULT FALSE
            )"#,
        )
        .execute(&pool)
        .await
        .expect("create execution_processes");

        sqlx::query(
            r#"CREATE TABLE execution_process_logs (
                execution_id BLOB NOT NULL,
                logs TEXT NOT NULL,
                byte_size INTEGER NOT NULL,
                inserted_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
                FOREIGN KEY (execution_id) REFERENCES execution_processes(id) ON DELETE CASCADE
            )"#,
        )
        .execute(&pool)
        .await
        .expect("create execution_process_logs");

        pool
    }

    async fn create_execution(pool: &SqlitePool) -> Uuid {
        let execution_id = Uuid::new_v4();
        sqlx::query("INSERT INTO execution_processes (id) VALUES (?)")
            .bind(execution_id)
            .execute(pool)
            .await
            .expect("insert execution process");
        execution_id
    }

    async fn process_log_state(pool: &SqlitePool, execution_id: Uuid) -> (i64, bool, String) {
        let (bytes_written, truncated) = sqlx::query_as::<_, (i64, bool)>(
            "SELECT log_bytes_written, log_truncated FROM execution_processes WHERE id = ?",
        )
        .bind(execution_id)
        .fetch_one(pool)
        .await
        .expect("fetch log state");

        let logs: Option<String> = sqlx::query_scalar(
            "SELECT group_concat(logs, '') FROM execution_process_logs WHERE execution_id = ?",
        )
        .bind(execution_id)
        .fetch_one(pool)
        .await
        .expect("fetch logs");

        (bytes_written, truncated, logs.unwrap_or_default())
    }

    #[tokio::test]
    async fn append_under_cap_updates_bytes_without_truncating() {
        let pool = test_pool().await;
        let execution_id = create_execution(&pool).await;
        let batch = "{\"Stdout\":\"hello\"}\n";

        let outcome =
            ExecutionProcessLogs::append_log_batch_capped(&pool, execution_id, batch, 1024)
                .await
                .expect("append logs");

        let (bytes_written, truncated, logs) = process_log_state(&pool, execution_id).await;
        assert_eq!(outcome, AppendLogBatchOutcome::Appended);
        assert_eq!(bytes_written, batch.len() as i64);
        assert!(!truncated);
        assert_eq!(logs, batch);
    }

    #[tokio::test]
    async fn append_crossing_cap_writes_marker_once() {
        let pool = test_pool().await;
        let execution_id = create_execution(&pool).await;
        let first = "{\"Stdout\":\"one\"}\n";
        let second = "{\"Stdout\":\"two\"}\n";
        let filler = "{\"Stdout\":\"three\"}\n".repeat(20);
        let mut max_bytes = (first.len() + truncation_marker(0).len()) as i64;
        loop {
            let needed = (first.len() + truncation_marker(max_bytes).len()) as i64;
            if needed == max_bytes {
                break;
            }
            max_bytes = needed;
        }
        let marker = truncation_marker(max_bytes);
        let batch = format!("{first}{second}{filler}");

        let outcome =
            ExecutionProcessLogs::append_log_batch_capped(&pool, execution_id, &batch, max_bytes)
                .await
                .expect("append logs");

        let (bytes_written, truncated, logs) = process_log_state(&pool, execution_id).await;
        assert_eq!(outcome, AppendLogBatchOutcome::Truncated);
        assert!(bytes_written <= max_bytes);
        assert!(truncated);
        assert_eq!(logs, format!("{first}{marker}"));
    }

    #[tokio::test]
    async fn append_after_truncation_is_noop() {
        let pool = test_pool().await;
        let execution_id = create_execution(&pool).await;
        let max_bytes = truncation_marker(0).len() as i64;

        ExecutionProcessLogs::append_log_batch_capped(
            &pool,
            execution_id,
            &"{\"Stdout\":\"too large\"}\n".repeat(20),
            max_bytes,
        )
        .await
        .expect("truncate logs");
        let before = process_log_state(&pool, execution_id).await;

        let outcome = ExecutionProcessLogs::append_log_batch_capped(
            &pool,
            execution_id,
            "{\"Stdout\":\"ignored\"}\n",
            max_bytes,
        )
        .await
        .expect("append after truncation");
        let after = process_log_state(&pool, execution_id).await;

        assert_eq!(outcome, AppendLogBatchOutcome::AlreadyTruncated);
        assert_eq!(before, after);
    }
}
