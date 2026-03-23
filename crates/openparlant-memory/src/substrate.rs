//! MemorySubstrate: unified implementation of the `Memory` trait.
//!
//! Composes the structured store, semantic store, knowledge store,
//! session store, and consolidation engine behind a single async API.

use crate::consolidation::ConsolidationEngine;
use crate::db::{block_on, SharedDb};
use crate::knowledge::KnowledgeStore;
use crate::migration::{run_migrations, run_postgres_migrations};
use crate::semantic::SemanticStore;
use crate::session::{Session, SessionStore};
use crate::structured::StructuredStore;
use crate::usage::UsageStore;

use async_trait::async_trait;
use openparlant_types::agent::{AgentEntry, AgentId, SessionId};
use openparlant_types::error::{SiliCrewError, SiliCrewResult};
use openparlant_types::memory::{
    ConsolidationReport, Entity, ExportFormat, GraphMatch, GraphPattern, ImportReport, Memory,
    MemoryFilter, MemoryFragment, MemoryId, MemorySource, Relation,
};
use sqlx::Row;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// The unified memory substrate. Implements the `Memory` trait by delegating
/// to specialized stores backed by a shared database handle.
pub struct MemorySubstrate {
    db: SharedDb,
    structured: StructuredStore,
    semantic: SemanticStore,
    knowledge: KnowledgeStore,
    sessions: SessionStore,
    consolidation: ConsolidationEngine,
    usage: UsageStore,
}

impl MemorySubstrate {
    /// Open or create a memory substrate at the given database path.
    pub fn open(db_path: &Path, decay_rate: f32) -> SiliCrewResult<Self> {
        let db = SharedDb::open_sqlite(db_path)?;
        if let Some(conn) = db.sqlite() {
            let conn = conn
                .lock()
                .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
            run_migrations(&conn).map_err(|e| SiliCrewError::Memory(e.to_string()))?;
        }
        Self::from_db(db, decay_rate)
    }

    /// Open or create a memory substrate backed by PostgreSQL.
    pub async fn open_postgres(database_url: &str, decay_rate: f32) -> SiliCrewResult<Self> {
        let db = SharedDb::open_postgres(database_url).await?;
        if let Some(pool) = db.postgres() {
            run_postgres_migrations(&pool)
                .await
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
        }
        Self::from_db(db, decay_rate)
    }

    /// Create an in-memory substrate (for testing).
    pub fn open_in_memory(decay_rate: f32) -> SiliCrewResult<Self> {
        let db = SharedDb::open_sqlite_in_memory()?;
        if let Some(conn) = db.sqlite() {
            let conn = conn
                .lock()
                .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
            run_migrations(&conn).map_err(|e| SiliCrewError::Memory(e.to_string()))?;
        }
        Self::from_db(db, decay_rate)
    }

    fn from_db(db: SharedDb, decay_rate: f32) -> SiliCrewResult<Self> {
        Ok(Self {
            db: db.clone(),
            structured: StructuredStore::new(db.clone()),
            semantic: SemanticStore::new(db.clone()),
            knowledge: KnowledgeStore::new(db.clone()),
            sessions: SessionStore::new(db.clone()),
            usage: UsageStore::new(db.clone()),
            consolidation: ConsolidationEngine::new(db, decay_rate),
        })
    }

    /// Get a reference to the usage store.
    pub fn usage(&self) -> &UsageStore {
        &self.usage
    }

    /// Get the shared database handle.
    pub fn shared_db(&self) -> SharedDb {
        self.db.clone()
    }

    /// Save an agent entry to persistent storage.
    pub fn save_agent(&self, entry: &AgentEntry) -> SiliCrewResult<()> {
        self.structured.save_agent(entry)
    }

    /// Load an agent entry from persistent storage.
    pub fn load_agent(&self, agent_id: AgentId) -> SiliCrewResult<Option<AgentEntry>> {
        self.structured.load_agent(agent_id)
    }

    /// Remove an agent from persistent storage and cascade-delete sessions.
    pub fn remove_agent(&self, agent_id: AgentId) -> SiliCrewResult<()> {
        // Delete associated sessions first
        let _ = self.sessions.delete_agent_sessions(agent_id);
        self.structured.remove_agent(agent_id)
    }

    /// Load all agent entries from persistent storage.
    pub fn load_all_agents(&self) -> SiliCrewResult<Vec<AgentEntry>> {
        self.structured.load_all_agents()
    }

    /// List all saved agents.
    pub fn list_agents(&self) -> SiliCrewResult<Vec<(String, String, String)>> {
        self.structured.list_agents()
    }

    /// Synchronous get from the structured store (for kernel handle use).
    pub fn structured_get(
        &self,
        agent_id: AgentId,
        key: &str,
    ) -> SiliCrewResult<Option<serde_json::Value>> {
        self.structured.get(agent_id, key)
    }

    /// List all KV pairs for an agent.
    pub fn list_kv(&self, agent_id: AgentId) -> SiliCrewResult<Vec<(String, serde_json::Value)>> {
        self.structured.list_kv(agent_id)
    }

    /// Delete a KV entry for an agent.
    pub fn structured_delete(&self, agent_id: AgentId, key: &str) -> SiliCrewResult<()> {
        self.structured.delete(agent_id, key)
    }

    /// Synchronous set in the structured store (for kernel handle use).
    pub fn structured_set(
        &self,
        agent_id: AgentId,
        key: &str,
        value: serde_json::Value,
    ) -> SiliCrewResult<()> {
        self.structured.set(agent_id, key, value)
    }

    /// Get a session by ID.
    pub fn get_session(&self, session_id: SessionId) -> SiliCrewResult<Option<Session>> {
        self.sessions.get_session(session_id)
    }

    /// Save a session.
    pub fn save_session(&self, session: &Session) -> SiliCrewResult<()> {
        self.sessions.save_session(session)
    }

    /// Save a session asynchronously — runs the SQLite write in a blocking
    /// thread so the tokio runtime stays responsive.
    pub async fn save_session_async(&self, session: &Session) -> SiliCrewResult<()> {
        let sessions = self.sessions.clone();
        let session = session.clone();
        tokio::task::spawn_blocking(move || sessions.save_session(&session))
            .await
            .map_err(|e| SiliCrewError::Internal(e.to_string()))?
    }

    /// Create a new empty session for an agent.
    pub fn create_session(&self, agent_id: AgentId) -> SiliCrewResult<Session> {
        self.sessions.create_session(agent_id)
    }

    /// List all sessions with metadata.
    pub fn list_sessions(&self) -> SiliCrewResult<Vec<serde_json::Value>> {
        self.sessions.list_sessions()
    }

    /// Delete a session by ID.
    pub fn delete_session(&self, session_id: SessionId) -> SiliCrewResult<()> {
        self.sessions.delete_session(session_id)
    }

    /// Delete all sessions belonging to an agent.
    pub fn delete_agent_sessions(&self, agent_id: AgentId) -> SiliCrewResult<()> {
        self.sessions.delete_agent_sessions(agent_id)
    }

    /// Delete the canonical (cross-channel) session for an agent.
    pub fn delete_canonical_session(&self, agent_id: AgentId) -> SiliCrewResult<()> {
        self.sessions.delete_canonical_session(agent_id)
    }

    /// Set or clear a session label.
    pub fn set_session_label(
        &self,
        session_id: SessionId,
        label: Option<&str>,
    ) -> SiliCrewResult<()> {
        self.sessions.set_session_label(session_id, label)
    }

    /// Find a session by label for a given agent.
    pub fn find_session_by_label(
        &self,
        agent_id: AgentId,
        label: &str,
    ) -> SiliCrewResult<Option<Session>> {
        self.sessions.find_session_by_label(agent_id, label)
    }

    /// List all sessions for a specific agent.
    pub fn list_agent_sessions(&self, agent_id: AgentId) -> SiliCrewResult<Vec<serde_json::Value>> {
        self.sessions.list_agent_sessions(agent_id)
    }

    /// Create a new session with an optional label.
    pub fn create_session_with_label(
        &self,
        agent_id: AgentId,
        label: Option<&str>,
    ) -> SiliCrewResult<Session> {
        self.sessions.create_session_with_label(agent_id, label)
    }

    /// Load canonical session context for cross-channel memory.
    ///
    /// Returns the compacted summary (if any) and recent messages from the
    /// agent's persistent canonical session.
    pub fn canonical_context(
        &self,
        agent_id: AgentId,
        window_size: Option<usize>,
    ) -> SiliCrewResult<(Option<String>, Vec<openparlant_types::message::Message>)> {
        self.sessions.canonical_context(agent_id, window_size)
    }

    /// Store an LLM-generated summary, replacing older messages with the kept subset.
    ///
    /// Used by the compactor to replace text-truncation compaction with an
    /// LLM-generated summary of older conversation history.
    pub fn store_llm_summary(
        &self,
        agent_id: AgentId,
        summary: &str,
        kept_messages: Vec<openparlant_types::message::Message>,
    ) -> SiliCrewResult<()> {
        self.sessions
            .store_llm_summary(agent_id, summary, kept_messages)
    }

    /// Write a human-readable JSONL mirror of a session to disk.
    ///
    /// Best-effort — errors are returned but should be logged,
    /// never affecting the primary SQLite store.
    pub fn write_jsonl_mirror(
        &self,
        session: &Session,
        sessions_dir: &Path,
    ) -> Result<(), std::io::Error> {
        self.sessions.write_jsonl_mirror(session, sessions_dir)
    }

    /// Append messages to the agent's canonical session for cross-channel persistence.
    pub fn append_canonical(
        &self,
        agent_id: AgentId,
        messages: &[openparlant_types::message::Message],
        compaction_threshold: Option<usize>,
    ) -> SiliCrewResult<()> {
        self.sessions
            .append_canonical(agent_id, messages, compaction_threshold)?;
        Ok(())
    }

    // -----------------------------------------------------------------
    // Paired devices persistence
    // -----------------------------------------------------------------

    /// Load all paired devices from the database.
    pub fn load_paired_devices(&self) -> SiliCrewResult<Vec<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let mut stmt = conn.prepare(
                    "SELECT device_id, display_name, platform, paired_at, last_seen, push_token FROM paired_devices"
                ).map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok(serde_json::json!({
                            "device_id": row.get::<_, String>(0)?,
                            "display_name": row.get::<_, String>(1)?,
                            "platform": row.get::<_, String>(2)?,
                            "paired_at": row.get::<_, String>(3)?,
                            "last_seen": row.get::<_, String>(4)?,
                            "push_token": row.get::<_, Option<String>>(5)?,
                        }))
                    })
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let mut devices = Vec::new();
                for row in rows {
                    devices.push(row.map_err(|e| SiliCrewError::Memory(e.to_string()))?);
                }
                Ok(devices)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT device_id, display_name, platform, paired_at, last_seen, push_token FROM paired_devices",
                    )
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let mut devices = Vec::with_capacity(rows.len());
                for row in rows {
                    devices.push(serde_json::json!({
                        "device_id": row.try_get::<String, _>("device_id").map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        "display_name": row.try_get::<String, _>("display_name").map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        "platform": row.try_get::<String, _>("platform").map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        "paired_at": row.try_get::<String, _>("paired_at").map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        "last_seen": row.try_get::<String, _>("last_seen").map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        "push_token": row.try_get::<Option<String>, _>("push_token").map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                    }));
                }
                Ok(devices)
            }
        }
    }

    /// Save a paired device to the database (insert or replace).
    pub fn save_paired_device(
        &self,
        device_id: &str,
        display_name: &str,
        platform: &str,
        paired_at: &str,
        last_seen: &str,
        push_token: Option<&str>,
    ) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                conn.execute(
                    "INSERT OR REPLACE INTO paired_devices (device_id, display_name, platform, paired_at, last_seen, push_token) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![device_id, display_name, platform, paired_at, last_seen, push_token],
                ).map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let device_id = device_id.to_string();
                let display_name = display_name.to_string();
                let platform = platform.to_string();
                let paired_at = paired_at.to_string();
                let last_seen = last_seen.to_string();
                let push_token = push_token.map(|s| s.to_string());
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO paired_devices (device_id, display_name, platform, paired_at, last_seen, push_token)
                         VALUES ($1, $2, $3, $4, $5, $6)
                         ON CONFLICT(device_id) DO UPDATE SET
                            display_name = EXCLUDED.display_name,
                            platform = EXCLUDED.platform,
                            paired_at = EXCLUDED.paired_at,
                            last_seen = EXCLUDED.last_seen,
                            push_token = EXCLUDED.push_token",
                    )
                    .bind(device_id)
                    .bind(display_name)
                    .bind(platform)
                    .bind(paired_at)
                    .bind(last_seen)
                    .bind(push_token)
                    .execute(&*pool)
                    .await
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// Remove a paired device from the database.
    pub fn remove_paired_device(&self, device_id: &str) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                conn.execute(
                    "DELETE FROM paired_devices WHERE device_id = ?1",
                    rusqlite::params![device_id],
                )
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let device_id = device_id.to_string();
                block_on(async move {
                    sqlx::query("DELETE FROM paired_devices WHERE device_id = $1")
                        .bind(device_id)
                        .execute(&*pool)
                        .await
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------
    // Embedding-aware memory operations
    // -----------------------------------------------------------------

    /// Store a memory with an embedding vector.
    pub fn remember_with_embedding(
        &self,
        agent_id: AgentId,
        content: &str,
        source: MemorySource,
        scope: &str,
        metadata: HashMap<String, serde_json::Value>,
        embedding: Option<&[f32]>,
    ) -> SiliCrewResult<MemoryId> {
        self.semantic
            .remember_with_embedding(agent_id, content, source, scope, metadata, embedding)
    }

    /// Recall memories using vector similarity when a query embedding is provided.
    pub fn recall_with_embedding(
        &self,
        query: &str,
        limit: usize,
        filter: Option<MemoryFilter>,
        query_embedding: Option<&[f32]>,
    ) -> SiliCrewResult<Vec<MemoryFragment>> {
        self.semantic
            .recall_with_embedding(query, limit, filter, query_embedding)
    }

    /// Update the embedding for an existing memory.
    pub fn update_embedding(&self, id: MemoryId, embedding: &[f32]) -> SiliCrewResult<()> {
        self.semantic.update_embedding(id, embedding)
    }

    /// Async wrapper for `recall_with_embedding` — runs in a blocking thread.
    pub async fn recall_with_embedding_async(
        &self,
        query: &str,
        limit: usize,
        filter: Option<MemoryFilter>,
        query_embedding: Option<&[f32]>,
    ) -> SiliCrewResult<Vec<MemoryFragment>> {
        let store = self.semantic.clone();
        let query = query.to_string();
        let embedding_owned = query_embedding.map(|e| e.to_vec());
        tokio::task::spawn_blocking(move || {
            store.recall_with_embedding(&query, limit, filter, embedding_owned.as_deref())
        })
        .await
        .map_err(|e| SiliCrewError::Internal(e.to_string()))?
    }

    /// Async wrapper for `remember_with_embedding` — runs in a blocking thread.
    pub async fn remember_with_embedding_async(
        &self,
        agent_id: AgentId,
        content: &str,
        source: MemorySource,
        scope: &str,
        metadata: HashMap<String, serde_json::Value>,
        embedding: Option<&[f32]>,
    ) -> SiliCrewResult<MemoryId> {
        let store = self.semantic.clone();
        let content = content.to_string();
        let scope = scope.to_string();
        let embedding_owned = embedding.map(|e| e.to_vec());
        tokio::task::spawn_blocking(move || {
            store.remember_with_embedding(
                agent_id,
                &content,
                source,
                &scope,
                metadata,
                embedding_owned.as_deref(),
            )
        })
        .await
        .map_err(|e| SiliCrewError::Internal(e.to_string()))?
    }

    // -----------------------------------------------------------------
    // Task queue operations
    // -----------------------------------------------------------------

    /// Post a new task to the shared queue. Returns the task ID.
    pub async fn task_post(
        &self,
        title: &str,
        description: &str,
        assigned_to: Option<&str>,
        created_by: Option<&str>,
    ) -> SiliCrewResult<String> {
        let title = title.to_string();
        let description = description.to_string();
        let assigned_to = assigned_to.unwrap_or("").to_string();
        let created_by = created_by.unwrap_or("").to_string();
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = Arc::clone(conn);
                let inserted_id = id.clone();
                tokio::task::spawn_blocking(move || {
                    let db = conn.lock().map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                    db.execute(
                        "INSERT INTO task_queue (id, agent_id, task_type, payload, status, priority, created_at, title, description, assigned_to, created_by)
                         VALUES (?1, ?2, ?3, ?4, 'pending', 0, ?5, ?6, ?7, ?8, ?9)",
                        rusqlite::params![inserted_id, &created_by, &title, b"", now, title, description, assigned_to, created_by],
                    )
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                    Ok::<_, SiliCrewError>(id)
                })
                .await
                .map_err(|e| SiliCrewError::Internal(e.to_string()))?
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let inserted_id = id.clone();
                sqlx::query(
                    "INSERT INTO task_queue (id, agent_id, task_type, payload, status, priority, created_at, title, description, assigned_to, created_by)
                     VALUES ($1, $2, $3, $4, 'pending', 0, $5, $6, $7, $8, $9)",
                )
                .bind(inserted_id)
                .bind(created_by.clone())
                .bind(title.clone())
                .bind(Vec::<u8>::new())
                .bind(now)
                .bind(title)
                .bind(description)
                .bind(assigned_to)
                .bind(created_by)
                .execute(&*pool)
                .await
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                Ok(id)
            }
        }
    }

    /// Claim the next pending task (optionally for a specific assignee). Returns task JSON or None.
    pub async fn task_claim(&self, agent_id: &str) -> SiliCrewResult<Option<serde_json::Value>> {
        let agent_id = agent_id.to_string();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = Arc::clone(conn);
                tokio::task::spawn_blocking(move || {
                    let db = conn.lock().map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                    let mut stmt = db.prepare(
                        "SELECT id, title, description, assigned_to, created_by, created_at
                         FROM task_queue
                         WHERE status = 'pending' AND (assigned_to = ?1 OR assigned_to = '')
                         ORDER BY priority DESC, created_at ASC
                         LIMIT 1"
                    ).map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                    let result = stmt.query_row(rusqlite::params![agent_id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                        ))
                    });
                    match result {
                        Ok((id, title, description, assigned, created_by, created_at)) => {
                            db.execute(
                                "UPDATE task_queue SET status = 'in_progress', assigned_to = ?2 WHERE id = ?1",
                                rusqlite::params![id, agent_id],
                            ).map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                            Ok(Some(task_json(id, title, description, assigned, created_by, created_at, "in_progress", None)))
                        }
                        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                        Err(e) => Err(SiliCrewError::Memory(e.to_string())),
                    }
                })
                .await
                .map_err(|e| SiliCrewError::Internal(e.to_string()))?
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let row = sqlx::query(
                    "SELECT id, title, description, assigned_to, created_by, created_at
                     FROM task_queue
                     WHERE status = 'pending' AND (assigned_to = $1 OR assigned_to = '')
                     ORDER BY priority DESC, created_at ASC
                     LIMIT 1",
                )
                .bind(&agent_id)
                .fetch_optional(&*pool)
                .await
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                match row {
                    Some(row) => {
                        let id: String = row
                            .try_get("id")
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                        let title: String = row
                            .try_get("title")
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                        let description: String = row
                            .try_get("description")
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                        let assigned: String = row.try_get("assigned_to").unwrap_or_default();
                        let created_by: String = row.try_get("created_by").unwrap_or_default();
                        let created_at: String = row.try_get("created_at").unwrap_or_default();
                        sqlx::query(
                            "UPDATE task_queue SET status = 'in_progress', assigned_to = $2 WHERE id = $1",
                        )
                        .bind(&id)
                        .bind(&agent_id)
                        .execute(&*pool)
                        .await
                        .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                        Ok(Some(task_json(
                            id,
                            title,
                            description,
                            assigned,
                            created_by,
                            created_at,
                            "in_progress",
                            Some(agent_id),
                        )))
                    }
                    None => Ok(None),
                }
            }
        }
    }

    /// Mark a task as completed with a result string.
    pub async fn task_complete(&self, task_id: &str, result: &str) -> SiliCrewResult<()> {
        let task_id = task_id.to_string();
        let result = result.to_string();
        let now = chrono::Utc::now().to_rfc3339();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = Arc::clone(conn);
                tokio::task::spawn_blocking(move || {
                    let db = conn.lock().map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                    let rows = db.execute(
                        "UPDATE task_queue SET status = 'completed', result = ?2, completed_at = ?3 WHERE id = ?1",
                        rusqlite::params![task_id, result, now],
                    ).map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                    if rows == 0 {
                        return Err(SiliCrewError::Internal(format!("Task not found: {task_id}")));
                    }
                    Ok(())
                })
                .await
                .map_err(|e| SiliCrewError::Internal(e.to_string()))?
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let rows = sqlx::query(
                    "UPDATE task_queue SET status = 'completed', result = $2, completed_at = $3 WHERE id = $1",
                )
                .bind(&task_id)
                .bind(&result)
                .bind(&now)
                .execute(&*pool)
                .await
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                if rows.rows_affected() == 0 {
                    Err(SiliCrewError::Internal(format!(
                        "Task not found: {task_id}"
                    )))
                } else {
                    Ok(())
                }
            }
        }
    }

    /// List tasks, optionally filtered by status.
    pub async fn task_list(&self, status: Option<&str>) -> SiliCrewResult<Vec<serde_json::Value>> {
        let status = status.map(|s| s.to_string());
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = Arc::clone(conn);
                tokio::task::spawn_blocking(move || {
                    let db = conn.lock().map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                    let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match &status {
                        Some(s) => (
                            "SELECT id, title, description, status, assigned_to, created_by, created_at, completed_at, result FROM task_queue WHERE status = ?1 ORDER BY created_at DESC",
                            vec![Box::new(s.clone())],
                        ),
                        None => (
                            "SELECT id, title, description, status, assigned_to, created_by, created_at, completed_at, result FROM task_queue ORDER BY created_at DESC",
                            vec![],
                        ),
                    };
                    let mut stmt = db.prepare(sql).map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                    let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
                    let rows = stmt.query_map(params_refs.as_slice(), |row| {
                        Ok(task_list_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1).unwrap_or_default(),
                            row.get::<_, String>(2).unwrap_or_default(),
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4).unwrap_or_default(),
                            row.get::<_, String>(5).unwrap_or_default(),
                            row.get::<_, String>(6).unwrap_or_default(),
                            row.get::<_, Option<String>>(7).unwrap_or(None),
                            row.get::<_, Option<String>>(8).unwrap_or(None),
                        ))
                    }).map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                    let mut tasks = Vec::new();
                    for row in rows {
                        tasks.push(row.map_err(|e| SiliCrewError::Memory(e.to_string()))?);
                    }
                    Ok(tasks)
                })
                .await
                .map_err(|e| SiliCrewError::Internal(e.to_string()))?
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let rows = match status {
                    Some(status) => sqlx::query(
                        "SELECT id, title, description, status, assigned_to, created_by, created_at, completed_at, result FROM task_queue WHERE status = $1 ORDER BY created_at DESC",
                    )
                    .bind(status)
                    .fetch_all(&*pool)
                    .await
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                    None => sqlx::query(
                        "SELECT id, title, description, status, assigned_to, created_by, created_at, completed_at, result FROM task_queue ORDER BY created_at DESC",
                    )
                    .fetch_all(&*pool)
                    .await
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                };
                let mut tasks = Vec::with_capacity(rows.len());
                for row in rows {
                    tasks.push(task_list_json(
                        row.try_get(0)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        row.try_get(1).unwrap_or_default(),
                        row.try_get(2).unwrap_or_default(),
                        row.try_get(3)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        row.try_get(4).unwrap_or_default(),
                        row.try_get(5).unwrap_or_default(),
                        row.try_get(6).unwrap_or_default(),
                        row.try_get(7).unwrap_or(None),
                        row.try_get(8).unwrap_or(None),
                    ));
                }
                Ok(tasks)
            }
        }
    }
}

fn task_json(
    id: String,
    title: String,
    description: String,
    assigned: String,
    created_by: String,
    created_at: String,
    status: &str,
    assigned_override: Option<String>,
) -> serde_json::Value {
    let assigned_to = if assigned.is_empty() {
        assigned_override.unwrap_or_default()
    } else {
        assigned
    };
    serde_json::json!({
        "id": id,
        "title": title,
        "description": description,
        "status": status,
        "assigned_to": assigned_to,
        "created_by": created_by,
        "created_at": created_at,
    })
}

fn task_list_json(
    id: String,
    title: String,
    description: String,
    status: String,
    assigned_to: String,
    created_by: String,
    created_at: String,
    completed_at: Option<String>,
    result: Option<String>,
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "title": title,
        "description": description,
        "status": status,
        "assigned_to": assigned_to,
        "created_by": created_by,
        "created_at": created_at,
        "completed_at": completed_at,
        "result": result,
    })
}

#[async_trait]
impl Memory for MemorySubstrate {
    async fn get(&self, agent_id: AgentId, key: &str) -> SiliCrewResult<Option<serde_json::Value>> {
        let store = self.structured.clone();
        let key = key.to_string();
        tokio::task::spawn_blocking(move || store.get(agent_id, &key))
            .await
            .map_err(|e| SiliCrewError::Internal(e.to_string()))?
    }

    async fn set(
        &self,
        agent_id: AgentId,
        key: &str,
        value: serde_json::Value,
    ) -> SiliCrewResult<()> {
        let store = self.structured.clone();
        let key = key.to_string();
        tokio::task::spawn_blocking(move || store.set(agent_id, &key, value))
            .await
            .map_err(|e| SiliCrewError::Internal(e.to_string()))?
    }

    async fn delete(&self, agent_id: AgentId, key: &str) -> SiliCrewResult<()> {
        let store = self.structured.clone();
        let key = key.to_string();
        tokio::task::spawn_blocking(move || store.delete(agent_id, &key))
            .await
            .map_err(|e| SiliCrewError::Internal(e.to_string()))?
    }

    async fn remember(
        &self,
        agent_id: AgentId,
        content: &str,
        source: MemorySource,
        scope: &str,
        metadata: HashMap<String, serde_json::Value>,
    ) -> SiliCrewResult<MemoryId> {
        let store = self.semantic.clone();
        let content = content.to_string();
        let scope = scope.to_string();
        tokio::task::spawn_blocking(move || {
            store.remember(agent_id, &content, source, &scope, metadata)
        })
        .await
        .map_err(|e| SiliCrewError::Internal(e.to_string()))?
    }

    async fn recall(
        &self,
        query: &str,
        limit: usize,
        filter: Option<MemoryFilter>,
    ) -> SiliCrewResult<Vec<MemoryFragment>> {
        let store = self.semantic.clone();
        let query = query.to_string();
        tokio::task::spawn_blocking(move || store.recall(&query, limit, filter))
            .await
            .map_err(|e| SiliCrewError::Internal(e.to_string()))?
    }

    async fn forget(&self, id: MemoryId) -> SiliCrewResult<()> {
        let store = self.semantic.clone();
        tokio::task::spawn_blocking(move || store.forget(id))
            .await
            .map_err(|e| SiliCrewError::Internal(e.to_string()))?
    }

    async fn add_entity(&self, entity: Entity) -> SiliCrewResult<String> {
        let store = self.knowledge.clone();
        tokio::task::spawn_blocking(move || store.add_entity(entity))
            .await
            .map_err(|e| SiliCrewError::Internal(e.to_string()))?
    }

    async fn add_relation(&self, relation: Relation) -> SiliCrewResult<String> {
        let store = self.knowledge.clone();
        tokio::task::spawn_blocking(move || store.add_relation(relation))
            .await
            .map_err(|e| SiliCrewError::Internal(e.to_string()))?
    }

    async fn query_graph(&self, pattern: GraphPattern) -> SiliCrewResult<Vec<GraphMatch>> {
        let store = self.knowledge.clone();
        tokio::task::spawn_blocking(move || store.query_graph(pattern))
            .await
            .map_err(|e| SiliCrewError::Internal(e.to_string()))?
    }

    async fn consolidate(&self) -> SiliCrewResult<ConsolidationReport> {
        let engine = self.consolidation.clone();
        tokio::task::spawn_blocking(move || engine.consolidate())
            .await
            .map_err(|e| SiliCrewError::Internal(e.to_string()))?
    }

    async fn export(&self, format: ExportFormat) -> SiliCrewResult<Vec<u8>> {
        let _ = format;
        Ok(Vec::new())
    }

    async fn import(&self, _data: &[u8], _format: ExportFormat) -> SiliCrewResult<ImportReport> {
        Ok(ImportReport {
            entities_imported: 0,
            relations_imported: 0,
            memories_imported: 0,
            errors: vec!["Import not yet implemented in Phase 1".to_string()],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_substrate_kv() {
        let substrate = MemorySubstrate::open_in_memory(0.1).unwrap();
        let agent_id = AgentId::new();
        substrate
            .set(agent_id, "key", serde_json::json!("value"))
            .await
            .unwrap();
        let val = substrate.get(agent_id, "key").await.unwrap();
        assert_eq!(val, Some(serde_json::json!("value")));
    }

    #[tokio::test]
    async fn test_substrate_remember_recall() {
        let substrate = MemorySubstrate::open_in_memory(0.1).unwrap();
        let agent_id = AgentId::new();
        substrate
            .remember(
                agent_id,
                "Rust is a great language",
                MemorySource::Conversation,
                "episodic",
                HashMap::new(),
            )
            .await
            .unwrap();
        let results = substrate.recall("Rust", 10, None).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_task_post_and_list() {
        let substrate = MemorySubstrate::open_in_memory(0.1).unwrap();
        let id = substrate
            .task_post(
                "Review code",
                "Check the auth module for issues",
                Some("auditor"),
                Some("orchestrator"),
            )
            .await
            .unwrap();
        assert!(!id.is_empty());

        let tasks = substrate.task_list(Some("pending")).await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["title"], "Review code");
        assert_eq!(tasks[0]["assigned_to"], "auditor");
        assert_eq!(tasks[0]["status"], "pending");
    }

    #[tokio::test]
    async fn test_task_claim_and_complete() {
        let substrate = MemorySubstrate::open_in_memory(0.1).unwrap();
        let task_id = substrate
            .task_post(
                "Audit endpoint",
                "Security audit the /api/login endpoint",
                Some("auditor"),
                None,
            )
            .await
            .unwrap();

        // Claim the task
        let claimed = substrate.task_claim("auditor").await.unwrap();
        assert!(claimed.is_some());
        let claimed = claimed.unwrap();
        assert_eq!(claimed["id"], task_id);
        assert_eq!(claimed["status"], "in_progress");

        // Complete the task
        substrate
            .task_complete(&task_id, "No vulnerabilities found")
            .await
            .unwrap();

        // Verify it shows as completed
        let tasks = substrate.task_list(Some("completed")).await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["result"], "No vulnerabilities found");
    }

    #[tokio::test]
    async fn test_task_claim_empty() {
        let substrate = MemorySubstrate::open_in_memory(0.1).unwrap();
        let claimed = substrate.task_claim("nobody").await.unwrap();
        assert!(claimed.is_none());
    }
}
