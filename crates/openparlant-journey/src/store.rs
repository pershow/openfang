use openparlant_types::control::{JourneyDefinition, JourneyId, ScopeId};
use openparlant_types::error::{OpenFangError, OpenFangResult};
use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};

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

/// SQLite-backed journey definition store.
#[derive(Clone)]
pub struct JourneyStore {
    conn: Arc<Mutex<Connection>>,
}

impl JourneyStore {
    /// Create a new journey store wrapping the shared SQLite connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert or update a journey definition.
    pub fn upsert_journey(&self, journey: &JourneyDefinition) -> OpenFangResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let trigger_config = serde_json::to_string(&journey.trigger_config)
            .map_err(|e| OpenFangError::Serialization(e.to_string()))?;
        conn.execute(
            "INSERT INTO journeys (journey_id, scope_id, name, trigger_config, completion_rule, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(journey_id) DO UPDATE SET
                scope_id = excluded.scope_id,
                name = excluded.name,
                trigger_config = excluded.trigger_config,
                completion_rule = excluded.completion_rule,
                enabled = excluded.enabled",
            params![
                journey.journey_id.0.to_string(),
                journey.scope_id.0.as_str(),
                journey.name.as_str(),
                trigger_config,
                journey.completion_rule.as_deref(),
                journey.enabled as i64,
            ],
        )
        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(())
    }

    /// Fetch a journey definition by ID.
    pub async fn get_journey(&self, journey_id: &JourneyId) -> OpenFangResult<Option<JourneyDefinition>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT journey_id, scope_id, name, trigger_config, completion_rule, enabled
                 FROM journeys WHERE journey_id = ?1",
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        let row = stmt.query_row(params![journey_id.0.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
            ))
        });

        match row {
            Ok(row) => Ok(Some(journey_from_row(row)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(OpenFangError::Memory(e.to_string())),
        }
    }

    /// List journeys for a scope.
    pub async fn list_journeys(
        &self,
        scope_id: &ScopeId,
        enabled_only: bool,
    ) -> OpenFangResult<Vec<JourneyDefinition>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let sql = if enabled_only {
            "SELECT journey_id, scope_id, name, trigger_config, completion_rule, enabled
             FROM journeys
             WHERE scope_id = ?1 AND enabled = 1
             ORDER BY name ASC"
        } else {
            "SELECT journey_id, scope_id, name, trigger_config, completion_rule, enabled
             FROM journeys
             WHERE scope_id = ?1
             ORDER BY name ASC"
        };
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        let rows = stmt
            .query_map(params![scope_id.0.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;

        let mut journeys = Vec::new();
        for row in rows {
            journeys.push(journey_from_row(
                row.map_err(|e| OpenFangError::Memory(e.to_string()))?,
            )?);
        }
        Ok(journeys)
    }

    /// Synchronous variant of `get_journey` — usable outside an async context.
    pub fn get_journey_sync(&self, journey_id: &JourneyId) -> OpenFangResult<Option<JourneyDefinition>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT journey_id, scope_id, name, trigger_config, completion_rule, enabled
                 FROM journeys WHERE journey_id = ?1",
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        let row = stmt.query_row(params![journey_id.0.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
            ))
        });
        match row {
            Ok(row) => Ok(Some(journey_from_row(row)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(OpenFangError::Memory(e.to_string())),
        }
    }

    /// Synchronous variant of `list_journeys` — usable outside an async context.
    pub fn list_journeys_sync(
        &self,
        scope_id: &ScopeId,
        enabled_only: bool,
    ) -> OpenFangResult<Vec<JourneyDefinition>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let sql = if enabled_only {
            "SELECT journey_id, scope_id, name, trigger_config, completion_rule, enabled
             FROM journeys WHERE scope_id = ?1 AND enabled = 1 ORDER BY name ASC"
        } else {
            "SELECT journey_id, scope_id, name, trigger_config, completion_rule, enabled
             FROM journeys WHERE scope_id = ?1 ORDER BY name ASC"
        };
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        let rows = stmt
            .query_map(params![scope_id.0.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        let mut journeys = Vec::new();
        for row in rows {
            journeys.push(journey_from_row(
                row.map_err(|e| OpenFangError::Memory(e.to_string()))?,
            )?);
        }
        Ok(journeys)
    }

    pub async fn list_active_instances(
        &self,
        scope_id: &ScopeId,
    ) -> OpenFangResult<Vec<JourneyInstanceRecord>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT journey_instance_id, journey_id, current_state_id, state_payload
                 FROM journey_instances
                 WHERE scope_id = ?1 AND status = 'active'
                 ORDER BY updated_at DESC",
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        let rows = stmt
            .query_map(params![scope_id.0.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3).unwrap_or_else(|_| "{}".to_string()),
                ))
            })
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;

        let mut instances = Vec::new();
        for row in rows {
            let (instance_id, journey_id_str, state_id, state_payload) =
                row.map_err(|e| OpenFangError::Memory(e.to_string()))?;
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

    pub async fn get_state(&self, state_id: &str) -> OpenFangResult<Option<JourneyStateRecord>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT state_id, name, description, required_fields,
                        COALESCE(guideline_actions_json, '[]')
                 FROM journey_states WHERE state_id = ?1",
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
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
            Err(e) => Err(OpenFangError::Memory(e.to_string())),
        }
    }

    pub async fn get_first_state_for_journey(
        &self,
        journey_id: &JourneyId,
    ) -> OpenFangResult<Option<JourneyStateRecord>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT state_id, name, description, required_fields,
                        COALESCE(guideline_actions_json, '[]')
                 FROM journey_states
                 WHERE journey_id = ?1
                 ORDER BY rowid ASC
                 LIMIT 1",
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
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
                    required_fields: serde_json::from_str(&required_fields_json).unwrap_or_default(),
                    guideline_actions: serde_json::from_str(&actions_json).unwrap_or_default(),
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(OpenFangError::Memory(e.to_string())),
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
    ) -> OpenFangResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
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
        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(())
    }

    /// Mark a journey instance as completed or abandoned.
    pub fn set_instance_status(
        &self,
        instance_id: &str,
        status: &str,
    ) -> OpenFangResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        conn.execute(
            "UPDATE journey_instances SET status = ?1, updated_at = datetime('now')
             WHERE journey_instance_id = ?2",
            params![status, instance_id],
        )
        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(())
    }

    /// Advance an instance to a new state.
    pub fn advance_instance(
        &self,
        instance_id: &str,
        new_state_id: &str,
    ) -> OpenFangResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        conn.execute(
            "UPDATE journey_instances
             SET current_state_id = ?1, updated_at = datetime('now')
             WHERE journey_instance_id = ?2",
            params![new_state_id, instance_id],
        )
        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(())
    }

    /// List active instances for a specific session.
    pub async fn list_active_instances_for_session(
        &self,
        session_id: &str,
    ) -> OpenFangResult<Vec<ActiveJourneyInstanceRecord>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT journey_instance_id, scope_id, journey_id, current_state_id, state_payload
                 FROM journey_instances
                 WHERE session_id = ?1 AND status = 'active'
                 ORDER BY updated_at DESC",
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
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
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            let (iid, sid, jid_str, state_id, state_payload) =
                row.map_err(|e| OpenFangError::Memory(e.to_string()))?;
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

    /// List outgoing transitions from a state.
    pub fn list_transitions_from(
        &self,
        journey_id: &JourneyId,
        from_state_id: &str,
    ) -> OpenFangResult<Vec<TransitionRecord>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT transition_id, to_state_id, condition_config, transition_type
                 FROM journey_transitions
                 WHERE journey_id = ?1 AND from_state_id = ?2",
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        let rows = stmt
            .query_map(
                params![journey_id.0.to_string(), from_state_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            let (tid, to_sid, cond, ttype) =
                row.map_err(|e| OpenFangError::Memory(e.to_string()))?;
            out.push(TransitionRecord {
                transition_id: tid,
                to_state_id: to_sid,
                condition_config: serde_json::from_str(&cond).unwrap_or_default(),
                transition_type: ttype,
            });
        }
        Ok(out)
    }
}

fn journey_from_row(
    row: (String, String, String, String, Option<String>, i64),
) -> OpenFangResult<JourneyDefinition> {
    Ok(JourneyDefinition {
        journey_id: parse_uuid(&row.0)
            .map(JourneyId)
            .map_err(memory_parse_error)?,
        scope_id: ScopeId::from(row.1),
        name: row.2,
        trigger_config: serde_json::from_str(&row.3)
            .map_err(|e| OpenFangError::Serialization(e.to_string()))?,
        completion_rule: row.4,
        enabled: row.5 != 0,
    })
}

fn parse_uuid(value: &str) -> Result<uuid::Uuid, uuid::Error> {
    uuid::Uuid::parse_str(value)
}

fn memory_parse_error<E: std::fmt::Display>(error: E) -> OpenFangError {
    OpenFangError::Memory(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openparlant_memory::migration::run_migrations;
    use serde_json::json;

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
            enabled: true,
        };
        let disabled = JourneyDefinition {
            journey_id: JourneyId::new(),
            scope_id: scope_id.clone(),
            name: "legacy_flow".to_string(),
            trigger_config: json!({ "contains": ["legacy"] }),
            completion_rule: None,
            enabled: false,
        };

        store.upsert_journey(&journey).unwrap();
        store.upsert_journey(&disabled).unwrap();

        let loaded = store.get_journey(&journey.journey_id).await.unwrap().unwrap();
        let enabled = store.list_journeys(&scope_id, true).await.unwrap();
        let all = store.list_journeys(&scope_id, false).await.unwrap();

        assert_eq!(loaded.name, "sales_qualification");
        assert_eq!(loaded.trigger_config["contains"][0], "quote");
        assert_eq!(enabled.len(), 1);
        assert_eq!(all.len(), 2);
    }
}
