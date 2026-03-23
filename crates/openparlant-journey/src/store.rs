use chrono::Utc;
use silicrew_memory::db::{block_on, SharedDb};
use silicrew_types::control::{JourneyDefinition, JourneyId, ScopeId};
use silicrew_types::error::{SiliCrewError, SiliCrewResult};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::sync::Arc;

/// A journey transition row returned from the store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionRecord {
    pub transition_id: String,
    pub to_state_id: String,
    pub condition_config: serde_json::Value,
    pub transition_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JourneyStateRecord {
    pub state_id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required_fields: Vec<String>,
    /// Optional guideline action texts projected by this journey state.
    /// These are injected as active guidelines when this state is current.
    #[serde(default)]
    pub guideline_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JourneyInstanceRecord {
    pub journey_instance_id: String,
    pub journey_id: JourneyId,
    pub current_state_id: String,
    #[serde(default)]
    pub state_payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveJourneyInstanceRecord {
    pub journey_instance_id: String,
    pub scope_id: ScopeId,
    pub journey_id: JourneyId,
    pub current_state_id: String,
    #[serde(default)]
    pub state_payload: serde_json::Value,
}

/// Journey definition store backed by the shared SQL database.
#[derive(Clone)]
pub struct JourneyStore {
    db: SharedDb,
}

impl JourneyStore {
    /// Create a new journey store wrapping the shared database handle.
    pub fn new(db: impl Into<SharedDb>) -> Self {
        Self { db: db.into() }
    }

    /// Insert or update a journey definition.
    pub fn upsert_journey(&self, journey: &JourneyDefinition) -> SiliCrewResult<()> {
        let trigger_config = serde_json::to_string(&journey.trigger_config)
            .map_err(|e| SiliCrewError::Serialization(e.to_string()))?;
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO journeys (journey_id, scope_id, name, trigger_config, completion_rule, entry_state_id, enabled)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(journey_id) DO UPDATE SET
                        scope_id = excluded.scope_id,
                        name = excluded.name,
                        trigger_config = excluded.trigger_config,
                        completion_rule = excluded.completion_rule,
                        entry_state_id = excluded.entry_state_id,
                        enabled = excluded.enabled",
                    params![
                        journey.journey_id.0.to_string(),
                        journey.scope_id.0.as_str(),
                        journey.name.as_str(),
                        trigger_config,
                        journey.completion_rule.as_deref(),
                        journey.entry_state_id.as_deref(),
                        journey.enabled as i64,
                    ],
                )
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let journey_id = journey.journey_id.0.to_string();
                let scope_id = journey.scope_id.0.clone();
                let name = journey.name.clone();
                let completion_rule = journey.completion_rule.clone();
                let entry_state_id = journey.entry_state_id.clone();
                let enabled = journey.enabled;
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO journeys (journey_id, scope_id, name, trigger_config, completion_rule, entry_state_id, enabled)
                         VALUES ($1, $2, $3, $4, $5, $6, $7)
                         ON CONFLICT(journey_id) DO UPDATE SET
                            scope_id = EXCLUDED.scope_id,
                            name = EXCLUDED.name,
                            trigger_config = EXCLUDED.trigger_config,
                            completion_rule = EXCLUDED.completion_rule,
                            entry_state_id = EXCLUDED.entry_state_id,
                            enabled = EXCLUDED.enabled",
                    )
                    .bind(journey_id)
                    .bind(scope_id)
                    .bind(name)
                    .bind(trigger_config)
                    .bind(completion_rule)
                    .bind(entry_state_id)
                    .bind(enabled)
                    .execute(&*pool)
                    .await
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// Set or clear the explicit entry state for a journey.
    pub fn set_entry_state(
        &self,
        journey_id: &JourneyId,
        entry_state_id: Option<&str>,
    ) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                if let Some(entry_state_id) = entry_state_id {
                    let exists: i64 = conn
                        .query_row(
                            "SELECT COUNT(*) FROM journey_states WHERE journey_id = ?1 AND state_id = ?2",
                            params![journey_id.0.to_string(), entry_state_id],
                            |row| row.get(0),
                        )
                        .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                    if exists == 0 {
                        return Err(SiliCrewError::Memory(format!(
                            "entry state {entry_state_id} does not belong to journey {journey_id}"
                        )));
                    }
                }
                conn.execute(
                    "UPDATE journeys SET entry_state_id = ?1 WHERE journey_id = ?2",
                    params![entry_state_id, journey_id.0.to_string()],
                )
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let journey_id = journey_id.0.to_string();
                let entry_state_id = entry_state_id.map(ToOwned::to_owned);
                block_on(async move {
                    if let Some(ref entry_state_id) = entry_state_id {
                        let exists = sqlx::query(
                            "SELECT 1
                             FROM journey_states
                             WHERE journey_id = $1 AND state_id = $2
                             LIMIT 1",
                        )
                        .bind(&journey_id)
                        .bind(entry_state_id)
                        .fetch_optional(&*pool)
                        .await?;
                        if exists.is_none() {
                            return Err(sqlx::Error::RowNotFound);
                        }
                    }
                    sqlx::query("UPDATE journeys SET entry_state_id = $1 WHERE journey_id = $2")
                        .bind(entry_state_id)
                        .bind(journey_id)
                        .execute(&*pool)
                        .await
                })
                .map_err(|e| match e {
                    sqlx::Error::RowNotFound => SiliCrewError::Memory(
                        "entry state does not belong to the target journey".to_string(),
                    ),
                    other => SiliCrewError::Memory(other.to_string()),
                })?;
            }
        }
        Ok(())
    }

    /// Fetch a journey definition by ID.
    pub async fn get_journey(
        &self,
        journey_id: &JourneyId,
    ) -> SiliCrewResult<Option<JourneyDefinition>> {
        self.get_journey_sync(journey_id)
    }

    /// List journeys for a scope.
    pub async fn list_journeys(
        &self,
        scope_id: &ScopeId,
        enabled_only: bool,
    ) -> SiliCrewResult<Vec<JourneyDefinition>> {
        self.list_journeys_sync(scope_id, enabled_only)
    }

    /// Synchronous variant of `get_journey` — usable outside an async context.
    pub fn get_journey_sync(
        &self,
        journey_id: &JourneyId,
    ) -> SiliCrewResult<Option<JourneyDefinition>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT journey_id, scope_id, name, trigger_config, completion_rule, entry_state_id, enabled
                         FROM journeys WHERE journey_id = ?1",
                    )
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let row = stmt.query_row(params![journey_id.0.to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, i64>(6)?,
                    ))
                });
                match row {
                    Ok(row) => Ok(Some(journey_from_row(row)?)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(SiliCrewError::Memory(e.to_string())),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let journey_id = journey_id.0.to_string();
                block_on(async move {
                    let row = sqlx::query(
                        "SELECT journey_id, scope_id, name, trigger_config, completion_rule, entry_state_id, enabled
                         FROM journeys WHERE journey_id = $1",
                    )
                    .bind(journey_id)
                    .fetch_optional(&*pool)
                    .await?;

                    match row {
                        Some(row) => Ok::<Option<JourneyDefinition>, sqlx::Error>(Some(
                            journey_from_row((
                                row.try_get("journey_id")?,
                                row.try_get("scope_id")?,
                                row.try_get("name")?,
                                row.try_get("trigger_config")?,
                                row.try_get("completion_rule")?,
                                row.try_get("entry_state_id")?,
                                i64::from(row.try_get::<bool, _>("enabled")?),
                            ))
                            .map_err(|e| {
                                sqlx::Error::Decode(Box::new(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    e.to_string(),
                                )))
                            })?,
                        )),
                        None => Ok(None),
                    }
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))
            }
        }
    }

    /// Synchronous variant of `list_journeys` — usable outside an async context.
    pub fn list_journeys_sync(
        &self,
        scope_id: &ScopeId,
        enabled_only: bool,
    ) -> SiliCrewResult<Vec<JourneyDefinition>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let sql = if enabled_only {
                    "SELECT journey_id, scope_id, name, trigger_config, completion_rule, entry_state_id, enabled
                     FROM journeys WHERE scope_id = ?1 AND enabled = 1 ORDER BY name ASC"
                } else {
                    "SELECT journey_id, scope_id, name, trigger_config, completion_rule, entry_state_id, enabled
                     FROM journeys WHERE scope_id = ?1 ORDER BY name ASC"
                };
                let mut stmt = conn
                    .prepare(sql)
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let rows = stmt
                    .query_map(params![scope_id.0.as_str()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, i64>(6)?,
                        ))
                    })
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let mut journeys = Vec::new();
                for row in rows {
                    journeys.push(journey_from_row(
                        row.map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                    )?);
                }
                Ok(journeys)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.0.clone();
                block_on(async move {
                    let sql = if enabled_only {
                        "SELECT journey_id, scope_id, name, trigger_config, completion_rule, entry_state_id, enabled
                         FROM journeys WHERE scope_id = $1 AND enabled = TRUE ORDER BY name ASC"
                    } else {
                        "SELECT journey_id, scope_id, name, trigger_config, completion_rule, entry_state_id, enabled
                         FROM journeys WHERE scope_id = $1 ORDER BY name ASC"
                    };
                    let rows = sqlx::query(sql).bind(scope_id).fetch_all(&*pool).await?;
                    let mut journeys = Vec::with_capacity(rows.len());
                    for row in rows {
                        journeys.push(
                            journey_from_row((
                                row.try_get("journey_id")?,
                                row.try_get("scope_id")?,
                                row.try_get("name")?,
                                row.try_get("trigger_config")?,
                                row.try_get("completion_rule")?,
                                row.try_get("entry_state_id")?,
                                i64::from(row.try_get::<bool, _>("enabled")?),
                            ))
                            .map_err(|e| {
                                sqlx::Error::Decode(Box::new(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    e.to_string(),
                                )))
                            })?,
                        );
                    }
                    Ok::<Vec<JourneyDefinition>, sqlx::Error>(journeys)
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))
            }
        }
    }

    /// Delete a journey and its dependent states, transitions, instances, and binding references.
    pub fn delete_journey(&self, journey_id: &JourneyId) -> SiliCrewResult<bool> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "UPDATE session_bindings
                     SET active_journey_instance_id = NULL
                     WHERE active_journey_instance_id IN (
                        SELECT journey_instance_id FROM journey_instances WHERE journey_id = ?1
                     )",
                    params![journey_id.0.to_string()],
                )
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                conn.execute(
                    "DELETE FROM journey_instances WHERE journey_id = ?1",
                    params![journey_id.0.to_string()],
                )
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                conn.execute(
                    "DELETE FROM journey_transitions WHERE journey_id = ?1",
                    params![journey_id.0.to_string()],
                )
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                conn.execute(
                    "DELETE FROM journey_states WHERE journey_id = ?1",
                    params![journey_id.0.to_string()],
                )
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let rows = conn
                    .execute(
                        "DELETE FROM journeys WHERE journey_id = ?1",
                        params![journey_id.0.to_string()],
                    )
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                Ok(rows > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let journey_id_str = journey_id.0.to_string();
                block_on(async move {
                    sqlx::query(
                        "UPDATE session_bindings
                         SET active_journey_instance_id = NULL
                         WHERE active_journey_instance_id IN (
                            SELECT journey_instance_id FROM journey_instances WHERE journey_id = $1
                         )",
                    )
                    .bind(&journey_id_str)
                    .execute(&*pool)
                    .await?;
                    sqlx::query("DELETE FROM journey_instances WHERE journey_id = $1")
                        .bind(&journey_id_str)
                        .execute(&*pool)
                        .await?;
                    sqlx::query("DELETE FROM journey_transitions WHERE journey_id = $1")
                        .bind(&journey_id_str)
                        .execute(&*pool)
                        .await?;
                    sqlx::query("DELETE FROM journey_states WHERE journey_id = $1")
                        .bind(&journey_id_str)
                        .execute(&*pool)
                        .await?;
                    sqlx::query("DELETE FROM journeys WHERE journey_id = $1")
                        .bind(&journey_id_str)
                        .execute(&*pool)
                        .await
                })
                .map(|rows| rows.rows_affected() > 0)
                .map_err(|e| SiliCrewError::Memory(e.to_string()))
            }
        }
    }

    pub async fn list_active_instances(
        &self,
        scope_id: &ScopeId,
    ) -> SiliCrewResult<Vec<JourneyInstanceRecord>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT journey_instance_id, journey_id, current_state_id, state_payload
                         FROM journey_instances
                         WHERE scope_id = ?1 AND status = 'active'
                         ORDER BY updated_at DESC",
                    )
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let rows = stmt
                    .query_map(params![scope_id.0.as_str()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3).unwrap_or_else(|_| "{}".to_string()),
                        ))
                    })
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;

                let mut instances = Vec::new();
                for row in rows {
                    let (instance_id, journey_id_str, state_id, state_payload) =
                        row.map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                    instances.push(JourneyInstanceRecord {
                        journey_instance_id: instance_id,
                        journey_id: parse_uuid(&journey_id_str)
                            .map(JourneyId)
                            .map_err(memory_parse_error)?,
                        current_state_id: state_id,
                        state_payload: serde_json::from_str(&state_payload).unwrap_or_default(),
                    });
                }
                Ok(instances)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.0.clone();
                block_on(async move {
                    let rows = sqlx::query(
                        "SELECT journey_instance_id, journey_id, current_state_id, state_payload
                         FROM journey_instances
                         WHERE scope_id = $1 AND status = 'active'
                         ORDER BY updated_at DESC",
                    )
                    .bind(scope_id)
                    .fetch_all(&*pool)
                    .await?;

                    let mut instances = Vec::with_capacity(rows.len());
                    for row in rows {
                        let journey_id_str: String = row.try_get("journey_id")?;
                        let state_payload: String = row.try_get("state_payload")?;
                        instances.push(JourneyInstanceRecord {
                            journey_instance_id: row.try_get("journey_instance_id")?,
                            journey_id: parse_uuid(&journey_id_str).map(JourneyId).map_err(
                                |e| {
                                    sqlx::Error::Decode(Box::new(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        e.to_string(),
                                    )))
                                },
                            )?,
                            current_state_id: row.try_get("current_state_id")?,
                            state_payload: serde_json::from_str(&state_payload).unwrap_or_default(),
                        });
                    }
                    Ok::<Vec<JourneyInstanceRecord>, sqlx::Error>(instances)
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))
            }
        }
    }

    pub async fn get_state(&self, state_id: &str) -> SiliCrewResult<Option<JourneyStateRecord>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT state_id, name, description, required_fields,
                                COALESCE(guideline_actions_json, '[]')
                         FROM journey_states WHERE state_id = ?1",
                    )
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let row = stmt.query_row(params![state_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3).unwrap_or_else(|_| "[]".to_string()),
                        row.get::<_, String>(4)?,
                    ))
                });
                match row {
                    Ok((state_id, name, description, required_fields_json, actions_json)) => {
                        let required_fields: Vec<String> =
                            serde_json::from_str(&required_fields_json).unwrap_or_default();
                        let guideline_actions: Vec<String> =
                            serde_json::from_str(&actions_json).unwrap_or_default();
                        Ok(Some(JourneyStateRecord {
                            state_id,
                            name,
                            description,
                            required_fields,
                            guideline_actions,
                        }))
                    }
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(SiliCrewError::Memory(e.to_string())),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let state_id = state_id.to_string();
                block_on(async move {
                    let row = sqlx::query(
                        "SELECT state_id, name, description, required_fields,
                                COALESCE(guideline_actions_json, '[]') AS guideline_actions_json
                         FROM journey_states WHERE state_id = $1",
                    )
                    .bind(state_id)
                    .fetch_optional(&*pool)
                    .await?;

                    match row {
                        Some(row) => {
                            let required_fields_json: String = row.try_get("required_fields")?;
                            let actions_json: String = row.try_get("guideline_actions_json")?;
                            Ok::<Option<JourneyStateRecord>, sqlx::Error>(Some(
                                JourneyStateRecord {
                                    state_id: row.try_get("state_id")?,
                                    name: row.try_get("name")?,
                                    description: row.try_get("description")?,
                                    required_fields: serde_json::from_str(&required_fields_json)
                                        .unwrap_or_default(),
                                    guideline_actions: serde_json::from_str(&actions_json)
                                        .unwrap_or_default(),
                                },
                            ))
                        }
                        None => Ok(None),
                    }
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))
            }
        }
    }

    /// Resolve the entry state for a journey.
    ///
    /// The control-plane builder models the entry state as the node with no
    /// inbound transition edges. Prefer that graph root when one exists, and
    /// only fall back to a deterministic row order when the graph is ambiguous
    /// (for example, a draft with multiple disconnected states).
    pub async fn get_first_state_for_journey(
        &self,
        journey_id: &JourneyId,
    ) -> SiliCrewResult<Option<JourneyStateRecord>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT js.state_id, js.name, js.description, js.required_fields,
                                COALESCE(js.guideline_actions_json, '[]')
                         FROM journey_states AS js
                         LEFT JOIN journeys AS j ON j.journey_id = js.journey_id
                         WHERE js.journey_id = ?1
                         ORDER BY
                            CASE
                                WHEN j.entry_state_id = js.state_id THEN 0
                                ELSE 1
                            END ASC,
                            CASE
                                WHEN EXISTS (
                                    SELECT 1
                                    FROM journey_transitions AS jt
                                    WHERE jt.journey_id = js.journey_id
                                      AND jt.to_state_id = js.state_id
                                ) THEN 1
                                ELSE 0
                            END ASC,
                            js.rowid ASC
                         LIMIT 1",
                    )
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let row = stmt.query_row(params![journey_id.0.to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3).unwrap_or_else(|_| "[]".to_string()),
                        row.get::<_, String>(4)?,
                    ))
                });
                match row {
                    Ok((state_id, name, description, required_fields_json, actions_json)) => {
                        Ok(Some(JourneyStateRecord {
                            state_id,
                            name,
                            description,
                            required_fields: serde_json::from_str(&required_fields_json)
                                .unwrap_or_default(),
                            guideline_actions: serde_json::from_str(&actions_json)
                                .unwrap_or_default(),
                        }))
                    }
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(SiliCrewError::Memory(e.to_string())),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let journey_id = journey_id.0.to_string();
                block_on(async move {
                    let row = sqlx::query(
                        "SELECT js.state_id, js.name, js.description, js.required_fields,
                                COALESCE(js.guideline_actions_json, '[]') AS guideline_actions_json
                         FROM journey_states AS js
                         LEFT JOIN journeys AS j ON j.journey_id = js.journey_id
                         WHERE js.journey_id = $1
                         ORDER BY
                            CASE
                                WHEN j.entry_state_id = js.state_id THEN 0
                                ELSE 1
                            END ASC,
                            CASE
                                WHEN EXISTS (
                                    SELECT 1
                                    FROM journey_transitions AS jt
                                    WHERE jt.journey_id = js.journey_id
                                      AND jt.to_state_id = js.state_id
                                ) THEN 1
                                ELSE 0
                            END ASC,
                            js.name ASC,
                            js.state_id ASC
                         LIMIT 1",
                    )
                    .bind(journey_id)
                    .fetch_optional(&*pool)
                    .await?;

                    match row {
                        Some(row) => {
                            let required_fields_json: String = row.try_get("required_fields")?;
                            let actions_json: String = row.try_get("guideline_actions_json")?;
                            Ok::<Option<JourneyStateRecord>, sqlx::Error>(Some(
                                JourneyStateRecord {
                                    state_id: row.try_get("state_id")?,
                                    name: row.try_get("name")?,
                                    description: row.try_get("description")?,
                                    required_fields: serde_json::from_str(&required_fields_json)
                                        .unwrap_or_default(),
                                    guideline_actions: serde_json::from_str(&actions_json)
                                        .unwrap_or_default(),
                                },
                            ))
                        }
                        None => Ok(None),
                    }
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))
            }
        }
    }

    /// Insert a journey instance (activate a journey for a session).
    pub fn upsert_journey_instance(
        &self,
        instance_id: &str,
        scope_id: &ScopeId,
        session_id: &str,
        journey_id: &JourneyId,
        current_state_id: &str,
    ) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO journey_instances (
                        journey_instance_id, scope_id, session_id,
                        journey_id, current_state_id, status, state_payload, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, 'active', '{}', datetime('now'))
                     ON CONFLICT(journey_instance_id) DO UPDATE SET
                        current_state_id = excluded.current_state_id,
                        updated_at = excluded.updated_at",
                    params![
                        instance_id,
                        scope_id.0.as_str(),
                        session_id,
                        journey_id.0.to_string(),
                        current_state_id,
                    ],
                )
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let instance_id = instance_id.to_string();
                let scope_id = scope_id.0.clone();
                let session_id = session_id.to_string();
                let journey_id = journey_id.0.to_string();
                let current_state_id = current_state_id.to_string();
                let updated_at = Utc::now().to_rfc3339();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO journey_instances (
                            journey_instance_id, scope_id, session_id,
                            journey_id, current_state_id, status, state_payload, updated_at
                         ) VALUES ($1, $2, $3, $4, $5, 'active', '{}', $6)
                         ON CONFLICT(journey_instance_id) DO UPDATE SET
                            current_state_id = EXCLUDED.current_state_id,
                            updated_at = EXCLUDED.updated_at",
                    )
                    .bind(instance_id)
                    .bind(scope_id)
                    .bind(session_id)
                    .bind(journey_id)
                    .bind(current_state_id)
                    .bind(updated_at)
                    .execute(&*pool)
                    .await
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// Mark a journey instance as completed or abandoned.
    pub fn set_instance_status(&self, instance_id: &str, status: &str) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "UPDATE journey_instances SET status = ?1, updated_at = datetime('now')
                     WHERE journey_instance_id = ?2",
                    params![status, instance_id],
                )
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let instance_id = instance_id.to_string();
                let status = status.to_string();
                let updated_at = Utc::now().to_rfc3339();
                block_on(async move {
                    sqlx::query(
                        "UPDATE journey_instances SET status = $1, updated_at = $2
                         WHERE journey_instance_id = $3",
                    )
                    .bind(status)
                    .bind(updated_at)
                    .bind(instance_id)
                    .execute(&*pool)
                    .await
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// Advance an instance to a new state.
    pub fn advance_instance(&self, instance_id: &str, new_state_id: &str) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "UPDATE journey_instances
                     SET current_state_id = ?1, updated_at = datetime('now')
                     WHERE journey_instance_id = ?2",
                    params![new_state_id, instance_id],
                )
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let instance_id = instance_id.to_string();
                let new_state_id = new_state_id.to_string();
                let updated_at = Utc::now().to_rfc3339();
                block_on(async move {
                    sqlx::query(
                        "UPDATE journey_instances
                         SET current_state_id = $1, updated_at = $2
                         WHERE journey_instance_id = $3",
                    )
                    .bind(new_state_id)
                    .bind(updated_at)
                    .bind(instance_id)
                    .execute(&*pool)
                    .await
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// List active instances for a specific session.
    pub async fn list_active_instances_for_session(
        &self,
        session_id: &str,
    ) -> SiliCrewResult<Vec<ActiveJourneyInstanceRecord>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT journey_instance_id, scope_id, journey_id, current_state_id, state_payload
                         FROM journey_instances
                         WHERE session_id = ?1 AND status = 'active'
                         ORDER BY updated_at DESC",
                    )
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let rows = stmt
                    .query_map(params![session_id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4).unwrap_or_else(|_| "{}".to_string()),
                        ))
                    })
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let mut out = Vec::new();
                for row in rows {
                    let (iid, sid, jid_str, state_id, state_payload) =
                        row.map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                    let journey_id = parse_uuid(&jid_str)
                        .map(JourneyId)
                        .map_err(memory_parse_error)?;
                    out.push(ActiveJourneyInstanceRecord {
                        journey_instance_id: iid,
                        scope_id: ScopeId::from(sid),
                        journey_id,
                        current_state_id: state_id,
                        state_payload: serde_json::from_str(&state_payload).unwrap_or_default(),
                    });
                }
                Ok(out)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let session_id = session_id.to_string();
                block_on(async move {
                    let rows = sqlx::query(
                        "SELECT journey_instance_id, scope_id, journey_id, current_state_id, state_payload
                         FROM journey_instances
                         WHERE session_id = $1 AND status = 'active'
                         ORDER BY updated_at DESC",
                    )
                    .bind(session_id)
                    .fetch_all(&*pool)
                    .await?;
                    let mut out = Vec::with_capacity(rows.len());
                    for row in rows {
                        let jid_str: String = row.try_get("journey_id")?;
                        let state_payload: String = row.try_get("state_payload")?;
                        out.push(ActiveJourneyInstanceRecord {
                            journey_instance_id: row.try_get("journey_instance_id")?,
                            scope_id: ScopeId::from(row.try_get::<String, _>("scope_id")?),
                            journey_id: parse_uuid(&jid_str)
                                .map(JourneyId)
                                .map_err(|e| {
                                    sqlx::Error::Decode(Box::new(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        e.to_string(),
                                    )))
                                })?,
                            current_state_id: row.try_get("current_state_id")?,
                            state_payload: serde_json::from_str(&state_payload).unwrap_or_default(),
                        });
                    }
                    Ok::<Vec<ActiveJourneyInstanceRecord>, sqlx::Error>(out)
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))
            }
        }
    }

    /// List outgoing transitions from a state.
    pub fn list_transitions_from(
        &self,
        journey_id: &JourneyId,
        from_state_id: &str,
    ) -> SiliCrewResult<Vec<TransitionRecord>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT transition_id, to_state_id, condition_config, transition_type
                         FROM journey_transitions
                         WHERE journey_id = ?1 AND from_state_id = ?2",
                    )
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let rows = stmt
                    .query_map(params![journey_id.0.to_string(), from_state_id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    })
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let mut out = Vec::new();
                for row in rows {
                    let (tid, to_sid, cond, ttype) =
                        row.map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                    out.push(TransitionRecord {
                        transition_id: tid,
                        to_state_id: to_sid,
                        condition_config: serde_json::from_str(&cond).unwrap_or_default(),
                        transition_type: ttype,
                    });
                }
                Ok(out)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let journey_id = journey_id.0.to_string();
                let from_state_id = from_state_id.to_string();
                block_on(async move {
                    let rows = sqlx::query(
                        "SELECT transition_id, to_state_id, condition_config, transition_type
                         FROM journey_transitions
                         WHERE journey_id = $1 AND from_state_id = $2",
                    )
                    .bind(journey_id)
                    .bind(from_state_id)
                    .fetch_all(&*pool)
                    .await?;
                    let mut out = Vec::with_capacity(rows.len());
                    for row in rows {
                        let cond: String = row.try_get("condition_config")?;
                        out.push(TransitionRecord {
                            transition_id: row.try_get("transition_id")?,
                            to_state_id: row.try_get("to_state_id")?,
                            condition_config: serde_json::from_str(&cond).unwrap_or_default(),
                            transition_type: row.try_get("transition_type")?,
                        });
                    }
                    Ok::<Vec<TransitionRecord>, sqlx::Error>(out)
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))
            }
        }
    }
}

fn journey_from_row(
    row: (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        i64,
    ),
) -> SiliCrewResult<JourneyDefinition> {
    Ok(JourneyDefinition {
        journey_id: parse_uuid(&row.0)
            .map(JourneyId)
            .map_err(memory_parse_error)?,
        scope_id: ScopeId::from(row.1),
        name: row.2,
        trigger_config: serde_json::from_str(&row.3)
            .map_err(|e| SiliCrewError::Serialization(e.to_string()))?,
        completion_rule: row.4,
        entry_state_id: row.5,
        enabled: row.6 != 0,
    })
}

fn parse_uuid(value: &str) -> Result<uuid::Uuid, uuid::Error> {
    uuid::Uuid::parse_str(value)
}

fn memory_parse_error<E: std::fmt::Display>(error: E) -> SiliCrewError {
    SiliCrewError::Memory(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use silicrew_memory::migration::run_migrations;
    use rusqlite::Connection;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn test_store() -> JourneyStore {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        JourneyStore::new(Arc::new(Mutex::new(conn)))
    }

    #[tokio::test]
    async fn journeys_round_trip_and_filter_enabled() {
        let store = test_store();
        let scope_id = ScopeId::from("default");
        let journey = JourneyDefinition {
            journey_id: JourneyId::new(),
            scope_id: scope_id.clone(),
            name: "sales_qualification".to_string(),
            trigger_config: json!({ "contains": ["quote"] }),
            completion_rule: Some("quote_sent".to_string()),
            entry_state_id: None,
            enabled: true,
        };
        let disabled = JourneyDefinition {
            journey_id: JourneyId::new(),
            scope_id: scope_id.clone(),
            name: "legacy_flow".to_string(),
            trigger_config: json!({ "contains": ["legacy"] }),
            completion_rule: None,
            entry_state_id: None,
            enabled: false,
        };

        store.upsert_journey(&journey).unwrap();
        store.upsert_journey(&disabled).unwrap();

        let loaded = store
            .get_journey(&journey.journey_id)
            .await
            .unwrap()
            .unwrap();
        let enabled = store.list_journeys(&scope_id, true).await.unwrap();
        let all = store.list_journeys(&scope_id, false).await.unwrap();

        assert_eq!(loaded.name, "sales_qualification");
        assert_eq!(loaded.trigger_config["contains"][0], "quote");
        assert_eq!(enabled.len(), 1);
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn prefers_root_state_over_lexical_state_id_order() {
        let store = test_store();
        let scope_id = ScopeId::from("default");
        let journey = JourneyDefinition {
            journey_id: JourneyId::new(),
            scope_id: scope_id.clone(),
            name: "rooted_journey".to_string(),
            trigger_config: json!({ "always": true }),
            completion_rule: None,
            entry_state_id: None,
            enabled: true,
        };

        store.upsert_journey(&journey).unwrap();

        {
            let conn = store.db.sqlite().unwrap();
            let conn = conn.lock().unwrap();
            conn.execute(
                "INSERT INTO journey_states (state_id, journey_id, name, description, required_fields, guideline_actions_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    "zzz-entry",
                    journey.journey_id.0.to_string(),
                    "Entry",
                    Option::<String>::None,
                    "[]",
                    "[]",
                ],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO journey_states (state_id, journey_id, name, description, required_fields, guideline_actions_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    "aaa-child",
                    journey.journey_id.0.to_string(),
                    "Child",
                    Option::<String>::None,
                    "[]",
                    "[]",
                ],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO journey_transitions (transition_id, journey_id, from_state_id, to_state_id, condition_config, transition_type)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    "t-root",
                    journey.journey_id.0.to_string(),
                    "zzz-entry",
                    "aaa-child",
                    "{}",
                    "auto",
                ],
            )
            .unwrap();
        }

        let state = store
            .get_first_state_for_journey(&journey.journey_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.state_id, "zzz-entry");
    }

    #[tokio::test]
    async fn explicit_entry_state_overrides_ambiguous_graph_roots() {
        let store = test_store();
        let scope_id = ScopeId::from("default");
        let journey = JourneyDefinition {
            journey_id: JourneyId::new(),
            scope_id,
            name: "explicit_entry".to_string(),
            trigger_config: json!({ "always": true }),
            completion_rule: None,
            entry_state_id: None,
            enabled: true,
        };

        store.upsert_journey(&journey).unwrap();

        {
            let conn = store.db.sqlite().unwrap();
            let conn = conn.lock().unwrap();
            conn.execute(
                "INSERT INTO journey_states (state_id, journey_id, name, description, required_fields, guideline_actions_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    "aaa-root",
                    journey.journey_id.0.to_string(),
                    "Other Root",
                    Option::<String>::None,
                    "[]",
                    "[]",
                ],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO journey_states (state_id, journey_id, name, description, required_fields, guideline_actions_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    "zzz-entry",
                    journey.journey_id.0.to_string(),
                    "Configured Entry",
                    Option::<String>::None,
                    "[]",
                    "[]",
                ],
            )
            .unwrap();
        }

        store
            .set_entry_state(&journey.journey_id, Some("zzz-entry"))
            .unwrap();

        let state = store
            .get_first_state_for_journey(&journey.journey_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.state_id, "zzz-entry");
    }

    #[test]
    fn deleting_journey_removes_states_transitions_and_instances() {
        let store = test_store();
        let scope_id = ScopeId::from("default");
        let journey = JourneyDefinition {
            journey_id: JourneyId::new(),
            scope_id: scope_id.clone(),
            name: "delete_me".to_string(),
            trigger_config: json!({ "always": true }),
            completion_rule: None,
            entry_state_id: Some("state-a".to_string()),
            enabled: true,
        };
        store.upsert_journey(&journey).unwrap();

        {
            let conn = store.db.sqlite().unwrap();
            let conn = conn.lock().unwrap();
            conn.execute(
                "INSERT INTO journey_states (state_id, journey_id, name, description, required_fields, guideline_actions_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params!["state-a", journey.journey_id.0.to_string(), "Start", Option::<String>::None, "[]", "[]"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO journey_states (state_id, journey_id, name, description, required_fields, guideline_actions_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params!["state-b", journey.journey_id.0.to_string(), "Next", Option::<String>::None, "[]", "[]"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO journey_transitions (transition_id, journey_id, from_state_id, to_state_id, condition_config, transition_type)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params!["transition-a", journey.journey_id.0.to_string(), "state-a", "state-b", "{}", "auto"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO journey_instances (journey_instance_id, scope_id, session_id, journey_id, current_state_id, status, state_payload, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'active', '{}', datetime('now'))",
                params!["instance-a", scope_id.0.as_str(), "session-a", journey.journey_id.0.to_string(), "state-a"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO session_bindings (binding_id, scope_id, channel_type, external_user_id, external_chat_id, agent_id, session_id, manual_mode, active_journey_instance_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?5, 0, ?6, datetime('now'), datetime('now'))",
                params!["binding-a", scope_id.0.as_str(), "web", uuid::Uuid::new_v4().to_string(), "session-a", "instance-a"],
            )
            .unwrap();
        }

        assert!(store.delete_journey(&journey.journey_id).unwrap());
        assert!(store
            .get_journey_sync(&journey.journey_id)
            .unwrap()
            .is_none());

        let conn = store.db.sqlite().unwrap();
        let conn = conn.lock().unwrap();
        let state_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM journey_states WHERE journey_id = ?1",
                params![journey.journey_id.0.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let transition_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM journey_transitions WHERE journey_id = ?1",
                params![journey.journey_id.0.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let instance_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM journey_instances WHERE journey_id = ?1",
                params![journey.journey_id.0.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let active_binding: Option<String> = conn
            .query_row(
                "SELECT active_journey_instance_id FROM session_bindings WHERE binding_id = ?1",
                params!["binding-a"],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(state_count, 0);
        assert_eq!(transition_count, 0);
        assert_eq!(instance_count, 0);
        assert_eq!(active_binding, None);
    }
}
