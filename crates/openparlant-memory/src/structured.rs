//! Structured store for key-value pairs and agent persistence.

use crate::db::{block_on, SharedDb};
use chrono::Utc;
use openparlant_types::agent::{AgentEntry, AgentId, AgentIdentity, SessionId};
use openparlant_types::error::{OpenFangError, OpenFangResult};
use sqlx::Row;
use std::sync::Arc;

/// Structured store backed by the shared SQL database for key-value operations and agent storage.
#[derive(Clone)]
pub struct StructuredStore {
    db: SharedDb,
}

fn decode_agent_entry(
    agent_id: AgentId,
    name: String,
    manifest_blob: Vec<u8>,
    state_str: String,
    created_str: String,
    session_id_str: Option<String>,
    identity_str: Option<String>,
) -> OpenFangResult<AgentEntry> {
    let manifest = rmp_serde::from_slice(&manifest_blob)
        .map_err(|e| OpenFangError::Serialization(e.to_string()))?;
    let state = serde_json::from_str(&state_str)
        .map_err(|e| OpenFangError::Serialization(e.to_string()))?;
    let created_at = chrono::DateTime::parse_from_rfc3339(&created_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let session_id = session_id_str
        .and_then(|s| uuid::Uuid::parse_str(&s).ok())
        .map(SessionId)
        .unwrap_or_else(SessionId::new);
    let identity = identity_str
        .and_then(|s| serde_json::from_str::<AgentIdentity>(&s).ok())
        .unwrap_or_default();

    Ok(AgentEntry {
        id: agent_id,
        name,
        manifest,
        state,
        mode: Default::default(),
        created_at,
        last_active: Utc::now(),
        parent: None,
        children: vec![],
        session_id,
        tags: vec![],
        identity,
        onboarding_completed: false,
        onboarding_completed_at: None,
    })
}

impl StructuredStore {
    /// Create a new structured store wrapping the given connection.
    pub fn new(db: impl Into<SharedDb>) -> Self {
        Self { db: db.into() }
    }

    /// Get a value from the key-value store.
    pub fn get(&self, agent_id: AgentId, key: &str) -> OpenFangResult<Option<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare("SELECT value FROM kv_store WHERE agent_id = ?1 AND key = ?2")
                    .map_err(|e| OpenFangError::Memory(e.to_string()))?;
                let result =
                    stmt.query_row(rusqlite::params![agent_id.0.to_string(), key], |row| {
                        let blob: Vec<u8> = row.get(0)?;
                        Ok(blob)
                    });
                match result {
                    Ok(blob) => {
                        let value: serde_json::Value = serde_json::from_slice(&blob)
                            .map_err(|e| OpenFangError::Serialization(e.to_string()))?;
                        Ok(Some(value))
                    }
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(OpenFangError::Memory(e.to_string())),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let key = key.to_string();
                block_on(async move {
                    let row =
                        sqlx::query("SELECT value FROM kv_store WHERE agent_id = $1 AND key = $2")
                            .bind(agent_id.0.to_string())
                            .bind(key)
                            .fetch_optional(&*pool)
                            .await?;

                    match row {
                        Some(row) => {
                            let blob: Vec<u8> = row.try_get("value")?;
                            let value: serde_json::Value = serde_json::from_slice(&blob)
                                .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
                            Ok::<Option<serde_json::Value>, sqlx::Error>(Some(value))
                        }
                        None => Ok(None),
                    }
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))
            }
        }
    }

    /// Set a value in the key-value store.
    pub fn set(
        &self,
        agent_id: AgentId,
        key: &str,
        value: serde_json::Value,
    ) -> OpenFangResult<()> {
        let blob =
            serde_json::to_vec(&value).map_err(|e| OpenFangError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO kv_store (agent_id, key, value, version, updated_at) VALUES (?1, ?2, ?3, 1, ?4)
                     ON CONFLICT(agent_id, key) DO UPDATE SET value = ?3, version = version + 1, updated_at = ?4",
                    rusqlite::params![agent_id.0.to_string(), key, blob, now],
                )
                .map_err(|e| OpenFangError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let key = key.to_string();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO kv_store (agent_id, key, value, version, updated_at)
                         VALUES ($1, $2, $3, 1, $4)
                         ON CONFLICT(agent_id, key) DO UPDATE
                         SET value = EXCLUDED.value,
                             version = kv_store.version + 1,
                             updated_at = EXCLUDED.updated_at",
                    )
                    .bind(agent_id.0.to_string())
                    .bind(key)
                    .bind(blob)
                    .bind(now)
                    .execute(&*pool)
                    .await
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// Delete a value from the key-value store.
    pub fn delete(&self, agent_id: AgentId, key: &str) -> OpenFangResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                conn.execute(
                    "DELETE FROM kv_store WHERE agent_id = ?1 AND key = ?2",
                    rusqlite::params![agent_id.0.to_string(), key],
                )
                .map_err(|e| OpenFangError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let key = key.to_string();
                block_on(async move {
                    sqlx::query("DELETE FROM kv_store WHERE agent_id = $1 AND key = $2")
                        .bind(agent_id.0.to_string())
                        .bind(key)
                        .execute(&*pool)
                        .await
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// List all key-value pairs for an agent.
    pub fn list_kv(&self, agent_id: AgentId) -> OpenFangResult<Vec<(String, serde_json::Value)>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare("SELECT key, value FROM kv_store WHERE agent_id = ?1 ORDER BY key")
                    .map_err(|e| OpenFangError::Memory(e.to_string()))?;
                let rows = stmt
                    .query_map(rusqlite::params![agent_id.0.to_string()], |row| {
                        let key: String = row.get(0)?;
                        let blob: Vec<u8> = row.get(1)?;
                        Ok((key, blob))
                    })
                    .map_err(|e| OpenFangError::Memory(e.to_string()))?;

                let mut pairs = Vec::new();
                for row in rows {
                    let (key, blob) = row.map_err(|e| OpenFangError::Memory(e.to_string()))?;
                    let value: serde_json::Value =
                        serde_json::from_slice(&blob).unwrap_or_else(|_| {
                            String::from_utf8(blob)
                                .map(serde_json::Value::String)
                                .unwrap_or(serde_json::Value::Null)
                        });
                    pairs.push((key, value));
                }
                Ok(pairs)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                block_on(async move {
                    let rows = sqlx::query(
                        "SELECT key, value FROM kv_store WHERE agent_id = $1 ORDER BY key",
                    )
                    .bind(agent_id.0.to_string())
                    .fetch_all(&*pool)
                    .await?;

                    Ok::<Vec<(String, serde_json::Value)>, sqlx::Error>(
                        rows.into_iter()
                            .map(|row| {
                                let key: String = row.try_get("key").unwrap_or_default();
                                let blob: Vec<u8> = row.try_get("value").unwrap_or_default();
                                let value = serde_json::from_slice(&blob).unwrap_or_else(|_| {
                                    String::from_utf8(blob)
                                        .map(serde_json::Value::String)
                                        .unwrap_or(serde_json::Value::Null)
                                });
                                (key, value)
                            })
                            .collect(),
                    )
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))
            }
        }
    }

    /// Save an agent entry to the database.
    pub fn save_agent(&self, entry: &AgentEntry) -> OpenFangResult<()> {
        let manifest_blob = rmp_serde::to_vec_named(&entry.manifest)
            .map_err(|e| OpenFangError::Serialization(e.to_string()))?;
        let state_str = serde_json::to_string(&entry.state)
            .map_err(|e| OpenFangError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();
        let identity_json = serde_json::to_string(&entry.identity)
            .map_err(|e| OpenFangError::Serialization(e.to_string()))?;
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                let _ = conn.execute(
                    "ALTER TABLE agents ADD COLUMN session_id TEXT DEFAULT ''",
                    [],
                );
                let _ = conn.execute(
                    "ALTER TABLE agents ADD COLUMN identity TEXT DEFAULT '{}'",
                    [],
                );

                conn.execute(
                    "INSERT INTO agents (id, name, manifest, state, created_at, updated_at, session_id, identity)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(id) DO UPDATE SET name = ?2, manifest = ?3, state = ?4, updated_at = ?6, session_id = ?7, identity = ?8",
                    rusqlite::params![
                        entry.id.0.to_string(),
                        entry.name,
                        manifest_blob,
                        state_str,
                        entry.created_at.to_rfc3339(),
                        now,
                        entry.session_id.0.to_string(),
                        identity_json,
                    ],
                )
                .map_err(|e| OpenFangError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let id = entry.id.0.to_string();
                let name = entry.name.clone();
                let created_at = entry.created_at.to_rfc3339();
                let session_id = entry.session_id.0.to_string();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO agents (id, name, manifest, state, created_at, updated_at, session_id, identity)
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                         ON CONFLICT(id) DO UPDATE SET
                             name = EXCLUDED.name,
                             manifest = EXCLUDED.manifest,
                             state = EXCLUDED.state,
                             updated_at = EXCLUDED.updated_at,
                             session_id = EXCLUDED.session_id,
                             identity = EXCLUDED.identity",
                    )
                    .bind(id)
                    .bind(name)
                    .bind(manifest_blob)
                    .bind(state_str)
                    .bind(created_at)
                    .bind(now)
                    .bind(session_id)
                    .bind(identity_json)
                    .execute(&*pool)
                    .await
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// Load an agent entry from the database.
    pub fn load_agent(&self, agent_id: AgentId) -> OpenFangResult<Option<AgentEntry>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;

                let mut stmt = conn
                    .prepare("SELECT id, name, manifest, state, created_at, updated_at, session_id, identity FROM agents WHERE id = ?1")
                    .or_else(|_| {
                        conn.prepare("SELECT id, name, manifest, state, created_at, updated_at, session_id FROM agents WHERE id = ?1")
                            .or_else(|_| conn.prepare("SELECT id, name, manifest, state, created_at, updated_at FROM agents WHERE id = ?1"))
                    })
                    .map_err(|e| OpenFangError::Memory(e.to_string()))?;

                let col_count = stmt.column_count();
                let result = stmt.query_row(rusqlite::params![agent_id.0.to_string()], |row| {
                    let manifest_blob: Vec<u8> = row.get(2)?;
                    let state_str: String = row.get(3)?;
                    let created_str: String = row.get(4)?;
                    let name: String = row.get(1)?;
                    let session_id_str: Option<String> = if col_count >= 7 {
                        row.get(6).ok()
                    } else {
                        None
                    };
                    let identity_str: Option<String> = if col_count >= 8 {
                        row.get(7).ok()
                    } else {
                        None
                    };
                    Ok((
                        name,
                        manifest_blob,
                        state_str,
                        created_str,
                        session_id_str,
                        identity_str,
                    ))
                });

                match result {
                    Ok((
                        name,
                        manifest_blob,
                        state_str,
                        created_str,
                        session_id_str,
                        identity_str,
                    )) => decode_agent_entry(
                        agent_id,
                        name,
                        manifest_blob,
                        state_str,
                        created_str,
                        session_id_str,
                        identity_str,
                    )
                    .map(Some),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(OpenFangError::Memory(e.to_string())),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                block_on(async move {
                    let row = sqlx::query(
                        "SELECT name, manifest, state, created_at, session_id, identity
                         FROM agents WHERE id = $1",
                    )
                    .bind(agent_id.0.to_string())
                    .fetch_optional(&*pool)
                    .await?;
                    match row {
                        Some(row) => {
                            let entry = decode_agent_entry(
                                agent_id,
                                row.try_get::<String, _>("name")?,
                                row.try_get::<Vec<u8>, _>("manifest")?,
                                row.try_get::<String, _>("state")?,
                                row.try_get::<String, _>("created_at")?,
                                row.try_get::<Option<String>, _>("session_id")?,
                                row.try_get::<Option<String>, _>("identity")?,
                            )
                            .map_err(|e| {
                                sqlx::Error::Decode(Box::new(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    e.to_string(),
                                )))
                            })?;
                            Ok(Some(entry))
                        }
                        None => Ok::<Option<AgentEntry>, sqlx::Error>(None),
                    }
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))
            }
        }
    }

    /// Remove an agent from the database.
    pub fn remove_agent(&self, agent_id: AgentId) -> OpenFangResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                conn.execute(
                    "DELETE FROM agents WHERE id = ?1",
                    rusqlite::params![agent_id.0.to_string()],
                )
                .map_err(|e| OpenFangError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                block_on(async move {
                    sqlx::query("DELETE FROM agents WHERE id = $1")
                        .bind(agent_id.0.to_string())
                        .execute(&*pool)
                        .await
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// Load all agent entries from the database.
    ///
    /// Uses lenient deserialization (via `serde_compat`) to handle schema-mismatched
    /// fields gracefully. When an agent is loaded with lenient defaults, it is
    /// automatically re-saved to upgrade the stored blob. Duplicate agent names
    /// are deduplicated (first occurrence wins).
    pub fn load_all_agents(&self) -> OpenFangResult<Vec<AgentEntry>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;

                let mut stmt = conn
                    .prepare(
                        "SELECT id, name, manifest, state, created_at, updated_at, session_id, identity FROM agents",
                    )
                    .or_else(|_| {
                        conn.prepare("SELECT id, name, manifest, state, created_at, updated_at, session_id FROM agents")
                    })
                    .or_else(|_| {
                        conn.prepare("SELECT id, name, manifest, state, created_at, updated_at FROM agents")
                    })
                    .map_err(|e| OpenFangError::Memory(e.to_string()))?;

                let col_count = stmt.column_count();
                let rows = stmt
                    .query_map([], |row| {
                        let id_str: String = row.get(0)?;
                        let name: String = row.get(1)?;
                        let manifest_blob: Vec<u8> = row.get(2)?;
                        let state_str: String = row.get(3)?;
                        let created_str: String = row.get(4)?;
                        let session_id_str: Option<String> =
                            if col_count >= 7 { row.get(6)? } else { None };
                        let identity_str: Option<String> =
                            if col_count >= 8 { row.get(7)? } else { None };
                        Ok((
                            id_str,
                            name,
                            manifest_blob,
                            state_str,
                            created_str,
                            session_id_str,
                            identity_str,
                        ))
                    })
                    .map_err(|e| OpenFangError::Memory(e.to_string()))?;

                let mut agents = Vec::new();
                let mut seen_names = std::collections::HashSet::new();
                let mut repair_queue: Vec<(String, Vec<u8>, String)> = Vec::new();

                for row in rows {
                    let (
                        id_str,
                        name,
                        manifest_blob,
                        state_str,
                        created_str,
                        session_id_str,
                        identity_str,
                    ) = match row {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::warn!("Skipping agent row with read error: {e}");
                            continue;
                        }
                    };

                    let name_lower = name.to_lowercase();
                    if !seen_names.insert(name_lower) {
                        tracing::info!(agent = %name, id = %id_str, "Skipping duplicate agent name");
                        continue;
                    }

                    let agent_id = match uuid::Uuid::parse_str(&id_str).map(AgentId) {
                        Ok(id) => id,
                        Err(e) => {
                            tracing::warn!(agent = %name, "Skipping agent with bad UUID '{id_str}': {e}");
                            continue;
                        }
                    };

                    let new_blob = match decode_agent_entry(
                        agent_id,
                        name.clone(),
                        manifest_blob.clone(),
                        state_str,
                        created_str,
                        session_id_str,
                        identity_str,
                    ) {
                        Ok(entry) => {
                            let new_blob = rmp_serde::to_vec_named(&entry.manifest)
                                .map_err(|e| OpenFangError::Serialization(e.to_string()))?;
                            if new_blob != manifest_blob {
                                tracing::info!(
                                    agent = %entry.name,
                                    id = %id_str,
                                    "Auto-repaired agent manifest (schema upgraded)"
                                );
                                repair_queue.push((
                                    id_str.clone(),
                                    new_blob.clone(),
                                    entry.name.clone(),
                                ));
                            }
                            agents.push(entry);
                            new_blob
                        }
                        Err(e) => {
                            tracing::warn!(agent = %name, id = %id_str, "Skipping agent row: {e}");
                            continue;
                        }
                    };
                    let _ = new_blob;
                }

                for (id_str, new_blob, name) in repair_queue {
                    if let Err(e) = conn.execute(
                        "UPDATE agents SET manifest = ?1 WHERE id = ?2",
                        rusqlite::params![new_blob, id_str],
                    ) {
                        tracing::warn!(agent = %name, "Failed to auto-repair agent blob: {e}");
                    }
                }

                Ok(agents)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT id, name, manifest, state, created_at, session_id, identity FROM agents",
                    )
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))?;

                let mut agents = Vec::new();
                let mut seen_names = std::collections::HashSet::new();

                for row in rows {
                    let id_str: String = row
                        .try_get("id")
                        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
                    let name: String = row
                        .try_get("name")
                        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
                    let manifest_blob: Vec<u8> = row
                        .try_get("manifest")
                        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
                    let state_str: String = row
                        .try_get("state")
                        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
                    let created_str: String = row
                        .try_get("created_at")
                        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
                    let session_id_str: Option<String> = row
                        .try_get("session_id")
                        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
                    let identity_str: Option<String> = row
                        .try_get("identity")
                        .map_err(|e| OpenFangError::Memory(e.to_string()))?;

                    let name_lower = name.to_lowercase();
                    if !seen_names.insert(name_lower) {
                        tracing::info!(agent = %name, id = %id_str, "Skipping duplicate agent name");
                        continue;
                    }

                    let agent_id = match uuid::Uuid::parse_str(&id_str).map(AgentId) {
                        Ok(id) => id,
                        Err(e) => {
                            tracing::warn!(agent = %name, "Skipping agent with bad UUID '{id_str}': {e}");
                            continue;
                        }
                    };

                    match decode_agent_entry(
                        agent_id,
                        name.clone(),
                        manifest_blob,
                        state_str,
                        created_str,
                        session_id_str,
                        identity_str,
                    ) {
                        Ok(entry) => agents.push(entry),
                        Err(e) => {
                            tracing::warn!(agent = %name, id = %id_str, "Skipping agent row: {e}");
                        }
                    }
                }

                Ok(agents)
            }
        }
    }

    /// List all agents in the database.
    pub fn list_agents(&self) -> OpenFangResult<Vec<(String, String, String)>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare("SELECT id, name, state FROM agents")
                    .map_err(|e| OpenFangError::Memory(e.to_string()))?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    })
                    .map_err(|e| OpenFangError::Memory(e.to_string()))?;
                let mut agents = Vec::new();
                for row in rows {
                    agents.push(row.map_err(|e| OpenFangError::Memory(e.to_string()))?);
                }
                Ok(agents)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                block_on(async move {
                    let rows = sqlx::query("SELECT id, name, state FROM agents")
                        .fetch_all(&*pool)
                        .await?;
                    Ok::<Vec<(String, String, String)>, sqlx::Error>(
                        rows.into_iter()
                            .map(|row| {
                                (
                                    row.try_get("id").unwrap_or_default(),
                                    row.try_get("name").unwrap_or_default(),
                                    row.try_get("state").unwrap_or_default(),
                                )
                            })
                            .collect(),
                    )
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))
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

    fn setup() -> StructuredStore {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        StructuredStore::new(Arc::new(Mutex::new(conn)))
    }

    #[test]
    fn test_kv_set_get() {
        let store = setup();
        let agent_id = AgentId::new();
        store
            .set(agent_id, "test_key", serde_json::json!("test_value"))
            .unwrap();
        let value = store.get(agent_id, "test_key").unwrap();
        assert_eq!(value, Some(serde_json::json!("test_value")));
    }

    #[test]
    fn test_kv_get_missing() {
        let store = setup();
        let agent_id = AgentId::new();
        let value = store.get(agent_id, "nonexistent").unwrap();
        assert!(value.is_none());
    }

    #[test]
    fn test_kv_delete() {
        let store = setup();
        let agent_id = AgentId::new();
        store
            .set(agent_id, "to_delete", serde_json::json!(42))
            .unwrap();
        store.delete(agent_id, "to_delete").unwrap();
        let value = store.get(agent_id, "to_delete").unwrap();
        assert!(value.is_none());
    }

    #[test]
    fn test_kv_update() {
        let store = setup();
        let agent_id = AgentId::new();
        store.set(agent_id, "key", serde_json::json!("v1")).unwrap();
        store.set(agent_id, "key", serde_json::json!("v2")).unwrap();
        let value = store.get(agent_id, "key").unwrap();
        assert_eq!(value, Some(serde_json::json!("v2")));
    }
}
