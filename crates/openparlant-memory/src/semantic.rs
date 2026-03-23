//! Semantic memory store with vector embedding support.
//!
//! Phase 1: SQLite LIKE matching (fallback when no embeddings).
//! Phase 2: Vector cosine similarity search using stored embeddings.
//!
//! Embeddings are stored as BLOBs in the `embedding` column of the memories table.
//! When a query embedding is provided, recall uses cosine similarity ranking.
//! When no embeddings are available, falls back to LIKE matching.

use crate::db::{block_on, SharedDb};
use chrono::Utc;
use silicrew_types::agent::AgentId;
use silicrew_types::error::{SiliCrewError, SiliCrewResult};
use silicrew_types::memory::{MemoryFilter, MemoryFragment, MemoryId, MemorySource};
#[cfg(test)]
use rusqlite::Connection;
use sqlx::{Postgres, QueryBuilder, Row};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::debug;

/// Semantic store backed by the shared SQL database with optional vector search.
#[derive(Clone)]
pub struct SemanticStore {
    db: SharedDb,
}

impl SemanticStore {
    /// Create a new semantic store wrapping the given connection.
    pub fn new(db: impl Into<SharedDb>) -> Self {
        Self { db: db.into() }
    }

    /// Store a new memory fragment (without embedding).
    pub fn remember(
        &self,
        agent_id: AgentId,
        content: &str,
        source: MemorySource,
        scope: &str,
        metadata: HashMap<String, serde_json::Value>,
    ) -> SiliCrewResult<MemoryId> {
        self.remember_with_embedding(agent_id, content, source, scope, metadata, None)
    }

    /// Store a new memory fragment with an optional embedding vector.
    pub fn remember_with_embedding(
        &self,
        agent_id: AgentId,
        content: &str,
        source: MemorySource,
        scope: &str,
        metadata: HashMap<String, serde_json::Value>,
        embedding: Option<&[f32]>,
    ) -> SiliCrewResult<MemoryId> {
        let id = MemoryId::new();
        let now = Utc::now().to_rfc3339();
        let source_str = serde_json::to_string(&source)
            .map_err(|e| SiliCrewError::Serialization(e.to_string()))?;
        let meta_str = serde_json::to_string(&metadata)
            .map_err(|e| SiliCrewError::Serialization(e.to_string()))?;
        let embedding_bytes: Option<Vec<u8>> = embedding.map(embedding_to_bytes);
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO memories (id, agent_id, content, source, scope, confidence, metadata, created_at, accessed_at, access_count, deleted, embedding)
                     VALUES (?1, ?2, ?3, ?4, ?5, 1.0, ?6, ?7, ?7, 0, 0, ?8)",
                    rusqlite::params![
                        id.0.to_string(),
                        agent_id.0.to_string(),
                        content,
                        source_str,
                        scope,
                        meta_str,
                        now,
                        embedding_bytes,
                    ],
                )
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let content = content.to_string();
                let scope = scope.to_string();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO memories (id, agent_id, content, source, scope, confidence, metadata, created_at, accessed_at, access_count, deleted, embedding)
                         VALUES ($1, $2, $3, $4, $5, 1.0, $6, $7, $7, 0, FALSE, $8)",
                    )
                    .bind(id.0.to_string())
                    .bind(agent_id.0.to_string())
                    .bind(content)
                    .bind(source_str)
                    .bind(scope)
                    .bind(meta_str)
                    .bind(now)
                    .bind(embedding_bytes)
                    .execute(&*pool)
                    .await
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
        }
        Ok(id)
    }

    /// Search for memories using text matching (fallback, no embeddings).
    pub fn recall(
        &self,
        query: &str,
        limit: usize,
        filter: Option<MemoryFilter>,
    ) -> SiliCrewResult<Vec<MemoryFragment>> {
        self.recall_with_embedding(query, limit, filter, None)
    }

    /// Search for memories using vector similarity when a query embedding is provided,
    /// falling back to LIKE matching otherwise.
    pub fn recall_with_embedding(
        &self,
        query: &str,
        limit: usize,
        filter: Option<MemoryFilter>,
        query_embedding: Option<&[f32]>,
    ) -> SiliCrewResult<Vec<MemoryFragment>> {
        let fetch_limit = if query_embedding.is_some() {
            (limit * 10).max(100)
        } else {
            limit
        };
        let mut fragments = match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut sql = String::from(
                    "SELECT id, agent_id, content, source, scope, confidence, metadata, created_at, accessed_at, access_count, embedding
                     FROM memories WHERE deleted = 0",
                );
                let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
                let mut param_idx = 1;
                if query_embedding.is_none() && !query.is_empty() {
                    sql.push_str(&format!(" AND content LIKE ?{param_idx}"));
                    params.push(Box::new(format!("%{query}%")));
                    param_idx += 1;
                }
                if let Some(ref f) = filter {
                    if let Some(agent_id) = f.agent_id {
                        sql.push_str(&format!(" AND agent_id = ?{param_idx}"));
                        params.push(Box::new(agent_id.0.to_string()));
                        param_idx += 1;
                    }
                    if let Some(ref scope) = f.scope {
                        sql.push_str(&format!(" AND scope = ?{param_idx}"));
                        params.push(Box::new(scope.clone()));
                        param_idx += 1;
                    }
                    if let Some(min_conf) = f.min_confidence {
                        sql.push_str(&format!(" AND confidence >= ?{param_idx}"));
                        params.push(Box::new(min_conf as f64));
                        param_idx += 1;
                    }
                    if let Some(ref source) = f.source {
                        let source_str = serde_json::to_string(source)
                            .map_err(|e| SiliCrewError::Serialization(e.to_string()))?;
                        sql.push_str(&format!(" AND source = ?{param_idx}"));
                        params.push(Box::new(source_str));
                    }
                }
                sql.push_str(" ORDER BY accessed_at DESC, access_count DESC");
                sql.push_str(&format!(" LIMIT {fetch_limit}"));
                let mut stmt = conn
                    .prepare(&sql)
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();
                let rows = stmt
                    .query_map(param_refs.as_slice(), |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, f64>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                            row.get::<_, String>(8)?,
                            row.get::<_, i64>(9)?,
                            row.get::<_, Option<Vec<u8>>>(10)?,
                        ))
                    })
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(decode_fragment(
                        row.map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                    )?);
                }
                for frag in &out {
                    let _ = conn.execute(
                        "UPDATE memories SET access_count = access_count + 1, accessed_at = ?1 WHERE id = ?2",
                        rusqlite::params![Utc::now().to_rfc3339(), frag.id.0.to_string()],
                    );
                }
                out
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new(
                    "SELECT id, agent_id, content, source, scope, confidence, metadata, created_at, accessed_at, access_count, embedding
                     FROM memories WHERE deleted = FALSE",
                );
                if query_embedding.is_none() && !query.is_empty() {
                    qb.push(" AND content ILIKE ");
                    qb.push_bind(format!("%{query}%"));
                }
                if let Some(ref f) = filter {
                    if let Some(agent_id) = f.agent_id {
                        qb.push(" AND agent_id = ");
                        qb.push_bind(agent_id.0.to_string());
                    }
                    if let Some(ref scope) = f.scope {
                        qb.push(" AND scope = ");
                        qb.push_bind(scope);
                    }
                    if let Some(min_conf) = f.min_confidence {
                        qb.push(" AND confidence >= ");
                        qb.push_bind(min_conf as f64);
                    }
                    if let Some(ref source) = f.source {
                        let source_str = serde_json::to_string(source)
                            .map_err(|e| SiliCrewError::Serialization(e.to_string()))?;
                        qb.push(" AND source = ");
                        qb.push_bind(source_str);
                    }
                }
                qb.push(" ORDER BY accessed_at DESC, access_count DESC LIMIT ");
                qb.push_bind(fetch_limit as i64);
                let query_pool = Arc::clone(&pool);
                let rows = block_on(async move { qb.build().fetch_all(&*query_pool).await })
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(decode_fragment((
                        row.try_get(0)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        row.try_get(1)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        row.try_get(2)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        row.try_get(3)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        row.try_get(4)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        row.try_get(5)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        row.try_get(6)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        row.try_get(7)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        row.try_get(8)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        row.try_get(9)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        row.try_get(10)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                    ))?);
                }
                for frag in &out {
                    let memory_id = frag.id.0.to_string();
                    let pool = Arc::clone(&pool);
                    let _ = block_on(async move {
                        sqlx::query(
                            "UPDATE memories SET access_count = access_count + 1, accessed_at = $1 WHERE id = $2",
                        )
                        .bind(Utc::now().to_rfc3339())
                        .bind(memory_id)
                        .execute(&*pool)
                        .await
                    });
                }
                out
            }
        };

        // If we have a query embedding, re-rank by cosine similarity
        if let Some(qe) = query_embedding {
            fragments.sort_by(|a, b| {
                let sim_a = a
                    .embedding
                    .as_deref()
                    .map(|e| cosine_similarity(qe, e))
                    .unwrap_or(-1.0);
                let sim_b = b
                    .embedding
                    .as_deref()
                    .map(|e| cosine_similarity(qe, e))
                    .unwrap_or(-1.0);
                sim_b
                    .partial_cmp(&sim_a)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            fragments.truncate(limit);
            debug!(
                "Vector recall: {} results from {} candidates",
                fragments.len(),
                fetch_limit
            );
        }
        Ok(fragments)
    }

    /// Soft-delete a memory fragment.
    pub fn forget(&self, id: MemoryId) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "UPDATE memories SET deleted = 1 WHERE id = ?1",
                    rusqlite::params![id.0.to_string()],
                )
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                block_on(async move {
                    sqlx::query("UPDATE memories SET deleted = TRUE WHERE id = $1")
                        .bind(id.0.to_string())
                        .execute(&*pool)
                        .await
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// Update the embedding for an existing memory.
    pub fn update_embedding(&self, id: MemoryId, embedding: &[f32]) -> SiliCrewResult<()> {
        let bytes = embedding_to_bytes(embedding);
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "UPDATE memories SET embedding = ?1 WHERE id = ?2",
                    rusqlite::params![bytes, id.0.to_string()],
                )
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                block_on(async move {
                    sqlx::query("UPDATE memories SET embedding = $1 WHERE id = $2")
                        .bind(bytes)
                        .bind(id.0.to_string())
                        .execute(&*pool)
                        .await
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }
}

fn decode_fragment(
    row: (
        String,
        String,
        String,
        String,
        String,
        f64,
        String,
        String,
        String,
        i64,
        Option<Vec<u8>>,
    ),
) -> SiliCrewResult<MemoryFragment> {
    let id = uuid::Uuid::parse_str(&row.0)
        .map(MemoryId)
        .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
    let agent_id = uuid::Uuid::parse_str(&row.1)
        .map(silicrew_types::agent::AgentId)
        .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
    let source: MemorySource = serde_json::from_str(&row.3).unwrap_or(MemorySource::System);
    let metadata: HashMap<String, serde_json::Value> =
        serde_json::from_str(&row.6).unwrap_or_default();
    let created_at = chrono::DateTime::parse_from_rfc3339(&row.7)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let accessed_at = chrono::DateTime::parse_from_rfc3339(&row.8)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    Ok(MemoryFragment {
        id,
        agent_id,
        content: row.2,
        embedding: row.10.as_deref().map(embedding_from_bytes),
        metadata,
        source,
        confidence: row.5 as f32,
        created_at,
        accessed_at,
        access_count: row.9 as u64,
        scope: row.4,
    })
}

/// Compute cosine similarity between two vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom < f32::EPSILON {
        0.0
    } else {
        dot / denom
    }
}

/// Serialize embedding to bytes for SQLite BLOB storage.
fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for &val in embedding {
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

/// Deserialize embedding from bytes.
fn embedding_from_bytes(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::run_migrations;
    use std::sync::{Arc, Mutex};

    fn setup() -> SemanticStore {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        SemanticStore::new(Arc::new(Mutex::new(conn)))
    }

    #[test]
    fn test_remember_and_recall() {
        let store = setup();
        let agent_id = AgentId::new();
        store
            .remember(
                agent_id,
                "The user likes Rust programming",
                MemorySource::Conversation,
                "episodic",
                HashMap::new(),
            )
            .unwrap();
        let results = store.recall("Rust", 10, None).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("Rust"));
    }

    #[test]
    fn test_recall_with_filter() {
        let store = setup();
        let agent_id = AgentId::new();
        store
            .remember(
                agent_id,
                "Memory A",
                MemorySource::Conversation,
                "episodic",
                HashMap::new(),
            )
            .unwrap();
        store
            .remember(
                AgentId::new(),
                "Memory B",
                MemorySource::Conversation,
                "episodic",
                HashMap::new(),
            )
            .unwrap();
        let filter = MemoryFilter::agent(agent_id);
        let results = store.recall("Memory", 10, Some(filter)).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "Memory A");
    }

    #[test]
    fn test_forget() {
        let store = setup();
        let agent_id = AgentId::new();
        let id = store
            .remember(
                agent_id,
                "To forget",
                MemorySource::Conversation,
                "episodic",
                HashMap::new(),
            )
            .unwrap();
        store.forget(id).unwrap();
        let results = store.recall("To forget", 10, None).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_remember_with_embedding() {
        let store = setup();
        let agent_id = AgentId::new();
        let embedding = vec![0.1, 0.2, 0.3, 0.4];
        let id = store
            .remember_with_embedding(
                agent_id,
                "Rust is great",
                MemorySource::Conversation,
                "episodic",
                HashMap::new(),
                Some(&embedding),
            )
            .unwrap();
        assert_ne!(id.0.to_string(), "");
    }

    #[test]
    fn test_vector_recall_ranking() {
        let store = setup();
        let agent_id = AgentId::new();

        // Store 3 memories with embeddings pointing in different directions
        let emb_rust = vec![0.9, 0.1, 0.0, 0.0]; // "Rust" direction
        let emb_python = vec![0.0, 0.0, 0.9, 0.1]; // "Python" direction
        let emb_mixed = vec![0.5, 0.5, 0.0, 0.0]; // mixed

        store
            .remember_with_embedding(
                agent_id,
                "Rust is a systems language",
                MemorySource::Conversation,
                "episodic",
                HashMap::new(),
                Some(&emb_rust),
            )
            .unwrap();
        store
            .remember_with_embedding(
                agent_id,
                "Python is interpreted",
                MemorySource::Conversation,
                "episodic",
                HashMap::new(),
                Some(&emb_python),
            )
            .unwrap();
        store
            .remember_with_embedding(
                agent_id,
                "Both are popular",
                MemorySource::Conversation,
                "episodic",
                HashMap::new(),
                Some(&emb_mixed),
            )
            .unwrap();

        // Query with a "Rust"-like embedding
        let query_emb = vec![0.85, 0.15, 0.0, 0.0];
        let results = store
            .recall_with_embedding("", 3, None, Some(&query_emb))
            .unwrap();

        assert_eq!(results.len(), 3);
        // Rust memory should be first (highest cosine similarity)
        assert!(results[0].content.contains("Rust"));
        // Python memory should be last (lowest similarity)
        assert!(results[2].content.contains("Python"));
    }

    #[test]
    fn test_update_embedding() {
        let store = setup();
        let agent_id = AgentId::new();
        let id = store
            .remember(
                agent_id,
                "No embedding yet",
                MemorySource::Conversation,
                "episodic",
                HashMap::new(),
            )
            .unwrap();

        // Update with embedding
        let emb = vec![1.0, 0.0, 0.0];
        store.update_embedding(id, &emb).unwrap();

        // Verify the embedding is stored by doing vector recall
        let query_emb = vec![1.0, 0.0, 0.0];
        let results = store
            .recall_with_embedding("", 10, None, Some(&query_emb))
            .unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].embedding.is_some());
        assert_eq!(results[0].embedding.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn test_mixed_embedded_and_non_embedded() {
        let store = setup();
        let agent_id = AgentId::new();

        // One memory with embedding, one without
        store
            .remember_with_embedding(
                agent_id,
                "Has embedding",
                MemorySource::Conversation,
                "episodic",
                HashMap::new(),
                Some(&[1.0, 0.0]),
            )
            .unwrap();
        store
            .remember(
                agent_id,
                "No embedding",
                MemorySource::Conversation,
                "episodic",
                HashMap::new(),
            )
            .unwrap();

        // Vector recall should rank embedded memory higher
        let results = store
            .recall_with_embedding("", 10, None, Some(&[1.0, 0.0]))
            .unwrap();
        assert_eq!(results.len(), 2);
        // Embedded memory should rank first
        assert_eq!(results[0].content, "Has embedding");
    }
}
