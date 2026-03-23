//! Usage tracking store — records LLM usage events for cost monitoring.

use crate::db::{block_on, SharedDb};
use chrono::{Datelike, Utc};
use silicrew_types::agent::AgentId;
use silicrew_types::error::{SiliCrewError, SiliCrewResult};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::sync::Arc;

/// A single usage event recording an LLM call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    /// Which agent made the call.
    pub agent_id: AgentId,
    /// Model used.
    pub model: String,
    /// Input tokens consumed.
    pub input_tokens: u64,
    /// Output tokens consumed.
    pub output_tokens: u64,
    /// Estimated cost in USD.
    pub cost_usd: f64,
    /// Number of tool calls in this interaction.
    pub tool_calls: u32,
}

/// Summary of usage over a period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSummary {
    /// Total input tokens.
    pub total_input_tokens: u64,
    /// Total output tokens.
    pub total_output_tokens: u64,
    /// Total estimated cost in USD.
    pub total_cost_usd: f64,
    /// Total number of calls.
    pub call_count: u64,
    /// Total tool calls.
    pub total_tool_calls: u64,
}

/// Usage grouped by model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    /// Model name.
    pub model: String,
    /// Total cost for this model.
    pub total_cost_usd: f64,
    /// Total input tokens.
    pub total_input_tokens: u64,
    /// Total output tokens.
    pub total_output_tokens: u64,
    /// Number of calls.
    pub call_count: u64,
}

/// Daily usage breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyBreakdown {
    /// Date string (YYYY-MM-DD).
    pub date: String,
    /// Total cost for this day.
    pub cost_usd: f64,
    /// Total tokens (input + output).
    pub tokens: u64,
    /// Number of API calls.
    pub calls: u64,
}

/// Usage store backed by the shared SQL database.
#[derive(Clone)]
pub struct UsageStore {
    db: SharedDb,
}

impl UsageStore {
    /// Create a new usage store wrapping the given connection.
    pub fn new(db: impl Into<SharedDb>) -> Self {
        Self { db: db.into() }
    }

    /// Record a usage event.
    pub fn record(&self, record: &UsageRecord) -> SiliCrewResult<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO usage_events (id, agent_id, timestamp, model, input_tokens, output_tokens, cost_usd, tool_calls)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        id,
                        record.agent_id.0.to_string(),
                        now,
                        record.model,
                        record.input_tokens as i64,
                        record.output_tokens as i64,
                        record.cost_usd,
                        record.tool_calls as i64,
                    ],
                )
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let record = record.clone();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO usage_events (id, agent_id, timestamp, model, input_tokens, output_tokens, cost_usd, tool_calls)
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                    )
                    .bind(id)
                    .bind(record.agent_id.0.to_string())
                    .bind(now)
                    .bind(record.model)
                    .bind(record.input_tokens as i64)
                    .bind(record.output_tokens as i64)
                    .bind(record.cost_usd)
                    .bind(record.tool_calls as i64)
                    .execute(&*pool)
                    .await
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// Query total cost in the last hour for an agent.
    pub fn query_hourly(&self, agent_id: AgentId) -> SiliCrewResult<f64> {
        let cutoff = (Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
        self.query_cost_since(Some(agent_id), &cutoff)
    }

    /// Query total cost today for an agent.
    pub fn query_daily(&self, agent_id: AgentId) -> SiliCrewResult<f64> {
        let cutoff = Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .expect("midnight")
            .and_utc()
            .to_rfc3339();
        self.query_cost_since(Some(agent_id), &cutoff)
    }

    /// Query total cost in the current calendar month for an agent.
    pub fn query_monthly(&self, agent_id: AgentId) -> SiliCrewResult<f64> {
        let now = Utc::now();
        let cutoff = now
            .date_naive()
            .with_day(1)
            .expect("first day")
            .and_hms_opt(0, 0, 0)
            .expect("midnight")
            .and_utc()
            .to_rfc3339();
        self.query_cost_since(Some(agent_id), &cutoff)
    }

    /// Query total cost across all agents for the current hour.
    pub fn query_global_hourly(&self) -> SiliCrewResult<f64> {
        let cutoff = (Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
        self.query_cost_since(None, &cutoff)
    }

    /// Query total cost across all agents for the current calendar month.
    pub fn query_global_monthly(&self) -> SiliCrewResult<f64> {
        let now = Utc::now();
        let cutoff = now
            .date_naive()
            .with_day(1)
            .expect("first day")
            .and_hms_opt(0, 0, 0)
            .expect("midnight")
            .and_utc()
            .to_rfc3339();
        self.query_cost_since(None, &cutoff)
    }

    /// Query usage summary, optionally filtered by agent.
    pub fn query_summary(&self, agent_id: Option<AgentId>) -> SiliCrewResult<UsageSummary> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;

                let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match agent_id {
                    Some(aid) => (
                        "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                                COALESCE(SUM(cost_usd), 0.0), COUNT(*), COALESCE(SUM(tool_calls), 0)
                         FROM usage_events WHERE agent_id = ?1",
                        vec![Box::new(aid.0.to_string())],
                    ),
                    None => (
                        "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                                COALESCE(SUM(cost_usd), 0.0), COUNT(*), COALESCE(SUM(tool_calls), 0)
                         FROM usage_events",
                        vec![],
                    ),
                };

                let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();

                conn.query_row(sql, params_refs.as_slice(), |row| {
                    Ok(UsageSummary {
                        total_input_tokens: row.get::<_, i64>(0)? as u64,
                        total_output_tokens: row.get::<_, i64>(1)? as u64,
                        total_cost_usd: row.get(2)?,
                        call_count: row.get::<_, i64>(3)? as u64,
                        total_tool_calls: row.get::<_, i64>(4)? as u64,
                    })
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                block_on(async move {
                    let row = if let Some(agent_id) = agent_id {
                        sqlx::query(
                            "SELECT COALESCE(SUM(input_tokens), 0) AS total_input_tokens,
                                    COALESCE(SUM(output_tokens), 0) AS total_output_tokens,
                                    COALESCE(SUM(cost_usd), 0.0) AS total_cost_usd,
                                    COUNT(*) AS call_count,
                                    COALESCE(SUM(tool_calls), 0) AS total_tool_calls
                             FROM usage_events WHERE agent_id = $1",
                        )
                        .bind(agent_id.0.to_string())
                        .fetch_one(&*pool)
                        .await?
                    } else {
                        sqlx::query(
                            "SELECT COALESCE(SUM(input_tokens), 0) AS total_input_tokens,
                                    COALESCE(SUM(output_tokens), 0) AS total_output_tokens,
                                    COALESCE(SUM(cost_usd), 0.0) AS total_cost_usd,
                                    COUNT(*) AS call_count,
                                    COALESCE(SUM(tool_calls), 0) AS total_tool_calls
                             FROM usage_events",
                        )
                        .fetch_one(&*pool)
                        .await?
                    };
                    Ok::<UsageSummary, sqlx::Error>(UsageSummary {
                        total_input_tokens: row.try_get::<i64, _>("total_input_tokens")? as u64,
                        total_output_tokens: row.try_get::<i64, _>("total_output_tokens")? as u64,
                        total_cost_usd: row.try_get("total_cost_usd")?,
                        call_count: row.try_get::<i64, _>("call_count")? as u64,
                        total_tool_calls: row.try_get::<i64, _>("total_tool_calls")? as u64,
                    })
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))
            }
        }
    }

    /// Query usage grouped by model.
    pub fn query_by_model(&self) -> SiliCrewResult<Vec<ModelUsage>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;

                let mut stmt = conn
                    .prepare(
                        "SELECT model, COALESCE(SUM(cost_usd), 0.0), COALESCE(SUM(input_tokens), 0),
                                COALESCE(SUM(output_tokens), 0), COUNT(*)
                         FROM usage_events GROUP BY model ORDER BY SUM(cost_usd) DESC",
                    )
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;

                let rows = stmt
                    .query_map([], |row| {
                        Ok(ModelUsage {
                            model: row.get(0)?,
                            total_cost_usd: row.get(1)?,
                            total_input_tokens: row.get::<_, i64>(2)? as u64,
                            total_output_tokens: row.get::<_, i64>(3)? as u64,
                            call_count: row.get::<_, i64>(4)? as u64,
                        })
                    })
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;

                let mut results = Vec::new();
                for row in rows {
                    results.push(row.map_err(|e| SiliCrewError::Memory(e.to_string()))?);
                }
                Ok(results)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                block_on(async move {
                    let rows = sqlx::query(
                        "SELECT model,
                                COALESCE(SUM(cost_usd), 0.0) AS total_cost_usd,
                                COALESCE(SUM(input_tokens), 0) AS total_input_tokens,
                                COALESCE(SUM(output_tokens), 0) AS total_output_tokens,
                                COUNT(*) AS call_count
                         FROM usage_events
                         GROUP BY model
                         ORDER BY total_cost_usd DESC",
                    )
                    .fetch_all(&*pool)
                    .await?;
                    Ok::<Vec<ModelUsage>, sqlx::Error>(
                        rows.into_iter()
                            .map(|row| ModelUsage {
                                model: row.try_get("model").unwrap_or_default(),
                                total_cost_usd: row.try_get("total_cost_usd").unwrap_or(0.0),
                                total_input_tokens: row
                                    .try_get::<i64, _>("total_input_tokens")
                                    .unwrap_or_default()
                                    as u64,
                                total_output_tokens: row
                                    .try_get::<i64, _>("total_output_tokens")
                                    .unwrap_or_default()
                                    as u64,
                                call_count: row.try_get::<i64, _>("call_count").unwrap_or_default()
                                    as u64,
                            })
                            .collect(),
                    )
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))
            }
        }
    }

    /// Query daily usage breakdown for the last N days.
    pub fn query_daily_breakdown(&self, days: u32) -> SiliCrewResult<Vec<DailyBreakdown>> {
        let cutoff = (Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;

                let mut stmt = conn
                    .prepare(
                        "SELECT date(timestamp) as day,
                                    COALESCE(SUM(cost_usd), 0.0),
                                    COALESCE(SUM(input_tokens) + SUM(output_tokens), 0),
                                    COUNT(*)
                             FROM usage_events
                             WHERE timestamp > ?1
                             GROUP BY day
                             ORDER BY day ASC",
                    )
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;

                let rows = stmt
                    .query_map(rusqlite::params![cutoff], |row| {
                        Ok(DailyBreakdown {
                            date: row.get(0)?,
                            cost_usd: row.get(1)?,
                            tokens: row.get::<_, i64>(2)? as u64,
                            calls: row.get::<_, i64>(3)? as u64,
                        })
                    })
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;

                let mut results = Vec::new();
                for row in rows {
                    results.push(row.map_err(|e| SiliCrewError::Memory(e.to_string()))?);
                }
                Ok(results)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                block_on(async move {
                    let rows = sqlx::query(
                        "SELECT LEFT(timestamp, 10) AS day,
                                COALESCE(SUM(cost_usd), 0.0) AS cost_usd,
                                COALESCE(SUM(input_tokens) + SUM(output_tokens), 0) AS tokens,
                                COUNT(*) AS calls
                         FROM usage_events
                         WHERE timestamp > $1
                         GROUP BY day
                         ORDER BY day ASC",
                    )
                    .bind(cutoff)
                    .fetch_all(&*pool)
                    .await?;
                    Ok::<Vec<DailyBreakdown>, sqlx::Error>(
                        rows.into_iter()
                            .map(|row| DailyBreakdown {
                                date: row.try_get("day").unwrap_or_default(),
                                cost_usd: row.try_get("cost_usd").unwrap_or(0.0),
                                tokens: row.try_get::<i64, _>("tokens").unwrap_or_default() as u64,
                                calls: row.try_get::<i64, _>("calls").unwrap_or_default() as u64,
                            })
                            .collect(),
                    )
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))
            }
        }
    }

    /// Query the timestamp of the earliest usage event.
    pub fn query_first_event_date(&self) -> SiliCrewResult<Option<String>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.query_row("SELECT MIN(timestamp) FROM usage_events", [], |row| {
                    row.get(0)
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                block_on(async move {
                    let row =
                        sqlx::query("SELECT MIN(timestamp) AS first_timestamp FROM usage_events")
                            .fetch_one(&*pool)
                            .await?;
                    Ok::<Option<String>, sqlx::Error>(row.try_get("first_timestamp")?)
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))
            }
        }
    }

    /// Query today's total cost across all agents.
    pub fn query_today_cost(&self) -> SiliCrewResult<f64> {
        let cutoff = Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .expect("midnight")
            .and_utc()
            .to_rfc3339();
        self.query_cost_since(None, &cutoff)
    }

    /// Delete usage events older than the given number of days.
    pub fn cleanup_old(&self, days: u32) -> SiliCrewResult<usize> {
        let cutoff = (Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let deleted = conn
                    .execute(
                        "DELETE FROM usage_events WHERE timestamp < ?1",
                        rusqlite::params![cutoff],
                    )
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                Ok(deleted)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                block_on(async move {
                    let result = sqlx::query("DELETE FROM usage_events WHERE timestamp < $1")
                        .bind(cutoff)
                        .execute(&*pool)
                        .await?;
                    Ok::<usize, sqlx::Error>(result.rows_affected() as usize)
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))
            }
        }
    }

    fn query_cost_since(&self, agent_id: Option<AgentId>, cutoff: &str) -> SiliCrewResult<f64> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                match agent_id {
                    Some(agent_id) => conn
                        .query_row(
                            "SELECT COALESCE(SUM(cost_usd), 0.0) FROM usage_events
                             WHERE agent_id = ?1 AND timestamp > ?2",
                            rusqlite::params![agent_id.0.to_string(), cutoff],
                            |row| row.get(0),
                        )
                        .map_err(|e| SiliCrewError::Memory(e.to_string())),
                    None => conn
                        .query_row(
                            "SELECT COALESCE(SUM(cost_usd), 0.0) FROM usage_events
                             WHERE timestamp > ?1",
                            rusqlite::params![cutoff],
                            |row| row.get(0),
                        )
                        .map_err(|e| SiliCrewError::Memory(e.to_string())),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let cutoff = cutoff.to_string();
                block_on(async move {
                    let row = if let Some(agent_id) = agent_id {
                        sqlx::query(
                            "SELECT COALESCE(SUM(cost_usd), 0.0) AS cost
                             FROM usage_events
                             WHERE agent_id = $1 AND timestamp > $2",
                        )
                        .bind(agent_id.0.to_string())
                        .bind(cutoff)
                        .fetch_one(&*pool)
                        .await?
                    } else {
                        sqlx::query(
                            "SELECT COALESCE(SUM(cost_usd), 0.0) AS cost
                             FROM usage_events
                             WHERE timestamp > $1",
                        )
                        .bind(cutoff)
                        .fetch_one(&*pool)
                        .await?
                    };
                    row.try_get("cost")
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::run_migrations;
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};

    fn setup() -> UsageStore {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        UsageStore::new(Arc::new(Mutex::new(conn)))
    }

    #[test]
    fn test_record_and_query_summary() {
        let store = setup();
        let agent_id = AgentId::new();

        store
            .record(&UsageRecord {
                agent_id,
                model: "claude-haiku".to_string(),
                input_tokens: 100,
                output_tokens: 50,
                cost_usd: 0.001,
                tool_calls: 2,
            })
            .unwrap();

        store
            .record(&UsageRecord {
                agent_id,
                model: "claude-sonnet".to_string(),
                input_tokens: 500,
                output_tokens: 200,
                cost_usd: 0.01,
                tool_calls: 1,
            })
            .unwrap();

        let summary = store.query_summary(Some(agent_id)).unwrap();
        assert_eq!(summary.call_count, 2);
        assert_eq!(summary.total_input_tokens, 600);
        assert_eq!(summary.total_output_tokens, 250);
        assert!((summary.total_cost_usd - 0.011).abs() < 0.0001);
        assert_eq!(summary.total_tool_calls, 3);
    }

    #[test]
    fn test_query_summary_all_agents() {
        let store = setup();
        let a1 = AgentId::new();
        let a2 = AgentId::new();

        store
            .record(&UsageRecord {
                agent_id: a1,
                model: "haiku".to_string(),
                input_tokens: 100,
                output_tokens: 50,
                cost_usd: 0.001,
                tool_calls: 0,
            })
            .unwrap();

        store
            .record(&UsageRecord {
                agent_id: a2,
                model: "sonnet".to_string(),
                input_tokens: 200,
                output_tokens: 100,
                cost_usd: 0.005,
                tool_calls: 1,
            })
            .unwrap();

        let summary = store.query_summary(None).unwrap();
        assert_eq!(summary.call_count, 2);
        assert_eq!(summary.total_input_tokens, 300);
    }

    #[test]
    fn test_query_by_model() {
        let store = setup();
        let agent_id = AgentId::new();

        for _ in 0..3 {
            store
                .record(&UsageRecord {
                    agent_id,
                    model: "haiku".to_string(),
                    input_tokens: 100,
                    output_tokens: 50,
                    cost_usd: 0.001,
                    tool_calls: 0,
                })
                .unwrap();
        }

        store
            .record(&UsageRecord {
                agent_id,
                model: "sonnet".to_string(),
                input_tokens: 500,
                output_tokens: 200,
                cost_usd: 0.01,
                tool_calls: 1,
            })
            .unwrap();

        let by_model = store.query_by_model().unwrap();
        assert_eq!(by_model.len(), 2);
        // sonnet should be first (highest cost)
        assert_eq!(by_model[0].model, "sonnet");
        assert_eq!(by_model[1].model, "haiku");
        assert_eq!(by_model[1].call_count, 3);
    }

    #[test]
    fn test_query_hourly() {
        let store = setup();
        let agent_id = AgentId::new();

        store
            .record(&UsageRecord {
                agent_id,
                model: "haiku".to_string(),
                input_tokens: 100,
                output_tokens: 50,
                cost_usd: 0.05,
                tool_calls: 0,
            })
            .unwrap();

        let hourly = store.query_hourly(agent_id).unwrap();
        assert!((hourly - 0.05).abs() < 0.001);
    }

    #[test]
    fn test_query_daily() {
        let store = setup();
        let agent_id = AgentId::new();

        store
            .record(&UsageRecord {
                agent_id,
                model: "haiku".to_string(),
                input_tokens: 100,
                output_tokens: 50,
                cost_usd: 0.123,
                tool_calls: 0,
            })
            .unwrap();

        let daily = store.query_daily(agent_id).unwrap();
        assert!((daily - 0.123).abs() < 0.001);
    }

    #[test]
    fn test_cleanup_old() {
        let store = setup();
        let agent_id = AgentId::new();

        store
            .record(&UsageRecord {
                agent_id,
                model: "haiku".to_string(),
                input_tokens: 100,
                output_tokens: 50,
                cost_usd: 0.001,
                tool_calls: 0,
            })
            .unwrap();

        // Cleanup events older than 1 day should not remove today's events
        let deleted = store.cleanup_old(1).unwrap();
        assert_eq!(deleted, 0);

        let summary = store.query_summary(None).unwrap();
        assert_eq!(summary.call_count, 1);
    }

    #[test]
    fn test_empty_summary() {
        let store = setup();
        let summary = store.query_summary(None).unwrap();
        assert_eq!(summary.call_count, 0);
        assert_eq!(summary.total_cost_usd, 0.0);
    }
}
