//! Memory consolidation and decay logic.
//!
//! Reduces confidence of old, unaccessed memories and merges
//! duplicate/similar memories.

use crate::db::{block_on, SharedDb};
use chrono::Utc;
use openparlant_types::error::{OpenFangError, OpenFangResult};
use openparlant_types::memory::ConsolidationReport;
#[cfg(test)]
use rusqlite::Connection;
#[cfg(test)]
use std::sync::{Arc, Mutex};

/// Memory consolidation engine.
#[derive(Clone)]
pub struct ConsolidationEngine {
    db: SharedDb,
    /// Decay rate: how much to reduce confidence per consolidation cycle.
    decay_rate: f32,
}

impl ConsolidationEngine {
    /// Create a new consolidation engine.
    pub fn new(db: impl Into<SharedDb>, decay_rate: f32) -> Self {
        Self {
            db: db.into(),
            decay_rate,
        }
    }

    /// Run a consolidation cycle: decay old memories.
    pub fn consolidate(&self) -> OpenFangResult<ConsolidationReport> {
        let start = std::time::Instant::now();
        // Decay confidence of memories not accessed in the last 7 days
        let cutoff = (Utc::now() - chrono::Duration::days(7)).to_rfc3339();
        let decay_factor = 1.0 - self.decay_rate as f64;
        let decayed = match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                conn.execute(
                    "UPDATE memories SET confidence = MAX(0.1, confidence * ?1)
                     WHERE deleted = 0 AND accessed_at < ?2 AND confidence > 0.1",
                    rusqlite::params![decay_factor, cutoff],
                )
                .map_err(|e| OpenFangError::Memory(e.to_string()))?
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let result = block_on(async move {
                    sqlx::query(
                        "UPDATE memories
                         SET confidence = GREATEST(0.1, confidence * $1)
                         WHERE deleted = FALSE AND accessed_at < $2 AND confidence > 0.1",
                    )
                    .bind(decay_factor)
                    .bind(cutoff)
                    .execute(&*pool)
                    .await
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))?;
                result.rows_affected() as usize
            }
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ConsolidationReport {
            memories_merged: 0, // Phase 1: no merging
            memories_decayed: decayed as u64,
            duration_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::run_migrations;

    fn setup() -> ConsolidationEngine {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        ConsolidationEngine::new(Arc::new(Mutex::new(conn)), 0.1)
    }

    #[test]
    fn test_consolidation_empty() {
        let engine = setup();
        let report = engine.consolidate().unwrap();
        assert_eq!(report.memories_decayed, 0);
    }

    #[test]
    fn test_consolidation_decays_old_memories() {
        let engine = setup();
        let conn = engine.db.sqlite().unwrap();
        let conn = conn.lock().unwrap();
        // Insert an old memory
        let old_date = (Utc::now() - chrono::Duration::days(30)).to_rfc3339();
        conn.execute(
            "INSERT INTO memories (id, agent_id, content, source, scope, confidence, metadata, created_at, accessed_at, access_count, deleted)
             VALUES ('test-id', 'agent-1', 'old memory', '\"conversation\"', 'episodic', 0.9, '{}', ?1, ?1, 0, 0)",
            rusqlite::params![old_date],
        ).unwrap();
        drop(conn);

        let report = engine.consolidate().unwrap();
        assert_eq!(report.memories_decayed, 1);

        // Verify confidence was reduced
        let conn = engine.db.sqlite().unwrap();
        let conn = conn.lock().unwrap();
        let confidence: f64 = conn
            .query_row(
                "SELECT confidence FROM memories WHERE id = 'test-id'",
                [],
                |row| row.get::<_, f64>(0),
            )
            .unwrap();
        assert!(confidence < 0.9);
    }
}
