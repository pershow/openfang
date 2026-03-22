use openparlant_memory::db::{block_on, SharedDb};
use openparlant_types::control::{
    GuidelineDefinition, GuidelineId, ObservationDefinition, ObservationId, ScopeId,
    ToolExposurePolicy,
};
use openparlant_types::error::{OpenFangError, OpenFangResult};
use rusqlite::params;
use sqlx::Row;
use std::sync::Arc;

/// Policy definition store backed by the shared SQL database.
#[derive(Clone)]
pub struct PolicyStore {
    db: SharedDb,
}

impl PolicyStore {
    /// Create a new policy store wrapping the shared database handle.
    pub fn new(db: impl Into<SharedDb>) -> Self {
        Self { db: db.into() }
    }

    /// Insert or update an observation definition.
    pub fn upsert_observation(&self, observation: &ObservationDefinition) -> OpenFangResult<()> {
        let matcher_config = serde_json::to_string(&observation.matcher_config)
            .map_err(|e| OpenFangError::Serialization(e.to_string()))?;
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO observations (
                        observation_id, scope_id, name, matcher_type, matcher_config, priority, enabled
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(observation_id) DO UPDATE SET
                        scope_id = excluded.scope_id,
                        name = excluded.name,
                        matcher_type = excluded.matcher_type,
                        matcher_config = excluded.matcher_config,
                        priority = excluded.priority,
                        enabled = excluded.enabled",
                    params![
                        observation.observation_id.0.to_string(),
                        observation.scope_id.0.as_str(),
                        observation.name.as_str(),
                        observation.matcher_type.as_str(),
                        matcher_config,
                        observation.priority,
                        observation.enabled as i64,
                    ],
                )
                .map_err(|e| OpenFangError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let observation_id = observation.observation_id.0.to_string();
                let scope_id = observation.scope_id.0.clone();
                let name = observation.name.clone();
                let matcher_type = observation.matcher_type.clone();
                let priority = observation.priority;
                let enabled = observation.enabled;
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO observations (
                            observation_id, scope_id, name, matcher_type, matcher_config, priority, enabled
                         ) VALUES ($1, $2, $3, $4, $5, $6, $7)
                         ON CONFLICT(observation_id) DO UPDATE SET
                            scope_id = EXCLUDED.scope_id,
                            name = EXCLUDED.name,
                            matcher_type = EXCLUDED.matcher_type,
                            matcher_config = EXCLUDED.matcher_config,
                            priority = EXCLUDED.priority,
                            enabled = EXCLUDED.enabled",
                    )
                    .bind(observation_id)
                    .bind(scope_id)
                    .bind(name)
                    .bind(matcher_type)
                    .bind(matcher_config)
                    .bind(priority)
                    .bind(enabled)
                    .execute(&*pool)
                    .await
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// Fetch an observation definition by ID.
    pub fn get_observation(
        &self,
        observation_id: ObservationId,
    ) -> OpenFangResult<Option<ObservationDefinition>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT observation_id, scope_id, name, matcher_type, matcher_config, priority, enabled
                         FROM observations WHERE observation_id = ?1",
                    )
                    .map_err(|e| OpenFangError::Memory(e.to_string()))?;
                let row = stmt.query_row(params![observation_id.0.to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i32>(5)?,
                        row.get::<_, i64>(6)?,
                    ))
                });

                match row {
                    Ok(row) => Ok(Some(observation_from_row(row)?)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(OpenFangError::Memory(e.to_string())),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let observation_id = observation_id.0.to_string();
                block_on(async move {
                    let row = sqlx::query(
                        "SELECT observation_id, scope_id, name, matcher_type, matcher_config, priority, enabled
                         FROM observations WHERE observation_id = $1",
                    )
                    .bind(observation_id)
                    .fetch_optional(&*pool)
                    .await?;

                    match row {
                        Some(row) => Ok::<Option<ObservationDefinition>, sqlx::Error>(Some(
                            observation_from_row((
                                row.try_get("observation_id")?,
                                row.try_get("scope_id")?,
                                row.try_get("name")?,
                                row.try_get("matcher_type")?,
                                row.try_get("matcher_config")?,
                                row.try_get("priority")?,
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
                .map_err(|e| OpenFangError::Memory(e.to_string()))
            }
        }
    }

    /// List observations for a scope ordered by priority.
    pub fn list_observations(
        &self,
        scope_id: &ScopeId,
        enabled_only: bool,
    ) -> OpenFangResult<Vec<ObservationDefinition>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                let sql = if enabled_only {
                    "SELECT observation_id, scope_id, name, matcher_type, matcher_config, priority, enabled
                     FROM observations
                     WHERE scope_id = ?1 AND enabled = 1
                     ORDER BY priority DESC, name ASC"
                } else {
                    "SELECT observation_id, scope_id, name, matcher_type, matcher_config, priority, enabled
                     FROM observations
                     WHERE scope_id = ?1
                     ORDER BY priority DESC, name ASC"
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
                            row.get::<_, String>(4)?,
                            row.get::<_, i32>(5)?,
                            row.get::<_, i64>(6)?,
                        ))
                    })
                    .map_err(|e| OpenFangError::Memory(e.to_string()))?;

                let mut observations = Vec::new();
                for row in rows {
                    observations.push(observation_from_row(
                        row.map_err(|e| OpenFangError::Memory(e.to_string()))?,
                    )?);
                }
                Ok(observations)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.0.clone();
                block_on(async move {
                    let sql = if enabled_only {
                        "SELECT observation_id, scope_id, name, matcher_type, matcher_config, priority, enabled
                         FROM observations
                         WHERE scope_id = $1 AND enabled = TRUE
                         ORDER BY priority DESC, name ASC"
                    } else {
                        "SELECT observation_id, scope_id, name, matcher_type, matcher_config, priority, enabled
                         FROM observations
                         WHERE scope_id = $1
                         ORDER BY priority DESC, name ASC"
                    };
                    let rows = sqlx::query(sql).bind(scope_id).fetch_all(&*pool).await?;

                    let mut observations = Vec::with_capacity(rows.len());
                    for row in rows {
                        observations.push(
                            observation_from_row((
                                row.try_get("observation_id")?,
                                row.try_get("scope_id")?,
                                row.try_get("name")?,
                                row.try_get("matcher_type")?,
                                row.try_get("matcher_config")?,
                                row.try_get("priority")?,
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
                    Ok::<Vec<ObservationDefinition>, sqlx::Error>(observations)
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))
            }
        }
    }

    /// Insert or update a guideline definition.
    pub fn upsert_guideline(&self, guideline: &GuidelineDefinition) -> OpenFangResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO guidelines (
                        guideline_id, scope_id, name, condition_ref, action_text, composition_mode, priority, enabled
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(guideline_id) DO UPDATE SET
                        scope_id = excluded.scope_id,
                        name = excluded.name,
                        condition_ref = excluded.condition_ref,
                        action_text = excluded.action_text,
                        composition_mode = excluded.composition_mode,
                        priority = excluded.priority,
                        enabled = excluded.enabled",
                    params![
                        guideline.guideline_id.0.to_string(),
                        guideline.scope_id.0.as_str(),
                        guideline.name.as_str(),
                        guideline.condition_ref.as_str(),
                        guideline.action_text.as_str(),
                        guideline.composition_mode.as_str(),
                        guideline.priority,
                        guideline.enabled as i64,
                    ],
                )
                .map_err(|e| OpenFangError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let guideline_id = guideline.guideline_id.0.to_string();
                let scope_id = guideline.scope_id.0.clone();
                let name = guideline.name.clone();
                let condition_ref = guideline.condition_ref.clone();
                let action_text = guideline.action_text.clone();
                let composition_mode = guideline.composition_mode.clone();
                let priority = guideline.priority;
                let enabled = guideline.enabled;
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO guidelines (
                            guideline_id, scope_id, name, condition_ref, action_text, composition_mode, priority, enabled
                         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                         ON CONFLICT(guideline_id) DO UPDATE SET
                            scope_id = EXCLUDED.scope_id,
                            name = EXCLUDED.name,
                            condition_ref = EXCLUDED.condition_ref,
                            action_text = EXCLUDED.action_text,
                            composition_mode = EXCLUDED.composition_mode,
                            priority = EXCLUDED.priority,
                            enabled = EXCLUDED.enabled",
                    )
                    .bind(guideline_id)
                    .bind(scope_id)
                    .bind(name)
                    .bind(condition_ref)
                    .bind(action_text)
                    .bind(composition_mode)
                    .bind(priority)
                    .bind(enabled)
                    .execute(&*pool)
                    .await
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// Fetch a guideline definition by ID.
    pub fn get_guideline(
        &self,
        guideline_id: GuidelineId,
    ) -> OpenFangResult<Option<GuidelineDefinition>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT guideline_id, scope_id, name, condition_ref, action_text, composition_mode, priority, enabled
                         FROM guidelines WHERE guideline_id = ?1",
                    )
                    .map_err(|e| OpenFangError::Memory(e.to_string()))?;
                let row = stmt.query_row(params![guideline_id.0.to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, i32>(6)?,
                        row.get::<_, i64>(7)?,
                    ))
                });

                match row {
                    Ok(row) => Ok(Some(guideline_from_row(row)?)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(OpenFangError::Memory(e.to_string())),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let guideline_id = guideline_id.0.to_string();
                block_on(async move {
                    let row = sqlx::query(
                        "SELECT guideline_id, scope_id, name, condition_ref, action_text, composition_mode, priority, enabled
                         FROM guidelines WHERE guideline_id = $1",
                    )
                    .bind(guideline_id)
                    .fetch_optional(&*pool)
                    .await?;

                    match row {
                        Some(row) => Ok::<Option<GuidelineDefinition>, sqlx::Error>(Some(
                            guideline_from_row((
                                row.try_get("guideline_id")?,
                                row.try_get("scope_id")?,
                                row.try_get("name")?,
                                row.try_get("condition_ref")?,
                                row.try_get("action_text")?,
                                row.try_get("composition_mode")?,
                                row.try_get("priority")?,
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
                .map_err(|e| OpenFangError::Memory(e.to_string()))
            }
        }
    }

    /// List guidelines for a scope ordered by priority.
    pub fn list_guidelines(
        &self,
        scope_id: &ScopeId,
        enabled_only: bool,
    ) -> OpenFangResult<Vec<GuidelineDefinition>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                let sql = if enabled_only {
                    "SELECT guideline_id, scope_id, name, condition_ref, action_text, composition_mode, priority, enabled
                     FROM guidelines
                     WHERE scope_id = ?1 AND enabled = 1
                     ORDER BY priority DESC, name ASC"
                } else {
                    "SELECT guideline_id, scope_id, name, condition_ref, action_text, composition_mode, priority, enabled
                     FROM guidelines
                     WHERE scope_id = ?1
                     ORDER BY priority DESC, name ASC"
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
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, i32>(6)?,
                            row.get::<_, i64>(7)?,
                        ))
                    })
                    .map_err(|e| OpenFangError::Memory(e.to_string()))?;

                let mut guidelines = Vec::new();
                for row in rows {
                    guidelines.push(guideline_from_row(
                        row.map_err(|e| OpenFangError::Memory(e.to_string()))?,
                    )?);
                }
                Ok(guidelines)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.0.clone();
                block_on(async move {
                    let sql = if enabled_only {
                        "SELECT guideline_id, scope_id, name, condition_ref, action_text, composition_mode, priority, enabled
                         FROM guidelines
                         WHERE scope_id = $1 AND enabled = TRUE
                         ORDER BY priority DESC, name ASC"
                    } else {
                        "SELECT guideline_id, scope_id, name, condition_ref, action_text, composition_mode, priority, enabled
                         FROM guidelines
                         WHERE scope_id = $1
                         ORDER BY priority DESC, name ASC"
                    };
                    let rows = sqlx::query(sql).bind(scope_id).fetch_all(&*pool).await?;

                    let mut guidelines = Vec::with_capacity(rows.len());
                    for row in rows {
                        guidelines.push(
                            guideline_from_row((
                                row.try_get("guideline_id")?,
                                row.try_get("scope_id")?,
                                row.try_get("name")?,
                                row.try_get("condition_ref")?,
                                row.try_get("action_text")?,
                                row.try_get("composition_mode")?,
                                row.try_get("priority")?,
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
                    Ok::<Vec<GuidelineDefinition>, sqlx::Error>(guidelines)
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))
            }
        }
    }

    // ── Guideline Relationships ───────────────────────────────────────────────

    /// List all guideline relationships for a scope.
    /// Returns tuples of `(relationship_id, from_guideline_id, to_guideline_id, relation_type)`.
    pub fn list_guideline_relationships(
        &self,
        scope_id: &ScopeId,
    ) -> OpenFangResult<Vec<(String, String, String, String)>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT relationship_id, from_guideline_id, to_guideline_id, relation_type
                         FROM guideline_relationships
                         WHERE scope_id = ?1",
                    )
                    .map_err(|e| OpenFangError::Memory(e.to_string()))?;
                let rows = stmt
                    .query_map(params![scope_id.0.as_str()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    })
                    .map_err(|e| OpenFangError::Memory(e.to_string()))?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row.map_err(|e| OpenFangError::Memory(e.to_string()))?);
                }
                Ok(out)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.0.clone();
                block_on(async move {
                    let rows = sqlx::query(
                        "SELECT relationship_id, from_guideline_id, to_guideline_id, relation_type
                         FROM guideline_relationships
                         WHERE scope_id = $1",
                    )
                    .bind(scope_id)
                    .fetch_all(&*pool)
                    .await?;

                    rows.into_iter()
                        .map(|row| {
                            Ok((
                                row.try_get("relationship_id")?,
                                row.try_get("from_guideline_id")?,
                                row.try_get("to_guideline_id")?,
                                row.try_get("relation_type")?,
                            ))
                        })
                        .collect::<Result<Vec<_>, sqlx::Error>>()
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))
            }
        }
    }

    // ── Tool Exposure Policies ────────────────────────────────────────────────

    /// Insert a new tool-exposure policy.
    pub fn upsert_tool_exposure_policy(&self, policy: &ToolExposurePolicy) -> OpenFangResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO tool_exposure_policies
                        (policy_id, scope_id, tool_name, skill_ref, observation_ref,
                         journey_state_ref, guideline_ref, approval_mode, enabled)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                     ON CONFLICT(policy_id) DO UPDATE SET
                        tool_name = excluded.tool_name,
                        skill_ref = excluded.skill_ref,
                        observation_ref = excluded.observation_ref,
                        journey_state_ref = excluded.journey_state_ref,
                        guideline_ref = excluded.guideline_ref,
                        approval_mode = excluded.approval_mode,
                        enabled = excluded.enabled",
                    params![
                        policy.policy_id,
                        policy.scope_id.0.as_str(),
                        policy.tool_name,
                        policy.skill_ref,
                        policy.observation_ref,
                        policy.journey_state_ref,
                        policy.guideline_ref,
                        policy.approval_mode.as_str(),
                        policy.enabled as i64,
                    ],
                )
                .map_err(|e| OpenFangError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let policy_id = policy.policy_id.clone();
                let scope_id = policy.scope_id.0.clone();
                let tool_name = policy.tool_name.clone();
                let skill_ref = policy.skill_ref.clone();
                let observation_ref = policy.observation_ref.clone();
                let journey_state_ref = policy.journey_state_ref.clone();
                let guideline_ref = policy.guideline_ref.clone();
                let approval_mode = policy.approval_mode.as_str().to_string();
                let enabled = policy.enabled;
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO tool_exposure_policies
                            (policy_id, scope_id, tool_name, skill_ref, observation_ref,
                             journey_state_ref, guideline_ref, approval_mode, enabled)
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                         ON CONFLICT(policy_id) DO UPDATE SET
                            tool_name = EXCLUDED.tool_name,
                            skill_ref = EXCLUDED.skill_ref,
                            observation_ref = EXCLUDED.observation_ref,
                            journey_state_ref = EXCLUDED.journey_state_ref,
                            guideline_ref = EXCLUDED.guideline_ref,
                            approval_mode = EXCLUDED.approval_mode,
                            enabled = EXCLUDED.enabled",
                    )
                    .bind(policy_id)
                    .bind(scope_id)
                    .bind(tool_name)
                    .bind(skill_ref)
                    .bind(observation_ref)
                    .bind(journey_state_ref)
                    .bind(guideline_ref)
                    .bind(approval_mode)
                    .bind(enabled)
                    .execute(&*pool)
                    .await
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))?;
            }
        }
        Ok(())
    }

    /// List all tool-exposure policies for a scope.
    pub fn list_tool_exposure_policies(
        &self,
        scope_id: &ScopeId,
    ) -> OpenFangResult<Vec<ToolExposurePolicy>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT policy_id, scope_id, tool_name, skill_ref, observation_ref,
                                journey_state_ref, guideline_ref, approval_mode, enabled
                         FROM tool_exposure_policies
                         WHERE scope_id = ?1
                         ORDER BY tool_name ASC",
                    )
                    .map_err(|e| OpenFangError::Memory(e.to_string()))?;
                let rows = stmt
                    .query_map(params![scope_id.0.as_str()], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, String>(7)?,
                            row.get::<_, i64>(8)?,
                        ))
                    })
                    .map_err(|e| OpenFangError::Memory(e.to_string()))?;

                let mut result = Vec::new();
                for row in rows {
                    let (pid, sid, tool, skill, obs, js, gr, am, en) =
                        row.map_err(|e| OpenFangError::Memory(e.to_string()))?;
                    let approval_mode = am
                        .parse::<openparlant_types::control::ApprovalMode>()
                        .unwrap_or_default();
                    result.push(ToolExposurePolicy {
                        policy_id: pid,
                        scope_id: ScopeId::from(sid),
                        tool_name: tool,
                        skill_ref: skill,
                        observation_ref: obs,
                        journey_state_ref: js,
                        guideline_ref: gr,
                        approval_mode,
                        enabled: en != 0,
                    });
                }
                Ok(result)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.0.clone();
                block_on(async move {
                    let rows = sqlx::query(
                        "SELECT policy_id, scope_id, tool_name, skill_ref, observation_ref,
                                journey_state_ref, guideline_ref, approval_mode, enabled
                         FROM tool_exposure_policies
                         WHERE scope_id = $1
                         ORDER BY tool_name ASC",
                    )
                    .bind(scope_id)
                    .fetch_all(&*pool)
                    .await?;

                    let mut result = Vec::with_capacity(rows.len());
                    for row in rows {
                        let approval_mode: String = row.try_get("approval_mode")?;
                        result.push(ToolExposurePolicy {
                            policy_id: row.try_get("policy_id")?,
                            scope_id: ScopeId::from(row.try_get::<String, _>("scope_id")?),
                            tool_name: row.try_get("tool_name")?,
                            skill_ref: row.try_get("skill_ref")?,
                            observation_ref: row.try_get("observation_ref")?,
                            journey_state_ref: row.try_get("journey_state_ref")?,
                            guideline_ref: row.try_get("guideline_ref")?,
                            approval_mode: approval_mode.parse().unwrap_or_default(),
                            enabled: row.try_get("enabled")?,
                        });
                    }
                    Ok::<Vec<ToolExposurePolicy>, sqlx::Error>(result)
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))
            }
        }
    }

    /// Get a single tool-exposure policy by ID.
    pub fn get_tool_exposure_policy(
        &self,
        policy_id: &str,
    ) -> OpenFangResult<Option<ToolExposurePolicy>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| OpenFangError::Internal(e.to_string()))?;
                let row = conn.query_row(
                    "SELECT policy_id, scope_id, tool_name, skill_ref, observation_ref,
                            journey_state_ref, guideline_ref, approval_mode, enabled
                     FROM tool_exposure_policies WHERE policy_id = ?1",
                    params![policy_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, String>(7)?,
                            row.get::<_, i64>(8)?,
                        ))
                    },
                );
                match row {
                    Ok((pid, sid, tool, skill, obs, js, gr, am, en)) => {
                        let approval_mode = am
                            .parse::<openparlant_types::control::ApprovalMode>()
                            .unwrap_or_default();
                        Ok(Some(ToolExposurePolicy {
                            policy_id: pid,
                            scope_id: ScopeId::from(sid),
                            tool_name: tool,
                            skill_ref: skill,
                            observation_ref: obs,
                            journey_state_ref: js,
                            guideline_ref: gr,
                            approval_mode,
                            enabled: en != 0,
                        }))
                    }
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(OpenFangError::Memory(e.to_string())),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let policy_id = policy_id.to_string();
                block_on(async move {
                    let row = sqlx::query(
                        "SELECT policy_id, scope_id, tool_name, skill_ref, observation_ref,
                                journey_state_ref, guideline_ref, approval_mode, enabled
                         FROM tool_exposure_policies WHERE policy_id = $1",
                    )
                    .bind(policy_id)
                    .fetch_optional(&*pool)
                    .await?;

                    match row {
                        Some(row) => {
                            let approval_mode: String = row.try_get("approval_mode")?;
                            Ok::<Option<ToolExposurePolicy>, sqlx::Error>(Some(
                                ToolExposurePolicy {
                                    policy_id: row.try_get("policy_id")?,
                                    scope_id: ScopeId::from(row.try_get::<String, _>("scope_id")?),
                                    tool_name: row.try_get("tool_name")?,
                                    skill_ref: row.try_get("skill_ref")?,
                                    observation_ref: row.try_get("observation_ref")?,
                                    journey_state_ref: row.try_get("journey_state_ref")?,
                                    guideline_ref: row.try_get("guideline_ref")?,
                                    approval_mode: approval_mode.parse().unwrap_or_default(),
                                    enabled: row.try_get("enabled")?,
                                },
                            ))
                        }
                        None => Ok(None),
                    }
                })
                .map_err(|e| OpenFangError::Memory(e.to_string()))
            }
        }
    }
}

fn observation_from_row(
    row: (String, String, String, String, String, i32, i64),
) -> OpenFangResult<ObservationDefinition> {
    Ok(ObservationDefinition {
        observation_id: parse_uuid(&row.0)
            .map(ObservationId)
            .map_err(memory_parse_error)?,
        scope_id: ScopeId::from(row.1),
        name: row.2,
        matcher_type: row.3,
        matcher_config: serde_json::from_str(&row.4)
            .map_err(|e| OpenFangError::Serialization(e.to_string()))?,
        priority: row.5,
        enabled: row.6 != 0,
    })
}

fn guideline_from_row(
    row: (String, String, String, String, String, String, i32, i64),
) -> OpenFangResult<GuidelineDefinition> {
    Ok(GuidelineDefinition {
        guideline_id: parse_uuid(&row.0)
            .map(GuidelineId)
            .map_err(memory_parse_error)?,
        scope_id: ScopeId::from(row.1),
        name: row.2,
        condition_ref: row.3,
        action_text: row.4,
        composition_mode: row.5,
        priority: row.6,
        enabled: row.7 != 0,
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
    use rusqlite::Connection;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn test_store() -> PolicyStore {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        PolicyStore::new(Arc::new(Mutex::new(conn)))
    }

    #[test]
    fn observations_round_trip_and_filter_enabled() {
        let store = test_store();
        let scope_id = ScopeId::from("default");
        let observation = ObservationDefinition {
            observation_id: ObservationId::new(),
            scope_id: scope_id.clone(),
            name: "vip_user".to_string(),
            matcher_type: "keyword".to_string(),
            matcher_config: json!({ "contains": ["vip"] }),
            priority: 100,
            enabled: true,
        };
        let disabled = ObservationDefinition {
            observation_id: ObservationId::new(),
            scope_id: scope_id.clone(),
            name: "disabled".to_string(),
            matcher_type: "keyword".to_string(),
            matcher_config: json!({ "contains": ["ignore"] }),
            priority: 10,
            enabled: false,
        };

        store.upsert_observation(&observation).unwrap();
        store.upsert_observation(&disabled).unwrap();

        let loaded = store
            .get_observation(observation.observation_id)
            .unwrap()
            .unwrap();
        let enabled = store.list_observations(&scope_id, true).unwrap();
        let all = store.list_observations(&scope_id, false).unwrap();

        assert_eq!(loaded.name, "vip_user");
        assert_eq!(loaded.matcher_config["contains"][0], "vip");
        assert_eq!(enabled.len(), 1);
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn guidelines_round_trip_and_filter_enabled() {
        let store = test_store();
        let scope_id = ScopeId::from("default");
        let guideline = GuidelineDefinition {
            guideline_id: GuidelineId::new(),
            scope_id: scope_id.clone(),
            name: "escalate_to_human".to_string(),
            condition_ref: "vip_user".to_string(),
            action_text: "Offer human handoff immediately.".to_string(),
            composition_mode: "append".to_string(),
            priority: 90,
            enabled: true,
        };
        let disabled = GuidelineDefinition {
            guideline_id: GuidelineId::new(),
            scope_id: scope_id.clone(),
            name: "legacy_rule".to_string(),
            condition_ref: "old".to_string(),
            action_text: "Ignore.".to_string(),
            composition_mode: "append".to_string(),
            priority: 1,
            enabled: false,
        };

        store.upsert_guideline(&guideline).unwrap();
        store.upsert_guideline(&disabled).unwrap();

        let loaded = store
            .get_guideline(guideline.guideline_id)
            .unwrap()
            .unwrap();
        let enabled = store.list_guidelines(&scope_id, true).unwrap();
        let all = store.list_guidelines(&scope_id, false).unwrap();

        assert_eq!(loaded.action_text, "Offer human handoff immediately.");
        assert_eq!(enabled.len(), 1);
        assert_eq!(all.len(), 2);
    }
}
