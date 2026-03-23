use chrono::{DateTime, Utc};
use silicrew_memory::db::{block_on, SharedDb};
use silicrew_types::agent::{AgentId, SessionId};
use silicrew_types::control::{
    ControlScope, JourneyTransitionRecord, PolicyMatchRecord, ResponseMode, ScopeId,
    ToolAuthorizationRecord, TraceId, TurnTraceRecord,
};
use silicrew_types::error::{SiliCrewError, SiliCrewResult};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::str::FromStr;
use std::sync::Arc;

/// Control-plane store backed by SQLite or PostgreSQL.
#[derive(Clone)]
pub struct ControlStore {
    pub(crate) db: SharedDb,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardTenant {
    pub tenant_id: String,
    pub name: String,
    pub slug: String,
    pub im_provider: String,
    pub timezone: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub default_message_limit: i32,
    pub default_message_period: String,
    pub default_max_agents: i32,
    pub default_agent_ttl_hours: i32,
    pub default_max_llm_calls_per_day: i32,
    pub min_heartbeat_interval_minutes: i32,
    pub default_max_triggers: i32,
    pub min_poll_interval_floor: i32,
    pub max_webhook_rate_ceiling: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardUser {
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub display_name: String,
    pub role: String,
    pub tenant_id: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub quota_message_limit: i32,
    pub quota_message_period: String,
    pub quota_messages_used: i32,
    pub quota_max_agents: i32,
    pub quota_agent_ttl_hours: i32,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvitationCodeRecord {
    pub invitation_id: String,
    pub code: String,
    pub tenant_id: Option<String>,
    pub max_uses: i32,
    pub used_count: i32,
    pub is_active: bool,
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SystemSettingRecord {
    pub key: String,
    pub value_json: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardCompanyStats {
    pub tenant: DashboardTenant,
    pub user_count: i64,
    pub agent_count: i64,
    pub agent_running_count: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlazaPostRecord {
    pub id: String,
    pub author_id: String,
    pub author_type: String,
    pub author_name: String,
    pub content: String,
    pub tenant_id: Option<String>,
    pub likes_count: i32,
    pub comments_count: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlazaCommentRecord {
    pub id: String,
    pub post_id: String,
    pub author_id: String,
    pub author_type: String,
    pub author_name: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotificationRecord {
    pub id: String,
    pub tenant_id: Option<String>,
    pub user_id: String,
    pub notification_type: String,
    pub category: String,
    pub title: String,
    pub body: Option<String>,
    pub link: Option<String>,
    pub sender_id: Option<String>,
    pub sender_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
}

impl ControlStore {
    /// Create a new control store wrapping the shared database handle.
    pub fn new(db: impl Into<SharedDb>) -> Self {
        Self { db: db.into() }
    }

    /// Insert or update a control scope.
    pub fn upsert_scope(&self, scope: &ControlScope) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO control_scopes (scope_id, name, scope_type, status, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(scope_id) DO UPDATE SET
                        name = excluded.name,
                        scope_type = excluded.scope_type,
                        status = excluded.status,
                        updated_at = excluded.updated_at",
                    params![
                        scope.scope_id.0.as_str(),
                        scope.name.as_str(),
                        scope.scope_type.as_str(),
                        scope.status.as_str(),
                        scope.created_at.to_rfc3339(),
                        scope.updated_at.to_rfc3339(),
                    ],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope.scope_id.0.clone();
                let name = scope.name.clone();
                let scope_type = scope.scope_type.clone();
                let status = scope.status.clone();
                let created_at = scope.created_at.to_rfc3339();
                let updated_at = scope.updated_at.to_rfc3339();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO control_scopes (scope_id, name, scope_type, status, created_at, updated_at)
                         VALUES ($1, $2, $3, $4, $5, $6)
                         ON CONFLICT(scope_id) DO UPDATE SET
                            name = EXCLUDED.name,
                            scope_type = EXCLUDED.scope_type,
                            status = EXCLUDED.status,
                            updated_at = EXCLUDED.updated_at",
                    )
                    .bind(scope_id)
                    .bind(name)
                    .bind(scope_type)
                    .bind(status)
                    .bind(created_at)
                    .bind(updated_at)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    /// Fetch a control scope by ID.
    pub fn get_scope(&self, scope_id: &ScopeId) -> SiliCrewResult<Option<ControlScope>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT scope_id, name, scope_type, status, created_at, updated_at
                         FROM control_scopes WHERE scope_id = ?1",
                    )
                    .map_err(memory_error)?;

                let row = stmt.query_row(params![scope_id.0.as_str()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                });

                match row {
                    Ok(row) => Ok(Some(scope_from_row(row)?)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.0.clone();
                let row = block_on(async move {
                    sqlx::query(
                        "SELECT scope_id, name, scope_type, status, created_at, updated_at
                         FROM control_scopes WHERE scope_id = $1",
                    )
                    .bind(scope_id)
                    .fetch_optional(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                match row {
                    Some(row) => Ok(Some(scope_from_row((
                        row.try_get("scope_id").map_err(memory_error)?,
                        row.try_get("name").map_err(memory_error)?,
                        row.try_get("scope_type").map_err(memory_error)?,
                        row.try_get("status").map_err(memory_error)?,
                        row.try_get("created_at").map_err(memory_error)?,
                        row.try_get("updated_at").map_err(memory_error)?,
                    ))?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// List all control scopes.
    pub fn list_scopes(&self) -> SiliCrewResult<Vec<ControlScope>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT scope_id, name, scope_type, status, created_at, updated_at
                         FROM control_scopes ORDER BY name ASC, scope_id ASC",
                    )
                    .map_err(memory_error)?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                        ))
                    })
                    .map_err(memory_error)?;

                let mut scopes = Vec::new();
                for row in rows {
                    scopes.push(scope_from_row(row.map_err(memory_error)?)?);
                }
                Ok(scopes)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT scope_id, name, scope_type, status, created_at, updated_at
                         FROM control_scopes ORDER BY name ASC, scope_id ASC",
                    )
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                let mut scopes = Vec::with_capacity(rows.len());
                for row in rows {
                    scopes.push(scope_from_row((
                        row.try_get("scope_id").map_err(memory_error)?,
                        row.try_get("name").map_err(memory_error)?,
                        row.try_get("scope_type").map_err(memory_error)?,
                        row.try_get("status").map_err(memory_error)?,
                        row.try_get("created_at").map_err(memory_error)?,
                        row.try_get("updated_at").map_err(memory_error)?,
                    ))?);
                }
                Ok(scopes)
            }
        }
    }

    /// Insert or update a turn trace record.
    pub fn upsert_turn_trace(&self, trace: &TurnTraceRecord) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO turn_traces (
                        trace_id, scope_id, session_id, agent_id, channel_type,
                        request_message_ref, compiled_context_hash, release_version, response_mode, created_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                     ON CONFLICT(trace_id) DO UPDATE SET
                        scope_id = excluded.scope_id,
                        session_id = excluded.session_id,
                        agent_id = excluded.agent_id,
                        channel_type = excluded.channel_type,
                        request_message_ref = excluded.request_message_ref,
                        compiled_context_hash = excluded.compiled_context_hash,
                        release_version = excluded.release_version,
                        response_mode = excluded.response_mode,
                        created_at = excluded.created_at",
                    params![
                        trace.trace_id.0.to_string(),
                        trace.scope_id.0.as_str(),
                        trace.session_id.0.to_string(),
                        trace.agent_id.0.to_string(),
                        trace.channel_type.as_str(),
                        trace.request_message_ref.as_deref(),
                        trace.compiled_context_hash.as_deref(),
                        trace.release_version.as_deref(),
                        trace.response_mode.as_str(),
                        trace.created_at.to_rfc3339(),
                    ],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let trace_id = trace.trace_id.0.to_string();
                let scope_id = trace.scope_id.0.clone();
                let session_id = trace.session_id.0.to_string();
                let agent_id = trace.agent_id.0.to_string();
                let channel_type = trace.channel_type.clone();
                let request_message_ref = trace.request_message_ref.clone();
                let compiled_context_hash = trace.compiled_context_hash.clone();
                let release_version = trace.release_version.clone();
                let response_mode = trace.response_mode.as_str().to_string();
                let created_at = trace.created_at.to_rfc3339();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO turn_traces (
                            trace_id, scope_id, session_id, agent_id, channel_type,
                            request_message_ref, compiled_context_hash, release_version, response_mode, created_at
                         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                         ON CONFLICT(trace_id) DO UPDATE SET
                            scope_id = EXCLUDED.scope_id,
                            session_id = EXCLUDED.session_id,
                            agent_id = EXCLUDED.agent_id,
                            channel_type = EXCLUDED.channel_type,
                            request_message_ref = EXCLUDED.request_message_ref,
                            compiled_context_hash = EXCLUDED.compiled_context_hash,
                            release_version = EXCLUDED.release_version,
                            response_mode = EXCLUDED.response_mode,
                            created_at = EXCLUDED.created_at",
                    )
                    .bind(trace_id)
                    .bind(scope_id)
                    .bind(session_id)
                    .bind(agent_id)
                    .bind(channel_type)
                    .bind(request_message_ref)
                    .bind(compiled_context_hash)
                    .bind(release_version)
                    .bind(response_mode)
                    .bind(created_at)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    /// Fetch a turn trace by ID.
    pub fn get_turn_trace(&self, trace_id: TraceId) -> SiliCrewResult<Option<TurnTraceRecord>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT trace_id, scope_id, session_id, agent_id, channel_type,
                                request_message_ref, compiled_context_hash, release_version, response_mode, created_at
                         FROM turn_traces WHERE trace_id = ?1",
                    )
                    .map_err(memory_error)?;

                let row = stmt.query_row(params![trace_id.0.to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                    ))
                });

                match row {
                    Ok(row) => Ok(Some(trace_from_row(row)?)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let trace_id = trace_id.0.to_string();
                let row = block_on(async move {
                    sqlx::query(
                        "SELECT trace_id, scope_id, session_id, agent_id, channel_type,
                                request_message_ref, compiled_context_hash, release_version, response_mode, created_at
                         FROM turn_traces WHERE trace_id = $1",
                    )
                    .bind(trace_id)
                    .fetch_optional(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                match row {
                    Some(row) => Ok(Some(trace_from_row((
                        row.try_get("trace_id").map_err(memory_error)?,
                        row.try_get("scope_id").map_err(memory_error)?,
                        row.try_get("session_id").map_err(memory_error)?,
                        row.try_get("agent_id").map_err(memory_error)?,
                        row.try_get("channel_type").map_err(memory_error)?,
                        row.try_get("request_message_ref").map_err(memory_error)?,
                        row.try_get("compiled_context_hash").map_err(memory_error)?,
                        row.try_get("release_version").map_err(memory_error)?,
                        row.try_get("response_mode").map_err(memory_error)?,
                        row.try_get("created_at").map_err(memory_error)?,
                    ))?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// List recent traces for a session, newest first.
    pub fn list_turn_traces_by_session(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> SiliCrewResult<Vec<TurnTraceRecord>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT trace_id, scope_id, session_id, agent_id, channel_type,
                                request_message_ref, compiled_context_hash, release_version, response_mode, created_at
                         FROM turn_traces
                         WHERE session_id = ?1
                         ORDER BY created_at DESC
                         LIMIT ?2",
                    )
                    .map_err(memory_error)?;
                let rows = stmt
                    .query_map(params![session_id.0.to_string(), limit as i64], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, Option<String>>(7)?,
                            row.get::<_, String>(8)?,
                            row.get::<_, String>(9)?,
                        ))
                    })
                    .map_err(memory_error)?;

                let mut traces = Vec::new();
                for row in rows {
                    traces.push(trace_from_row(row.map_err(memory_error)?)?);
                }
                Ok(traces)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let session_id = session_id.0.to_string();
                let limit = limit as i64;
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT trace_id, scope_id, session_id, agent_id, channel_type,
                                request_message_ref, compiled_context_hash, release_version, response_mode, created_at
                         FROM turn_traces
                         WHERE session_id = $1
                         ORDER BY created_at DESC
                         LIMIT $2",
                    )
                    .bind(session_id)
                    .bind(limit)
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                let mut traces = Vec::with_capacity(rows.len());
                for row in rows {
                    traces.push(trace_from_row((
                        row.try_get("trace_id").map_err(memory_error)?,
                        row.try_get("scope_id").map_err(memory_error)?,
                        row.try_get("session_id").map_err(memory_error)?,
                        row.try_get("agent_id").map_err(memory_error)?,
                        row.try_get("channel_type").map_err(memory_error)?,
                        row.try_get("request_message_ref").map_err(memory_error)?,
                        row.try_get("compiled_context_hash").map_err(memory_error)?,
                        row.try_get("release_version").map_err(memory_error)?,
                        row.try_get("response_mode").map_err(memory_error)?,
                        row.try_get("created_at").map_err(memory_error)?,
                    ))?);
                }
                Ok(traces)
            }
        }
    }

    // ─── Explainability sub-records ───────────────────────────────────────────

    /// Persist the policy match record for a turn.
    pub fn upsert_policy_match_record(&self, rec: &PolicyMatchRecord) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO policy_match_records (
                        record_id, trace_id, observation_hits_json,
                        guideline_hits_json, guideline_exclusions_json
                     ) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(record_id) DO UPDATE SET
                        trace_id = excluded.trace_id,
                        observation_hits_json = excluded.observation_hits_json,
                        guideline_hits_json = excluded.guideline_hits_json,
                        guideline_exclusions_json = excluded.guideline_exclusions_json",
                    params![
                        rec.record_id.0.to_string(),
                        rec.trace_id.0.to_string(),
                        rec.observation_hits_json.as_str(),
                        rec.guideline_hits_json.as_str(),
                        rec.guideline_exclusions_json.as_str(),
                    ],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let record_id = rec.record_id.0.to_string();
                let trace_id = rec.trace_id.0.to_string();
                let observation_hits_json = rec.observation_hits_json.clone();
                let guideline_hits_json = rec.guideline_hits_json.clone();
                let guideline_exclusions_json = rec.guideline_exclusions_json.clone();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO policy_match_records (
                            record_id, trace_id, observation_hits_json,
                            guideline_hits_json, guideline_exclusions_json
                         ) VALUES ($1, $2, $3, $4, $5)
                         ON CONFLICT(record_id) DO UPDATE SET
                            trace_id = EXCLUDED.trace_id,
                            observation_hits_json = EXCLUDED.observation_hits_json,
                            guideline_hits_json = EXCLUDED.guideline_hits_json,
                            guideline_exclusions_json = EXCLUDED.guideline_exclusions_json",
                    )
                    .bind(record_id)
                    .bind(trace_id)
                    .bind(observation_hits_json)
                    .bind(guideline_hits_json)
                    .bind(guideline_exclusions_json)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    /// Fetch the policy match record for a trace.
    pub fn get_policy_match_record(
        &self,
        trace_id: TraceId,
    ) -> SiliCrewResult<Option<PolicyMatchRecord>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT record_id, trace_id, observation_hits_json,
                                guideline_hits_json, guideline_exclusions_json
                         FROM policy_match_records WHERE trace_id = ?1",
                    )
                    .map_err(memory_error)?;

                let row = stmt.query_row(params![trace_id.0.to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                });

                match row {
                    Ok(r) => Ok(Some(policy_match_record_from_row(r)?)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let trace_id = trace_id.0.to_string();
                let row = block_on(async move {
                    sqlx::query(
                        "SELECT record_id, trace_id, observation_hits_json,
                                guideline_hits_json, guideline_exclusions_json
                         FROM policy_match_records WHERE trace_id = $1",
                    )
                    .bind(trace_id)
                    .fetch_optional(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                match row {
                    Some(row) => Ok(Some(policy_match_record_from_row((
                        row.try_get("record_id").map_err(memory_error)?,
                        row.try_get("trace_id").map_err(memory_error)?,
                        row.try_get("observation_hits_json").map_err(memory_error)?,
                        row.try_get("guideline_hits_json").map_err(memory_error)?,
                        row.try_get("guideline_exclusions_json")
                            .map_err(memory_error)?,
                    ))?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// Persist the journey transition record for a turn.
    pub fn upsert_journey_transition_record(
        &self,
        rec: &JourneyTransitionRecord,
    ) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO journey_transition_records (
                        record_id, trace_id, journey_instance_id,
                        before_state_id, after_state_id, decision_json
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(record_id) DO UPDATE SET
                        trace_id = excluded.trace_id,
                        journey_instance_id = excluded.journey_instance_id,
                        before_state_id = excluded.before_state_id,
                        after_state_id = excluded.after_state_id,
                        decision_json = excluded.decision_json",
                    params![
                        rec.record_id.0.to_string(),
                        rec.trace_id.0.to_string(),
                        rec.journey_instance_id.as_str(),
                        rec.before_state_id.as_deref(),
                        rec.after_state_id.as_deref(),
                        rec.decision_json.as_str(),
                    ],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let record_id = rec.record_id.0.to_string();
                let trace_id = rec.trace_id.0.to_string();
                let journey_instance_id = rec.journey_instance_id.clone();
                let before_state_id = rec.before_state_id.clone();
                let after_state_id = rec.after_state_id.clone();
                let decision_json = rec.decision_json.clone();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO journey_transition_records (
                            record_id, trace_id, journey_instance_id,
                            before_state_id, after_state_id, decision_json
                         ) VALUES ($1, $2, $3, $4, $5, $6)
                         ON CONFLICT(record_id) DO UPDATE SET
                            trace_id = EXCLUDED.trace_id,
                            journey_instance_id = EXCLUDED.journey_instance_id,
                            before_state_id = EXCLUDED.before_state_id,
                            after_state_id = EXCLUDED.after_state_id,
                            decision_json = EXCLUDED.decision_json",
                    )
                    .bind(record_id)
                    .bind(trace_id)
                    .bind(journey_instance_id)
                    .bind(before_state_id)
                    .bind(after_state_id)
                    .bind(decision_json)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    /// Persist the tool authorization record for a turn.
    pub fn upsert_tool_authorization_record(
        &self,
        rec: &ToolAuthorizationRecord,
    ) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO tool_authorization_records (
                        record_id, trace_id, allowed_tools_json,
                        authorization_reasons_json, approval_requirements_json
                     ) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(record_id) DO UPDATE SET
                        trace_id = excluded.trace_id,
                        allowed_tools_json = excluded.allowed_tools_json,
                        authorization_reasons_json = excluded.authorization_reasons_json,
                        approval_requirements_json = excluded.approval_requirements_json",
                    params![
                        rec.record_id.0.to_string(),
                        rec.trace_id.0.to_string(),
                        rec.allowed_tools_json.as_str(),
                        rec.authorization_reasons_json.as_str(),
                        rec.approval_requirements_json.as_str(),
                    ],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let record_id = rec.record_id.0.to_string();
                let trace_id = rec.trace_id.0.to_string();
                let allowed_tools_json = rec.allowed_tools_json.clone();
                let authorization_reasons_json = rec.authorization_reasons_json.clone();
                let approval_requirements_json = rec.approval_requirements_json.clone();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO tool_authorization_records (
                            record_id, trace_id, allowed_tools_json,
                            authorization_reasons_json, approval_requirements_json
                         ) VALUES ($1, $2, $3, $4, $5)
                         ON CONFLICT(record_id) DO UPDATE SET
                            trace_id = EXCLUDED.trace_id,
                            allowed_tools_json = EXCLUDED.allowed_tools_json,
                            authorization_reasons_json = EXCLUDED.authorization_reasons_json,
                            approval_requirements_json = EXCLUDED.approval_requirements_json",
                    )
                    .bind(record_id)
                    .bind(trace_id)
                    .bind(allowed_tools_json)
                    .bind(authorization_reasons_json)
                    .bind(approval_requirements_json)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    // ── Session Bindings ──────────────────────────────────────────────────────

    /// Upsert a session binding (creates or updates).
    pub fn upsert_session_binding(
        &self,
        binding: &silicrew_types::control::SessionBinding,
    ) -> SiliCrewResult<()> {
        let now = now_rfc3339();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let updated = conn
                    .execute(
                        "UPDATE session_bindings
                         SET scope_id = ?1,
                             channel_type = ?2,
                             external_user_id = ?3,
                             external_chat_id = ?4,
                             agent_id = ?5,
                             manual_mode = ?6,
                             active_journey_instance_id = ?7,
                             updated_at = ?8
                         WHERE session_id = ?9",
                        params![
                            binding.scope_id.0.as_str(),
                            binding.channel_type,
                            binding.external_user_id,
                            binding.external_chat_id,
                            binding.agent_id,
                            binding.manual_mode as i64,
                            binding.active_journey_instance_id,
                            now,
                            binding.session_id,
                        ],
                    )
                    .map_err(memory_error)?;
                if updated == 0 {
                    conn.execute(
                        "INSERT INTO session_bindings
                            (binding_id, scope_id, channel_type, external_user_id, external_chat_id,
                             agent_id, session_id, manual_mode, active_journey_instance_id, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                        params![
                            binding.binding_id,
                            binding.scope_id.0.as_str(),
                            binding.channel_type,
                            binding.external_user_id,
                            binding.external_chat_id,
                            binding.agent_id,
                            binding.session_id,
                            binding.manual_mode as i64,
                            binding.active_journey_instance_id,
                            now,
                        ],
                    )
                    .map_err(memory_error)?;
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = binding.scope_id.0.clone();
                let channel_type = binding.channel_type.clone();
                let external_user_id = binding.external_user_id.clone();
                let external_chat_id = binding.external_chat_id.clone();
                let agent_id = binding.agent_id.clone();
                let manual_mode = binding.manual_mode;
                let active_journey_instance_id = binding.active_journey_instance_id.clone();
                let session_id = binding.session_id.clone();
                let update_pool = Arc::clone(&pool);
                let update_now = now.clone();
                let updated = block_on(async move {
                    sqlx::query(
                        "UPDATE session_bindings
                         SET scope_id = $1,
                             channel_type = $2,
                             external_user_id = $3,
                             external_chat_id = $4,
                             agent_id = $5,
                             manual_mode = $6,
                             active_journey_instance_id = $7,
                             updated_at = $8
                         WHERE session_id = $9",
                    )
                    .bind(scope_id)
                    .bind(channel_type)
                    .bind(external_user_id)
                    .bind(external_chat_id)
                    .bind(agent_id)
                    .bind(manual_mode)
                    .bind(active_journey_instance_id)
                    .bind(update_now)
                    .bind(session_id)
                    .execute(&*update_pool)
                    .await
                })
                .map_err(memory_error)?;

                if updated.rows_affected() == 0 {
                    let pool = Arc::clone(&pool);
                    let binding_id = binding.binding_id.clone();
                    let scope_id = binding.scope_id.0.clone();
                    let channel_type = binding.channel_type.clone();
                    let external_user_id = binding.external_user_id.clone();
                    let external_chat_id = binding.external_chat_id.clone();
                    let agent_id = binding.agent_id.clone();
                    let session_id = binding.session_id.clone();
                    let manual_mode = binding.manual_mode;
                    let active_journey_instance_id = binding.active_journey_instance_id.clone();
                    let created_at = now.clone();
                    block_on(async move {
                        sqlx::query(
                            "INSERT INTO session_bindings
                                (binding_id, scope_id, channel_type, external_user_id, external_chat_id,
                                 agent_id, session_id, manual_mode, active_journey_instance_id, created_at, updated_at)
                             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10)",
                        )
                        .bind(binding_id)
                        .bind(scope_id)
                        .bind(channel_type)
                        .bind(external_user_id)
                        .bind(external_chat_id)
                        .bind(agent_id)
                        .bind(session_id)
                        .bind(manual_mode)
                        .bind(active_journey_instance_id)
                        .bind(created_at)
                        .execute(&*pool)
                        .await
                    })
                    .map_err(memory_error)?;
                }
            }
        }
        Ok(())
    }

    /// Look up the session binding for a session_id (first match).
    pub fn get_session_binding(
        &self,
        session_id: &str,
    ) -> SiliCrewResult<Option<silicrew_types::control::SessionBinding>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let row = conn.query_row(
                    "SELECT binding_id, scope_id, channel_type, external_user_id, external_chat_id,
                            agent_id, session_id, manual_mode, active_journey_instance_id
                     FROM session_bindings
                     WHERE session_id = ?1
                     ORDER BY updated_at DESC, rowid DESC
                     LIMIT 1",
                    params![session_id],
                    |row| session_binding_from_sqlite_row(row),
                );
                match row {
                    Ok(b) => Ok(Some(b)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let session_id = session_id.to_string();
                let row = block_on(async move {
                    sqlx::query(
                        "SELECT binding_id, scope_id, channel_type, external_user_id, external_chat_id,
                                agent_id, session_id, manual_mode, active_journey_instance_id
                         FROM session_bindings
                         WHERE session_id = $1
                         ORDER BY updated_at DESC, binding_id DESC
                         LIMIT 1",
                    )
                    .bind(session_id)
                    .fetch_optional(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                match row {
                    Some(row) => Ok(Some(session_binding_from_pg_row(&row)?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// Enable or disable manual mode for a session binding.
    pub fn set_manual_mode(&self, session_id: &str, manual: bool) -> SiliCrewResult<bool> {
        let now = now_rfc3339();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let rows = conn
                    .execute(
                        "UPDATE session_bindings SET manual_mode = ?1, updated_at = ?2
                         WHERE session_id = ?3",
                        params![manual as i64, now, session_id],
                    )
                    .map_err(memory_error)?;
                Ok(rows > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let session_id = session_id.to_string();
                let rows = block_on(async move {
                    sqlx::query(
                        "UPDATE session_bindings SET manual_mode = $1, updated_at = $2
                         WHERE session_id = $3",
                    )
                    .bind(manual)
                    .bind(now)
                    .bind(session_id)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
                Ok(rows.rows_affected() > 0)
            }
        }
    }

    // ── Retrievers ────────────────────────────────────────────────────────────

    /// Insert or update a retriever definition.
    pub fn upsert_retriever(&self, r: &serde_json::Value) -> SiliCrewResult<()> {
        let retriever_id = r["retriever_id"].as_str().unwrap_or("").to_string();
        let scope_id = r["scope_id"].as_str().unwrap_or("").to_string();
        let name = r["name"].as_str().unwrap_or("").to_string();
        let retriever_type = r["retriever_type"].as_str().unwrap_or("static").to_string();
        let config_json = r["config_json"].to_string();
        let enabled = r["enabled"].as_bool().unwrap_or(true);

        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO retrievers (retriever_id, scope_id, name, retriever_type, config_json, enabled)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(retriever_id) DO UPDATE SET
                        name = excluded.name,
                        retriever_type = excluded.retriever_type,
                        config_json = excluded.config_json,
                        enabled = excluded.enabled",
                    params![
                        retriever_id,
                        scope_id,
                        name,
                        retriever_type,
                        config_json,
                        enabled as i64,
                    ],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO retrievers (retriever_id, scope_id, name, retriever_type, config_json, enabled)
                         VALUES ($1, $2, $3, $4, $5, $6)
                         ON CONFLICT(retriever_id) DO UPDATE SET
                            name = EXCLUDED.name,
                            retriever_type = EXCLUDED.retriever_type,
                            config_json = EXCLUDED.config_json,
                            enabled = EXCLUDED.enabled",
                    )
                    .bind(retriever_id)
                    .bind(scope_id)
                    .bind(name)
                    .bind(retriever_type)
                    .bind(config_json)
                    .bind(enabled)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    /// List retrievers for a scope.
    pub fn list_retrievers(&self, scope_id: &ScopeId) -> SiliCrewResult<Vec<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT retriever_id, scope_id, name, retriever_type, config_json, enabled
                         FROM retrievers WHERE scope_id = ?1 ORDER BY name ASC",
                    )
                    .map_err(memory_error)?;
                let rows = stmt
                    .query_map(params![scope_id.0.as_str()], |row| {
                        Ok(retriever_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, i64>(5)? != 0,
                        ))
                    })
                    .map_err(memory_error)?;
                Ok(rows.filter_map(|r| r.ok()).collect())
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.0.clone();
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT retriever_id, scope_id, name, retriever_type, config_json, enabled
                         FROM retrievers WHERE scope_id = $1 ORDER BY name ASC",
                    )
                    .bind(scope_id)
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(retriever_json(
                        row.try_get("retriever_id").map_err(memory_error)?,
                        row.try_get("scope_id").map_err(memory_error)?,
                        row.try_get("name").map_err(memory_error)?,
                        row.try_get("retriever_type").map_err(memory_error)?,
                        row.try_get("config_json").map_err(memory_error)?,
                        row.try_get::<bool, _>("enabled").map_err(memory_error)?,
                    ));
                }
                Ok(out)
            }
        }
    }

    /// Fetch a single retriever by id.
    pub fn get_retriever(&self, retriever_id: &str) -> SiliCrewResult<Option<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let row = conn.query_row(
                    "SELECT retriever_id, scope_id, name, retriever_type, config_json, enabled
                     FROM retrievers WHERE retriever_id = ?1",
                    params![retriever_id],
                    |row| {
                        Ok(retriever_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, i64>(5)? != 0,
                        ))
                    },
                );
                match row {
                    Ok(value) => Ok(Some(value)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let retriever_id = retriever_id.to_string();
                block_on(async move {
                    let row = sqlx::query(
                        "SELECT retriever_id, scope_id, name, retriever_type, config_json, enabled
                         FROM retrievers WHERE retriever_id = $1",
                    )
                    .bind(retriever_id)
                    .fetch_optional(&*pool)
                    .await?;

                    Ok::<Option<serde_json::Value>, sqlx::Error>(row.map(|row| {
                        retriever_json(
                            row.try_get("retriever_id").unwrap_or_default(),
                            row.try_get("scope_id").unwrap_or_default(),
                            row.try_get("name").unwrap_or_default(),
                            row.try_get("retriever_type").unwrap_or_default(),
                            row.try_get("config_json")
                                .unwrap_or_else(|_| "{}".to_string()),
                            row.try_get("enabled").unwrap_or(false),
                        )
                    }))
                })
                .map_err(memory_error)
            }
        }
    }

    /// Delete a retriever and any associated bindings.
    pub fn delete_retriever(&self, retriever_id: &str) -> SiliCrewResult<bool> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "DELETE FROM retriever_bindings WHERE retriever_id = ?1",
                    params![retriever_id],
                )
                .map_err(memory_error)?;
                let rows = conn
                    .execute(
                        "DELETE FROM retrievers WHERE retriever_id = ?1",
                        params![retriever_id],
                    )
                    .map_err(memory_error)?;
                Ok(rows > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let retriever_id = retriever_id.to_string();
                let rows = block_on(async move {
                    sqlx::query("DELETE FROM retriever_bindings WHERE retriever_id = $1")
                        .bind(&retriever_id)
                        .execute(&*pool)
                        .await?;
                    sqlx::query("DELETE FROM retrievers WHERE retriever_id = $1")
                        .bind(&retriever_id)
                        .execute(&*pool)
                        .await
                })
                .map_err(memory_error)?;
                Ok(rows.rows_affected() > 0)
            }
        }
    }

    /// Create a retriever binding (which guideline / journey / scope activates this retriever).
    pub fn insert_retriever_binding(
        &self,
        scope_id: &ScopeId,
        retriever_id: &str,
        bind_type: &str,
        bind_ref: &str,
    ) -> SiliCrewResult<String> {
        let binding_id = uuid::Uuid::new_v4().to_string();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO retriever_bindings (binding_id, scope_id, retriever_id, bind_type, bind_ref)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        binding_id.as_str(),
                        scope_id.0.as_str(),
                        retriever_id,
                        bind_type,
                        bind_ref
                    ],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let binding_id_for_insert = binding_id.clone();
                let scope_id = scope_id.0.clone();
                let retriever_id = retriever_id.to_string();
                let bind_type = bind_type.to_string();
                let bind_ref = bind_ref.to_string();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO retriever_bindings (binding_id, scope_id, retriever_id, bind_type, bind_ref)
                         VALUES ($1, $2, $3, $4, $5)",
                    )
                    .bind(binding_id_for_insert)
                    .bind(scope_id)
                    .bind(retriever_id)
                    .bind(bind_type)
                    .bind(bind_ref)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(binding_id)
    }

    /// List all retriever bindings for a scope.
    pub fn list_retriever_bindings(
        &self,
        scope_id: &ScopeId,
    ) -> SiliCrewResult<Vec<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT binding_id, scope_id, retriever_id, bind_type, bind_ref
                         FROM retriever_bindings WHERE scope_id = ?1
                         ORDER BY retriever_id ASC, bind_type ASC, bind_ref ASC",
                    )
                    .map_err(memory_error)?;
                let rows = stmt
                    .query_map(params![scope_id.0.as_str()], |row| {
                        Ok(retriever_binding_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    })
                    .map_err(memory_error)?;
                Ok(rows.filter_map(|r| r.ok()).collect())
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.0.clone();
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT binding_id, scope_id, retriever_id, bind_type, bind_ref
                         FROM retriever_bindings WHERE scope_id = $1
                         ORDER BY retriever_id ASC, bind_type ASC, bind_ref ASC",
                    )
                    .bind(scope_id)
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(retriever_binding_json(
                        row.try_get("binding_id").map_err(memory_error)?,
                        row.try_get("scope_id").map_err(memory_error)?,
                        row.try_get("retriever_id").map_err(memory_error)?,
                        row.try_get("bind_type").map_err(memory_error)?,
                        row.try_get("bind_ref").map_err(memory_error)?,
                    ));
                }
                Ok(out)
            }
        }
    }

    /// Delete a retriever binding by id.
    pub fn delete_retriever_binding(&self, binding_id: &str) -> SiliCrewResult<bool> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let n = conn
                    .execute(
                        "DELETE FROM retriever_bindings WHERE binding_id = ?1",
                        params![binding_id],
                    )
                    .map_err(memory_error)?;
                Ok(n > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let binding_id = binding_id.to_string();
                let n = block_on(async move {
                    sqlx::query("DELETE FROM retriever_bindings WHERE binding_id = $1")
                        .bind(binding_id)
                        .execute(&*pool)
                        .await
                })
                .map_err(memory_error)?;
                Ok(n.rows_affected() > 0)
            }
        }
    }

    // ── Control Releases ──────────────────────────────────────────────────────

    /// Publish a new release snapshot for a scope.
    pub fn publish_release(
        &self,
        release_id: &str,
        scope_id: &ScopeId,
        version: &str,
        published_by: &str,
    ) -> SiliCrewResult<()> {
        let created_at = now_rfc3339();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "UPDATE control_releases SET status = 'superseded'
                     WHERE scope_id = ?1 AND status = 'published'",
                    params![scope_id.0.as_str()],
                )
                .map_err(memory_error)?;
                conn.execute(
                    "INSERT INTO control_releases (release_id, scope_id, version, status, published_by, created_at)
                     VALUES (?1, ?2, ?3, 'published', ?4, ?5)",
                    params![release_id, scope_id.0.as_str(), version, published_by, created_at],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.0.clone();
                let release_id = release_id.to_string();
                let version = version.to_string();
                let published_by = published_by.to_string();
                block_on(async move {
                    sqlx::query(
                        "UPDATE control_releases SET status = 'superseded'
                         WHERE scope_id = $1 AND status = 'published'",
                    )
                    .bind(scope_id.clone())
                    .execute(&*pool)
                    .await?;

                    sqlx::query(
                        "INSERT INTO control_releases (release_id, scope_id, version, status, published_by, created_at)
                         VALUES ($1, $2, $3, 'published', $4, $5)",
                    )
                    .bind(release_id)
                    .bind(scope_id)
                    .bind(version)
                    .bind(published_by)
                    .bind(created_at)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    /// Rollback: mark the latest published release as rolled_back and re-activate the previous one.
    pub fn rollback_release(&self, scope_id: &ScopeId) -> SiliCrewResult<Option<String>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let current: Option<String> = conn
                    .query_row(
                        "SELECT release_id FROM control_releases
                         WHERE scope_id = ?1 AND status = 'published'
                         ORDER BY created_at DESC LIMIT 1",
                        params![scope_id.0.as_str()],
                        |row| row.get(0),
                    )
                    .ok();
                if let Some(ref rid) = current {
                    conn.execute(
                        "UPDATE control_releases SET status = 'rolled_back' WHERE release_id = ?1",
                        params![rid],
                    )
                    .map_err(memory_error)?;
                    conn.execute(
                        "UPDATE control_releases SET status = 'published'
                         WHERE scope_id = ?1 AND status = 'superseded'
                         AND release_id = (
                             SELECT release_id FROM control_releases
                             WHERE scope_id = ?1 AND status = 'superseded'
                             ORDER BY created_at DESC LIMIT 1
                         )",
                        params![scope_id.0.as_str(), scope_id.0.as_str()],
                    )
                    .map_err(memory_error)?;
                }
                Ok(current)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.0.clone();
                block_on(async move {
                    let current = sqlx::query(
                        "SELECT release_id FROM control_releases
                         WHERE scope_id = $1 AND status = 'published'
                         ORDER BY created_at DESC LIMIT 1",
                    )
                    .bind(scope_id.clone())
                    .fetch_optional(&*pool)
                    .await?;
                    let current = current
                        .map(|row| row.try_get::<String, _>("release_id"))
                        .transpose()?;
                    if let Some(ref rid) = current {
                        sqlx::query(
                            "UPDATE control_releases SET status = 'rolled_back' WHERE release_id = $1",
                        )
                        .bind(rid)
                        .execute(&*pool)
                        .await?;
                        sqlx::query(
                            "UPDATE control_releases SET status = 'published'
                             WHERE scope_id = $1 AND status = 'superseded'
                             AND release_id = (
                                 SELECT release_id FROM control_releases
                                 WHERE scope_id = $1 AND status = 'superseded'
                                 ORDER BY created_at DESC LIMIT 1
                             )",
                        )
                        .bind(scope_id)
                        .execute(&*pool)
                        .await?;
                    }
                    Ok::<Option<String>, sqlx::Error>(current)
                })
                .map_err(memory_error)
            }
        }
    }

    /// List releases for a scope (newest first).
    pub fn list_releases(&self, scope_id: &ScopeId) -> SiliCrewResult<Vec<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT release_id, scope_id, version, status, published_by, created_at
                         FROM control_releases WHERE scope_id = ?1 ORDER BY created_at DESC",
                    )
                    .map_err(memory_error)?;
                let rows = stmt
                    .query_map(params![scope_id.0.as_str()], |row| {
                        Ok(release_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                        ))
                    })
                    .map_err(memory_error)?;
                Ok(rows.filter_map(|r| r.ok()).collect())
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.0.clone();
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT release_id, scope_id, version, status, published_by, created_at
                         FROM control_releases WHERE scope_id = $1 ORDER BY created_at DESC",
                    )
                    .bind(scope_id)
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(release_json(
                        row.try_get("release_id").map_err(memory_error)?,
                        row.try_get("scope_id").map_err(memory_error)?,
                        row.try_get("version").map_err(memory_error)?,
                        row.try_get("status").map_err(memory_error)?,
                        row.try_get("published_by").map_err(memory_error)?,
                        row.try_get("created_at").map_err(memory_error)?,
                    ));
                }
                Ok(out)
            }
        }
    }

    /// Return the currently published release version for a scope, if any.
    pub fn current_release_version(&self, scope_id: &ScopeId) -> SiliCrewResult<Option<String>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let row = conn.query_row(
                    "SELECT version
                     FROM control_releases
                     WHERE scope_id = ?1 AND status = 'published'
                     ORDER BY created_at DESC
                     LIMIT 1",
                    params![scope_id.0.as_str()],
                    |row| row.get::<_, String>(0),
                );
                match row {
                    Ok(version) => Ok(Some(version)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.0.clone();
                let row = block_on(async move {
                    sqlx::query(
                        "SELECT version
                         FROM control_releases
                         WHERE scope_id = $1 AND status = 'published'
                         ORDER BY created_at DESC
                         LIMIT 1",
                    )
                    .bind(scope_id)
                    .fetch_optional(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                match row {
                    Some(row) => Ok(Some(row.try_get("version").map_err(memory_error)?)),
                    None => Ok(None),
                }
            }
        }
    }

    // ── Handoff Records ───────────────────────────────────────────────────────

    /// Insert a new handoff record.
    pub fn create_handoff(
        &self,
        handoff: &silicrew_types::control::HandoffRecord,
    ) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO handoff_records
                        (handoff_id, scope_id, session_id, reason, summary, status, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        handoff.handoff_id,
                        handoff.scope_id.0.as_str(),
                        handoff.session_id,
                        handoff.reason,
                        handoff.summary,
                        handoff.status.as_str(),
                        handoff.created_at.to_rfc3339(),
                        handoff.updated_at.to_rfc3339(),
                    ],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let handoff_id = handoff.handoff_id.clone();
                let scope_id = handoff.scope_id.0.clone();
                let session_id = handoff.session_id.clone();
                let reason = handoff.reason.clone();
                let summary = handoff.summary.clone();
                let status = handoff.status.as_str().to_string();
                let created_at = handoff.created_at.to_rfc3339();
                let updated_at = handoff.updated_at.to_rfc3339();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO handoff_records
                            (handoff_id, scope_id, session_id, reason, summary, status, created_at, updated_at)
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                    )
                    .bind(handoff_id)
                    .bind(scope_id)
                    .bind(session_id)
                    .bind(reason)
                    .bind(summary)
                    .bind(status)
                    .bind(created_at)
                    .bind(updated_at)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    /// Update the status of a handoff record.
    pub fn update_handoff_status(
        &self,
        handoff_id: &str,
        status: &silicrew_types::control::HandoffStatus,
    ) -> SiliCrewResult<bool> {
        let now = now_rfc3339();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let rows = conn
                    .execute(
                        "UPDATE handoff_records SET status = ?1, updated_at = ?2
                         WHERE handoff_id = ?3",
                        params![status.as_str(), now, handoff_id],
                    )
                    .map_err(memory_error)?;
                Ok(rows > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let handoff_id = handoff_id.to_string();
                let status = status.as_str().to_string();
                let rows = block_on(async move {
                    sqlx::query(
                        "UPDATE handoff_records SET status = $1, updated_at = $2
                         WHERE handoff_id = $3",
                    )
                    .bind(status)
                    .bind(now)
                    .bind(handoff_id)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
                Ok(rows.rows_affected() > 0)
            }
        }
    }

    /// List handoff records for a session (newest first).
    pub fn list_handoffs_by_session(
        &self,
        session_id: &str,
        limit: usize,
    ) -> SiliCrewResult<Vec<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT handoff_id, scope_id, session_id, reason, summary, status, created_at, updated_at
                         FROM handoff_records WHERE session_id = ?1
                         ORDER BY created_at DESC LIMIT ?2",
                    )
                    .map_err(memory_error)?;
                let rows = stmt
                    .query_map(params![session_id, limit as i64], |row| {
                        Ok(handoff_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                        ))
                    })
                    .map_err(memory_error)?;
                Ok(rows.filter_map(|r| r.ok()).collect())
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let session_id = session_id.to_string();
                let limit = limit as i64;
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT handoff_id, scope_id, session_id, reason, summary, status, created_at, updated_at
                         FROM handoff_records WHERE session_id = $1
                         ORDER BY created_at DESC LIMIT $2",
                    )
                    .bind(session_id)
                    .bind(limit)
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(handoff_json(
                        row.try_get("handoff_id").map_err(memory_error)?,
                        row.try_get("scope_id").map_err(memory_error)?,
                        row.try_get("session_id").map_err(memory_error)?,
                        row.try_get("reason").map_err(memory_error)?,
                        row.try_get("summary").map_err(memory_error)?,
                        row.try_get("status").map_err(memory_error)?,
                        row.try_get("created_at").map_err(memory_error)?,
                        row.try_get("updated_at").map_err(memory_error)?,
                    ));
                }
                Ok(out)
            }
        }
    }

    /// Insert or update a glossary term.
    pub fn upsert_glossary_term(
        &self,
        term_id: &str,
        scope_id: &str,
        name: &str,
        description: &str,
        synonyms_json: &str,
        enabled: bool,
        always_include: bool,
    ) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO glossary_terms (term_id, scope_id, name, description, synonyms_json, enabled, always_include)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(term_id) DO UPDATE SET
                        name = excluded.name,
                        description = excluded.description,
                        synonyms_json = excluded.synonyms_json,
                        enabled = excluded.enabled,
                        always_include = excluded.always_include",
                    params![term_id, scope_id, name, description, synonyms_json, enabled as i64, always_include as i64],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let term_id = term_id.to_string();
                let scope_id = scope_id.to_string();
                let name = name.to_string();
                let description = description.to_string();
                let synonyms_json = synonyms_json.to_string();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO glossary_terms (term_id, scope_id, name, description, synonyms_json, enabled, always_include)
                         VALUES ($1, $2, $3, $4, $5, $6, $7)
                         ON CONFLICT(term_id) DO UPDATE SET
                            name = EXCLUDED.name,
                            description = EXCLUDED.description,
                            synonyms_json = EXCLUDED.synonyms_json,
                            enabled = EXCLUDED.enabled,
                            always_include = EXCLUDED.always_include",
                    )
                    .bind(term_id)
                    .bind(scope_id)
                    .bind(name)
                    .bind(description)
                    .bind(synonyms_json)
                    .bind(enabled)
                    .bind(always_include)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    /// List glossary terms for a scope.
    pub fn list_glossary_terms(&self, scope_id: &str) -> SiliCrewResult<Vec<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT term_id, scope_id, name, description, synonyms_json, enabled, COALESCE(always_include, 0) AS always_include
                         FROM glossary_terms WHERE scope_id = ?1 ORDER BY name",
                    )
                    .map_err(memory_error)?;
                let rows = stmt
                    .query_map(params![scope_id], |row| {
                        Ok(glossary_term_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, bool>(5)?,
                            row.get::<_, i64>(6)? != 0,
                        ))
                    })
                    .map_err(memory_error)?;
                Ok(rows.filter_map(|r| r.ok()).collect())
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.to_string();
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT term_id, scope_id, name, description, synonyms_json, enabled,
                                COALESCE(always_include, FALSE) AS always_include
                         FROM glossary_terms WHERE scope_id = $1 ORDER BY name",
                    )
                    .bind(scope_id)
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(glossary_term_json(
                        row.try_get("term_id").map_err(memory_error)?,
                        row.try_get("scope_id").map_err(memory_error)?,
                        row.try_get("name").map_err(memory_error)?,
                        row.try_get("description").map_err(memory_error)?,
                        row.try_get("synonyms_json").map_err(memory_error)?,
                        row.try_get::<bool, _>("enabled").map_err(memory_error)?,
                        row.try_get::<bool, _>("always_include")
                            .map_err(memory_error)?,
                    ));
                }
                Ok(out)
            }
        }
    }

    /// Fetch a single glossary term by id.
    pub fn get_glossary_term(&self, term_id: &str) -> SiliCrewResult<Option<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let row = conn.query_row(
                    "SELECT term_id, scope_id, name, description, synonyms_json, enabled,
                            COALESCE(always_include, 0) AS always_include
                     FROM glossary_terms WHERE term_id = ?1",
                    params![term_id],
                    |row| {
                        Ok(glossary_term_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, bool>(5)?,
                            row.get::<_, i64>(6)? != 0,
                        ))
                    },
                );
                match row {
                    Ok(value) => Ok(Some(value)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let term_id = term_id.to_string();
                block_on(async move {
                    let row = sqlx::query(
                        "SELECT term_id, scope_id, name, description, synonyms_json, enabled,
                                COALESCE(always_include, FALSE) AS always_include
                         FROM glossary_terms WHERE term_id = $1",
                    )
                    .bind(term_id)
                    .fetch_optional(&*pool)
                    .await?;

                    Ok::<Option<serde_json::Value>, sqlx::Error>(row.map(|row| {
                        glossary_term_json(
                            row.try_get("term_id").unwrap_or_default(),
                            row.try_get("scope_id").unwrap_or_default(),
                            row.try_get("name").unwrap_or_default(),
                            row.try_get("description").unwrap_or_default(),
                            row.try_get("synonyms_json")
                                .unwrap_or_else(|_| "[]".to_string()),
                            row.try_get("enabled").unwrap_or(false),
                            row.try_get("always_include").unwrap_or(false),
                        )
                    }))
                })
                .map_err(memory_error)
            }
        }
    }

    /// Delete a glossary term by id.
    pub fn delete_glossary_term(&self, term_id: &str) -> SiliCrewResult<bool> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let rows = conn
                    .execute(
                        "DELETE FROM glossary_terms WHERE term_id = ?1",
                        params![term_id],
                    )
                    .map_err(memory_error)?;
                Ok(rows > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let term_id = term_id.to_string();
                let rows = block_on(async move {
                    sqlx::query("DELETE FROM glossary_terms WHERE term_id = $1")
                        .bind(term_id)
                        .execute(&*pool)
                        .await
                })
                .map_err(memory_error)?;
                Ok(rows.rows_affected() > 0)
            }
        }
    }

    /// Insert or update a context variable.
    pub fn upsert_context_variable(
        &self,
        variable_id: &str,
        scope_id: &str,
        name: &str,
        value_source_type: &str,
        value_source_config: &str,
        visibility_rule: Option<&str>,
        enabled: bool,
    ) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO context_variables (variable_id, scope_id, name, value_source_type, value_source_config, visibility_rule, enabled)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(variable_id) DO UPDATE SET
                        name = excluded.name,
                        value_source_type = excluded.value_source_type,
                        value_source_config = excluded.value_source_config,
                        visibility_rule = excluded.visibility_rule,
                        enabled = excluded.enabled",
                    params![variable_id, scope_id, name, value_source_type, value_source_config, visibility_rule, enabled as i64],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let variable_id = variable_id.to_string();
                let scope_id = scope_id.to_string();
                let name = name.to_string();
                let value_source_type = value_source_type.to_string();
                let value_source_config = value_source_config.to_string();
                let visibility_rule = visibility_rule.map(ToOwned::to_owned);
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO context_variables (variable_id, scope_id, name, value_source_type, value_source_config, visibility_rule, enabled)
                         VALUES ($1, $2, $3, $4, $5, $6, $7)
                         ON CONFLICT(variable_id) DO UPDATE SET
                            name = EXCLUDED.name,
                            value_source_type = EXCLUDED.value_source_type,
                            value_source_config = EXCLUDED.value_source_config,
                            visibility_rule = EXCLUDED.visibility_rule,
                            enabled = EXCLUDED.enabled",
                    )
                    .bind(variable_id)
                    .bind(scope_id)
                    .bind(name)
                    .bind(value_source_type)
                    .bind(value_source_config)
                    .bind(visibility_rule)
                    .bind(enabled)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    /// List context variables for a scope.
    pub fn list_context_variables(&self, scope_id: &str) -> SiliCrewResult<Vec<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT variable_id, scope_id, name, value_source_type, value_source_config, enabled, visibility_rule
                         FROM context_variables WHERE scope_id = ?1 ORDER BY name",
                    )
                    .map_err(memory_error)?;
                let rows = stmt
                    .query_map(params![scope_id], |row| {
                        Ok(context_variable_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, bool>(5)?,
                            row.get::<_, Option<String>>(6)?,
                        ))
                    })
                    .map_err(memory_error)?;
                Ok(rows.filter_map(|r| r.ok()).collect())
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.to_string();
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT variable_id, scope_id, name, value_source_type, value_source_config, enabled, visibility_rule
                         FROM context_variables WHERE scope_id = $1 ORDER BY name",
                    )
                    .bind(scope_id)
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(context_variable_json(
                        row.try_get("variable_id").map_err(memory_error)?,
                        row.try_get("scope_id").map_err(memory_error)?,
                        row.try_get("name").map_err(memory_error)?,
                        row.try_get("value_source_type").map_err(memory_error)?,
                        row.try_get("value_source_config").map_err(memory_error)?,
                        row.try_get::<bool, _>("enabled").map_err(memory_error)?,
                        row.try_get("visibility_rule").map_err(memory_error)?,
                    ));
                }
                Ok(out)
            }
        }
    }

    /// Fetch a single context variable by id.
    pub fn get_context_variable(
        &self,
        variable_id: &str,
    ) -> SiliCrewResult<Option<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let row = conn.query_row(
                    "SELECT variable_id, scope_id, name, value_source_type, value_source_config, enabled, visibility_rule
                     FROM context_variables WHERE variable_id = ?1",
                    params![variable_id],
                    |row| {
                        Ok(context_variable_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, bool>(5)?,
                            row.get::<_, Option<String>>(6)?,
                        ))
                    },
                );
                match row {
                    Ok(value) => Ok(Some(value)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let variable_id = variable_id.to_string();
                block_on(async move {
                    let row = sqlx::query(
                        "SELECT variable_id, scope_id, name, value_source_type, value_source_config, enabled, visibility_rule
                         FROM context_variables WHERE variable_id = $1",
                    )
                    .bind(variable_id)
                    .fetch_optional(&*pool)
                    .await?;

                    Ok::<Option<serde_json::Value>, sqlx::Error>(row.map(|row| {
                        context_variable_json(
                            row.try_get("variable_id").unwrap_or_default(),
                            row.try_get("scope_id").unwrap_or_default(),
                            row.try_get("name").unwrap_or_default(),
                            row.try_get("value_source_type").unwrap_or_default(),
                            row.try_get("value_source_config")
                                .unwrap_or_else(|_| "{}".to_string()),
                            row.try_get("enabled").unwrap_or(false),
                            row.try_get("visibility_rule").ok(),
                        )
                    }))
                })
                .map_err(memory_error)
            }
        }
    }

    /// Delete a context variable and any attached values.
    pub fn delete_context_variable(&self, variable_id: &str) -> SiliCrewResult<bool> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "DELETE FROM context_variable_values WHERE variable_id = ?1",
                    params![variable_id],
                )
                .map_err(memory_error)?;
                let rows = conn
                    .execute(
                        "DELETE FROM context_variables WHERE variable_id = ?1",
                        params![variable_id],
                    )
                    .map_err(memory_error)?;
                Ok(rows > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let variable_id = variable_id.to_string();
                block_on(async move {
                    sqlx::query("DELETE FROM context_variable_values WHERE variable_id = $1")
                        .bind(&variable_id)
                        .execute(&*pool)
                        .await?;
                    sqlx::query("DELETE FROM context_variables WHERE variable_id = $1")
                        .bind(&variable_id)
                        .execute(&*pool)
                        .await
                })
                .map_err(memory_error)
                .map(|rows| rows.rows_affected() > 0)
            }
        }
    }

    /// Insert or update a context variable value for a specific lookup key.
    pub fn upsert_context_variable_value(
        &self,
        value_id: &str,
        variable_id: &str,
        key: &str,
        data_json: &str,
    ) -> SiliCrewResult<()> {
        let updated_at = Utc::now().to_rfc3339();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO context_variable_values (value_id, variable_id, key, data_json, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(variable_id, key) DO UPDATE SET
                        data_json = excluded.data_json,
                        updated_at = excluded.updated_at",
                    params![value_id, variable_id, key, data_json, updated_at],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let value_id = value_id.to_string();
                let variable_id = variable_id.to_string();
                let key = key.to_string();
                let data_json = data_json.to_string();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO context_variable_values (value_id, variable_id, key, data_json, updated_at)
                         VALUES ($1, $2, $3, $4, $5)
                         ON CONFLICT(variable_id, key) DO UPDATE SET
                            data_json = EXCLUDED.data_json,
                            updated_at = EXCLUDED.updated_at",
                    )
                    .bind(value_id)
                    .bind(variable_id)
                    .bind(key)
                    .bind(data_json)
                    .bind(updated_at)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    /// Fetch a single context variable value by variable and key.
    pub fn get_context_variable_value(
        &self,
        variable_id: &str,
        key: &str,
    ) -> SiliCrewResult<Option<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let row = conn.query_row(
                    "SELECT value_id, variable_id, key, data_json, updated_at
                     FROM context_variable_values
                     WHERE variable_id = ?1 AND key = ?2",
                    params![variable_id, key],
                    |row| {
                        Ok(context_variable_value_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    },
                );
                match row {
                    Ok(value) => Ok(Some(value)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let variable_id = variable_id.to_string();
                let key = key.to_string();
                block_on(async move {
                    let row = sqlx::query(
                        "SELECT value_id, variable_id, key, data_json, updated_at
                         FROM context_variable_values
                         WHERE variable_id = $1 AND key = $2",
                    )
                    .bind(variable_id)
                    .bind(key)
                    .fetch_optional(&*pool)
                    .await?;

                    Ok::<Option<serde_json::Value>, sqlx::Error>(row.map(|row| {
                        context_variable_value_json(
                            row.try_get("value_id").unwrap_or_default(),
                            row.try_get("variable_id").unwrap_or_default(),
                            row.try_get("key").unwrap_or_default(),
                            row.try_get("data_json")
                                .unwrap_or_else(|_| "null".to_string()),
                            row.try_get("updated_at").unwrap_or_default(),
                        )
                    }))
                })
                .map_err(memory_error)
            }
        }
    }

    /// List all context variable values for a variable.
    pub fn list_context_variable_values(
        &self,
        variable_id: &str,
    ) -> SiliCrewResult<Vec<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT value_id, variable_id, key, data_json, updated_at
                         FROM context_variable_values
                         WHERE variable_id = ?1
                         ORDER BY key ASC",
                    )
                    .map_err(memory_error)?;
                let rows = stmt
                    .query_map(params![variable_id], |row| {
                        Ok(context_variable_value_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    })
                    .map_err(memory_error)?;
                Ok(rows.filter_map(|r| r.ok()).collect())
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let variable_id = variable_id.to_string();
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT value_id, variable_id, key, data_json, updated_at
                         FROM context_variable_values
                         WHERE variable_id = $1
                         ORDER BY key ASC",
                    )
                    .bind(variable_id)
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(context_variable_value_json(
                        row.try_get("value_id").map_err(memory_error)?,
                        row.try_get("variable_id").map_err(memory_error)?,
                        row.try_get("key").map_err(memory_error)?,
                        row.try_get("data_json")
                            .unwrap_or_else(|_| "null".to_string()),
                        row.try_get("updated_at").map_err(memory_error)?,
                    ));
                }
                Ok(out)
            }
        }
    }

    /// Delete a single context variable value by variable and key.
    pub fn delete_context_variable_value(
        &self,
        variable_id: &str,
        key: &str,
    ) -> SiliCrewResult<bool> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let rows = conn
                    .execute(
                        "DELETE FROM context_variable_values WHERE variable_id = ?1 AND key = ?2",
                        params![variable_id, key],
                    )
                    .map_err(memory_error)?;
                Ok(rows > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let variable_id = variable_id.to_string();
                let key = key.to_string();
                let rows = block_on(async move {
                    sqlx::query(
                        "DELETE FROM context_variable_values WHERE variable_id = $1 AND key = $2",
                    )
                    .bind(variable_id)
                    .bind(key)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
                Ok(rows.rows_affected() > 0)
            }
        }
    }

    /// Insert or update a canned response.
    pub fn upsert_canned_response(
        &self,
        response_id: &str,
        scope_id: &str,
        name: &str,
        template_text: &str,
        trigger_rule: Option<&str>,
        priority: i32,
        enabled: bool,
    ) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO canned_responses (response_id, scope_id, name, template_text, trigger_rule, priority, enabled)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(response_id) DO UPDATE SET
                        name = excluded.name,
                        template_text = excluded.template_text,
                        trigger_rule = excluded.trigger_rule,
                        priority = excluded.priority,
                        enabled = excluded.enabled",
                    params![response_id, scope_id, name, template_text, trigger_rule, priority, enabled as i64],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let response_id = response_id.to_string();
                let scope_id = scope_id.to_string();
                let name = name.to_string();
                let template_text = template_text.to_string();
                let trigger_rule = trigger_rule.map(ToOwned::to_owned);
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO canned_responses (response_id, scope_id, name, template_text, trigger_rule, priority, enabled)
                         VALUES ($1, $2, $3, $4, $5, $6, $7)
                         ON CONFLICT(response_id) DO UPDATE SET
                            name = EXCLUDED.name,
                            template_text = EXCLUDED.template_text,
                            trigger_rule = EXCLUDED.trigger_rule,
                            priority = EXCLUDED.priority,
                            enabled = EXCLUDED.enabled",
                    )
                    .bind(response_id)
                    .bind(scope_id)
                    .bind(name)
                    .bind(template_text)
                    .bind(trigger_rule)
                    .bind(priority)
                    .bind(enabled)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    /// List canned responses for a scope.
    pub fn list_canned_responses(&self, scope_id: &str) -> SiliCrewResult<Vec<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT response_id, scope_id, name, template_text, priority, enabled, trigger_rule
                         FROM canned_responses WHERE scope_id = ?1 ORDER BY priority DESC, name",
                    )
                    .map_err(memory_error)?;
                let rows = stmt
                    .query_map(params![scope_id], |row| {
                        Ok(canned_response_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i32>(4)?,
                            row.get::<_, bool>(5)?,
                            row.get::<_, Option<String>>(6)?,
                        ))
                    })
                    .map_err(memory_error)?;
                Ok(rows.filter_map(|r| r.ok()).collect())
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.to_string();
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT response_id, scope_id, name, template_text, priority, enabled, trigger_rule
                         FROM canned_responses WHERE scope_id = $1 ORDER BY priority DESC, name",
                    )
                    .bind(scope_id)
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(canned_response_json(
                        row.try_get("response_id").map_err(memory_error)?,
                        row.try_get("scope_id").map_err(memory_error)?,
                        row.try_get("name").map_err(memory_error)?,
                        row.try_get("template_text").map_err(memory_error)?,
                        row.try_get("priority").map_err(memory_error)?,
                        row.try_get::<bool, _>("enabled").map_err(memory_error)?,
                        row.try_get("trigger_rule").map_err(memory_error)?,
                    ));
                }
                Ok(out)
            }
        }
    }

    /// Fetch a single canned response by id.
    pub fn get_canned_response(
        &self,
        response_id: &str,
    ) -> SiliCrewResult<Option<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let row = conn.query_row(
                    "SELECT response_id, scope_id, name, template_text, priority, enabled, trigger_rule
                     FROM canned_responses WHERE response_id = ?1",
                    params![response_id],
                    |row| {
                        Ok(canned_response_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i32>(4)?,
                            row.get::<_, bool>(5)?,
                            row.get::<_, Option<String>>(6)?,
                        ))
                    },
                );
                match row {
                    Ok(value) => Ok(Some(value)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let response_id = response_id.to_string();
                block_on(async move {
                    let row = sqlx::query(
                        "SELECT response_id, scope_id, name, template_text, priority, enabled, trigger_rule
                         FROM canned_responses WHERE response_id = $1",
                    )
                    .bind(response_id)
                    .fetch_optional(&*pool)
                    .await?;

                    Ok::<Option<serde_json::Value>, sqlx::Error>(row.map(|row| {
                        canned_response_json(
                            row.try_get("response_id").unwrap_or_default(),
                            row.try_get("scope_id").unwrap_or_default(),
                            row.try_get("name").unwrap_or_default(),
                            row.try_get("template_text").unwrap_or_default(),
                            row.try_get("priority").unwrap_or_default(),
                            row.try_get("enabled").unwrap_or(false),
                            row.try_get("trigger_rule").ok(),
                        )
                    }))
                })
                .map_err(memory_error)
            }
        }
    }

    /// Delete a canned response by id.
    pub fn delete_canned_response(&self, response_id: &str) -> SiliCrewResult<bool> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let rows = conn
                    .execute(
                        "DELETE FROM canned_responses WHERE response_id = ?1",
                        params![response_id],
                    )
                    .map_err(memory_error)?;
                Ok(rows > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let response_id = response_id.to_string();
                let rows = block_on(async move {
                    sqlx::query("DELETE FROM canned_responses WHERE response_id = $1")
                        .bind(response_id)
                        .execute(&*pool)
                        .await
                })
                .map_err(memory_error)?;
                Ok(rows.rows_affected() > 0)
            }
        }
    }

    /// Join turn traces with explainability records.
    pub fn enrich_turn_traces_json(
        &self,
        traces: Vec<TurnTraceRecord>,
    ) -> SiliCrewResult<Vec<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut out = Vec::with_capacity(traces.len());
                for trace in traces {
                    out.push(enrich_trace_with_sqlite(&conn, trace).map_err(memory_error)?);
                }
                Ok(out)
            }
            SharedDb::Postgres(pool) => {
                let mut out = Vec::with_capacity(traces.len());
                for trace in traces {
                    out.push(enrich_trace_with_postgres(Arc::clone(pool), trace)?);
                }
                Ok(out)
            }
        }
    }

    /// Insert a guideline relationship.
    pub fn create_guideline_relationship(
        &self,
        relationship_id: &str,
        scope_id: &str,
        from_guideline_id: &str,
        to_guideline_id: &str,
        relation_type: &str,
    ) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO guideline_relationships
                        (relationship_id, scope_id, from_guideline_id, to_guideline_id, relation_type)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(scope_id, from_guideline_id, to_guideline_id, relation_type) DO NOTHING",
                    params![relationship_id, scope_id, from_guideline_id, to_guideline_id, relation_type],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let relationship_id = relationship_id.to_string();
                let scope_id = scope_id.to_string();
                let from_guideline_id = from_guideline_id.to_string();
                let to_guideline_id = to_guideline_id.to_string();
                let relation_type = relation_type.to_string();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO guideline_relationships
                            (relationship_id, scope_id, from_guideline_id, to_guideline_id, relation_type)
                         VALUES ($1, $2, $3, $4, $5)
                         ON CONFLICT(scope_id, from_guideline_id, to_guideline_id, relation_type) DO NOTHING",
                    )
                    .bind(relationship_id)
                    .bind(scope_id)
                    .bind(from_guideline_id)
                    .bind(to_guideline_id)
                    .bind(relation_type)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    /// List guideline relationships for a scope.
    pub fn list_guideline_relationships(
        &self,
        scope_id: &str,
    ) -> SiliCrewResult<Vec<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT relationship_id, scope_id, from_guideline_id, to_guideline_id, relation_type
                         FROM guideline_relationships WHERE scope_id = ?1
                         ORDER BY relation_type, from_guideline_id",
                    )
                    .map_err(memory_error)?;
                let rows = stmt
                    .query_map(params![scope_id], |row| {
                        Ok(guideline_relationship_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    })
                    .map_err(memory_error)?;
                Ok(rows.filter_map(|r| r.ok()).collect())
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let scope_id = scope_id.to_string();
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT relationship_id, scope_id, from_guideline_id, to_guideline_id, relation_type
                         FROM guideline_relationships WHERE scope_id = $1
                         ORDER BY relation_type, from_guideline_id",
                    )
                    .bind(scope_id)
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(guideline_relationship_json(
                        row.try_get("relationship_id").map_err(memory_error)?,
                        row.try_get("scope_id").map_err(memory_error)?,
                        row.try_get("from_guideline_id").map_err(memory_error)?,
                        row.try_get("to_guideline_id").map_err(memory_error)?,
                        row.try_get("relation_type").map_err(memory_error)?,
                    ));
                }
                Ok(out)
            }
        }
    }

    /// Delete a guideline relationship by id.
    pub fn delete_guideline_relationship(&self, relationship_id: &str) -> SiliCrewResult<bool> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let rows = conn
                    .execute(
                        "DELETE FROM guideline_relationships WHERE relationship_id = ?1",
                        params![relationship_id],
                    )
                    .map_err(memory_error)?;
                Ok(rows > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let relationship_id = relationship_id.to_string();
                let rows = block_on(async move {
                    sqlx::query("DELETE FROM guideline_relationships WHERE relationship_id = $1")
                        .bind(relationship_id)
                        .execute(&*pool)
                        .await
                })
                .map_err(memory_error)?;
                Ok(rows.rows_affected() > 0)
            }
        }
    }

    /// Insert or update a journey state.
    pub fn upsert_journey_state(
        &self,
        state_id: &str,
        journey_id: &str,
        name: &str,
        description: Option<&str>,
        required_fields_json: &str,
        guideline_actions_json: &str,
    ) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO journey_states (state_id, journey_id, name, description, required_fields, guideline_actions_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(journey_id, name) DO UPDATE SET
                        description = excluded.description,
                        required_fields = excluded.required_fields,
                        guideline_actions_json = excluded.guideline_actions_json",
                    params![state_id, journey_id, name, description, required_fields_json, guideline_actions_json],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let state_id = state_id.to_string();
                let journey_id = journey_id.to_string();
                let name = name.to_string();
                let description = description.map(ToOwned::to_owned);
                let required_fields_json = required_fields_json.to_string();
                let guideline_actions_json = guideline_actions_json.to_string();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO journey_states (state_id, journey_id, name, description, required_fields, guideline_actions_json)
                         VALUES ($1, $2, $3, $4, $5, $6)
                         ON CONFLICT(journey_id, name) DO UPDATE SET
                            description = EXCLUDED.description,
                            required_fields = EXCLUDED.required_fields,
                            guideline_actions_json = EXCLUDED.guideline_actions_json",
                    )
                    .bind(state_id)
                    .bind(journey_id)
                    .bind(name)
                    .bind(description)
                    .bind(required_fields_json)
                    .bind(guideline_actions_json)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    /// List journey states for a journey.
    pub fn list_journey_states(&self, journey_id: &str) -> SiliCrewResult<Vec<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT state_id, journey_id, name, description, required_fields, COALESCE(guideline_actions_json, '[]')
                         FROM journey_states WHERE journey_id = ?1 ORDER BY name ASC",
                    )
                    .map_err(memory_error)?;
                let rows = stmt
                    .query_map(params![journey_id], |row| {
                        Ok(journey_state_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, String>(4).unwrap_or_else(|_| "[]".into()),
                            row.get::<_, String>(5).unwrap_or_else(|_| "[]".into()),
                        ))
                    })
                    .map_err(memory_error)?;
                Ok(rows.filter_map(|r| r.ok()).collect())
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let journey_id = journey_id.to_string();
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT state_id, journey_id, name, description, required_fields, COALESCE(guideline_actions_json, '[]') AS guideline_actions_json
                         FROM journey_states WHERE journey_id = $1 ORDER BY name ASC",
                    )
                    .bind(journey_id)
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(journey_state_json(
                        row.try_get("state_id").map_err(memory_error)?,
                        row.try_get("journey_id").map_err(memory_error)?,
                        row.try_get("name").map_err(memory_error)?,
                        row.try_get("description").map_err(memory_error)?,
                        row.try_get("required_fields").map_err(memory_error)?,
                        row.try_get("guideline_actions_json")
                            .map_err(memory_error)?,
                    ));
                }
                Ok(out)
            }
        }
    }

    /// Fetch a single journey state by id.
    pub fn get_journey_state(&self, state_id: &str) -> SiliCrewResult<Option<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let row = conn.query_row(
                    "SELECT state_id, journey_id, name, description, required_fields,
                            COALESCE(guideline_actions_json, '[]')
                     FROM journey_states WHERE state_id = ?1",
                    params![state_id],
                    |row| {
                        Ok(journey_state_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, String>(4).unwrap_or_else(|_| "[]".into()),
                            row.get::<_, String>(5).unwrap_or_else(|_| "[]".into()),
                        ))
                    },
                );
                match row {
                    Ok(value) => Ok(Some(value)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let state_id = state_id.to_string();
                block_on(async move {
                    let row = sqlx::query(
                        "SELECT state_id, journey_id, name, description, required_fields,
                                COALESCE(guideline_actions_json, '[]') AS guideline_actions_json
                         FROM journey_states WHERE state_id = $1",
                    )
                    .bind(state_id)
                    .fetch_optional(&*pool)
                    .await?;

                    Ok::<Option<serde_json::Value>, sqlx::Error>(row.map(|row| {
                        journey_state_json(
                            row.try_get("state_id").unwrap_or_default(),
                            row.try_get("journey_id").unwrap_or_default(),
                            row.try_get("name").unwrap_or_default(),
                            row.try_get("description").ok(),
                            row.try_get("required_fields")
                                .unwrap_or_else(|_| "[]".to_string()),
                            row.try_get("guideline_actions_json")
                                .unwrap_or_else(|_| "[]".to_string()),
                        )
                    }))
                })
                .map_err(memory_error)
            }
        }
    }

    /// Update an existing journey state by id.
    pub fn update_journey_state(
        &self,
        state_id: &str,
        name: &str,
        description: Option<&str>,
        required_fields_json: &str,
        guideline_actions_json: &str,
    ) -> SiliCrewResult<bool> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let rows = conn
                    .execute(
                        "UPDATE journey_states
                         SET name = ?1, description = ?2, required_fields = ?3, guideline_actions_json = ?4
                         WHERE state_id = ?5",
                        params![
                            name,
                            description,
                            required_fields_json,
                            guideline_actions_json,
                            state_id,
                        ],
                    )
                    .map_err(memory_error)?;
                Ok(rows > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let state_id = state_id.to_string();
                let name = name.to_string();
                let description = description.map(ToOwned::to_owned);
                let required_fields_json = required_fields_json.to_string();
                let guideline_actions_json = guideline_actions_json.to_string();
                let rows = block_on(async move {
                    sqlx::query(
                        "UPDATE journey_states
                         SET name = $1, description = $2, required_fields = $3, guideline_actions_json = $4
                         WHERE state_id = $5",
                    )
                    .bind(name)
                    .bind(description)
                    .bind(required_fields_json)
                    .bind(guideline_actions_json)
                    .bind(state_id)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
                Ok(rows.rows_affected() > 0)
            }
        }
    }

    /// Delete a journey state and dependent transitions / active instances.
    pub fn delete_journey_state(&self, state_id: &str) -> SiliCrewResult<bool> {
        let Some(existing) = self.get_journey_state(state_id)? else {
            return Ok(false);
        };
        let journey_id = existing
            .get("journey_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "UPDATE journeys SET entry_state_id = NULL
                     WHERE journey_id = ?1 AND entry_state_id = ?2",
                    params![journey_id.as_str(), state_id],
                )
                .map_err(memory_error)?;
                conn.execute(
                    "UPDATE session_bindings
                     SET active_journey_instance_id = NULL
                     WHERE active_journey_instance_id IN (
                        SELECT journey_instance_id FROM journey_instances WHERE current_state_id = ?1
                     )",
                    params![state_id],
                )
                .map_err(memory_error)?;
                conn.execute(
                    "DELETE FROM journey_instances WHERE current_state_id = ?1",
                    params![state_id],
                )
                .map_err(memory_error)?;
                conn.execute(
                    "DELETE FROM journey_transitions WHERE from_state_id = ?1 OR to_state_id = ?1",
                    params![state_id],
                )
                .map_err(memory_error)?;
                let rows = conn
                    .execute(
                        "DELETE FROM journey_states WHERE state_id = ?1",
                        params![state_id],
                    )
                    .map_err(memory_error)?;
                Ok(rows > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let state_id = state_id.to_string();
                let journey_id_for_entry = journey_id;
                block_on(async move {
                    sqlx::query(
                        "UPDATE journeys SET entry_state_id = NULL
                         WHERE journey_id = $1 AND entry_state_id = $2",
                    )
                    .bind(&journey_id_for_entry)
                    .bind(&state_id)
                    .execute(&*pool)
                    .await?;
                    sqlx::query(
                        "UPDATE session_bindings
                         SET active_journey_instance_id = NULL
                         WHERE active_journey_instance_id IN (
                            SELECT journey_instance_id FROM journey_instances WHERE current_state_id = $1
                         )",
                    )
                    .bind(&state_id)
                    .execute(&*pool)
                    .await?;
                    sqlx::query("DELETE FROM journey_instances WHERE current_state_id = $1")
                        .bind(&state_id)
                        .execute(&*pool)
                        .await?;
                    sqlx::query(
                        "DELETE FROM journey_transitions WHERE from_state_id = $1 OR to_state_id = $1",
                    )
                    .bind(&state_id)
                    .execute(&*pool)
                    .await?;
                    sqlx::query("DELETE FROM journey_states WHERE state_id = $1")
                        .bind(&state_id)
                        .execute(&*pool)
                        .await
                })
                .map(|rows| rows.rows_affected() > 0)
                .map_err(memory_error)
            }
        }
    }

    /// Insert or update a journey transition.
    pub fn upsert_journey_transition(
        &self,
        transition_id: &str,
        journey_id: &str,
        from_state_id: &str,
        to_state_id: &str,
        condition_config_json: &str,
        transition_type: &str,
    ) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO journey_transitions
                        (transition_id, journey_id, from_state_id, to_state_id, condition_config, transition_type)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(journey_id, from_state_id, to_state_id, transition_type) DO UPDATE SET
                        condition_config = excluded.condition_config",
                    params![transition_id, journey_id, from_state_id, to_state_id, condition_config_json, transition_type],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let transition_id = transition_id.to_string();
                let journey_id = journey_id.to_string();
                let from_state_id = from_state_id.to_string();
                let to_state_id = to_state_id.to_string();
                let condition_config_json = condition_config_json.to_string();
                let transition_type = transition_type.to_string();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO journey_transitions
                            (transition_id, journey_id, from_state_id, to_state_id, condition_config, transition_type)
                         VALUES ($1, $2, $3, $4, $5, $6)
                         ON CONFLICT(journey_id, from_state_id, to_state_id, transition_type) DO UPDATE SET
                            condition_config = EXCLUDED.condition_config",
                    )
                    .bind(transition_id)
                    .bind(journey_id)
                    .bind(from_state_id)
                    .bind(to_state_id)
                    .bind(condition_config_json)
                    .bind(transition_type)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    /// List journey transitions for a journey.
    pub fn list_journey_transitions(
        &self,
        journey_id: &str,
    ) -> SiliCrewResult<Vec<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT transition_id, journey_id, from_state_id, to_state_id, condition_config, transition_type
                         FROM journey_transitions WHERE journey_id = ?1
                         ORDER BY from_state_id, transition_type",
                    )
                    .map_err(memory_error)?;
                let rows = stmt
                    .query_map(params![journey_id], |row| {
                        Ok(journey_transition_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4).unwrap_or_else(|_| "{}".into()),
                            row.get::<_, String>(5)?,
                        ))
                    })
                    .map_err(memory_error)?;
                Ok(rows.filter_map(|r| r.ok()).collect())
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let journey_id = journey_id.to_string();
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT transition_id, journey_id, from_state_id, to_state_id, condition_config, transition_type
                         FROM journey_transitions WHERE journey_id = $1
                         ORDER BY from_state_id, transition_type",
                    )
                    .bind(journey_id)
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    out.push(journey_transition_json(
                        row.try_get("transition_id").map_err(memory_error)?,
                        row.try_get("journey_id").map_err(memory_error)?,
                        row.try_get("from_state_id").map_err(memory_error)?,
                        row.try_get("to_state_id").map_err(memory_error)?,
                        row.try_get("condition_config").map_err(memory_error)?,
                        row.try_get("transition_type").map_err(memory_error)?,
                    ));
                }
                Ok(out)
            }
        }
    }

    /// Fetch a single journey transition by id.
    pub fn get_journey_transition(
        &self,
        transition_id: &str,
    ) -> SiliCrewResult<Option<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let row = conn.query_row(
                    "SELECT transition_id, journey_id, from_state_id, to_state_id, condition_config, transition_type
                     FROM journey_transitions WHERE transition_id = ?1",
                    params![transition_id],
                    |row| {
                        Ok(journey_transition_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4).unwrap_or_else(|_| "{}".into()),
                            row.get::<_, String>(5)?,
                        ))
                    },
                );
                match row {
                    Ok(value) => Ok(Some(value)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let transition_id = transition_id.to_string();
                block_on(async move {
                    let row = sqlx::query(
                        "SELECT transition_id, journey_id, from_state_id, to_state_id, condition_config, transition_type
                         FROM journey_transitions WHERE transition_id = $1",
                    )
                    .bind(transition_id)
                    .fetch_optional(&*pool)
                    .await?;

                    Ok::<Option<serde_json::Value>, sqlx::Error>(row.map(|row| {
                        journey_transition_json(
                            row.try_get("transition_id").unwrap_or_default(),
                            row.try_get("journey_id").unwrap_or_default(),
                            row.try_get("from_state_id").unwrap_or_default(),
                            row.try_get("to_state_id").unwrap_or_default(),
                            row.try_get("condition_config")
                                .unwrap_or_else(|_| "{}".to_string()),
                            row.try_get("transition_type").unwrap_or_default(),
                        )
                    }))
                })
                .map_err(memory_error)
            }
        }
    }

    /// Update a journey transition by id.
    pub fn update_journey_transition(
        &self,
        transition_id: &str,
        from_state_id: &str,
        to_state_id: &str,
        condition_config_json: &str,
        transition_type: &str,
    ) -> SiliCrewResult<bool> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let rows = conn
                    .execute(
                        "UPDATE journey_transitions
                         SET from_state_id = ?1, to_state_id = ?2, condition_config = ?3, transition_type = ?4
                         WHERE transition_id = ?5",
                        params![
                            from_state_id,
                            to_state_id,
                            condition_config_json,
                            transition_type,
                            transition_id,
                        ],
                    )
                    .map_err(memory_error)?;
                Ok(rows > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let transition_id = transition_id.to_string();
                let from_state_id = from_state_id.to_string();
                let to_state_id = to_state_id.to_string();
                let condition_config_json = condition_config_json.to_string();
                let transition_type = transition_type.to_string();
                let rows = block_on(async move {
                    sqlx::query(
                        "UPDATE journey_transitions
                         SET from_state_id = $1, to_state_id = $2, condition_config = $3, transition_type = $4
                         WHERE transition_id = $5",
                    )
                    .bind(from_state_id)
                    .bind(to_state_id)
                    .bind(condition_config_json)
                    .bind(transition_type)
                    .bind(transition_id)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
                Ok(rows.rows_affected() > 0)
            }
        }
    }

    /// Delete a journey transition by id.
    pub fn delete_journey_transition(&self, transition_id: &str) -> SiliCrewResult<bool> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let rows = conn
                    .execute(
                        "DELETE FROM journey_transitions WHERE transition_id = ?1",
                        params![transition_id],
                    )
                    .map_err(memory_error)?;
                Ok(rows > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let transition_id = transition_id.to_string();
                let rows = block_on(async move {
                    sqlx::query("DELETE FROM journey_transitions WHERE transition_id = $1")
                        .bind(transition_id)
                        .execute(&*pool)
                        .await
                })
                .map_err(memory_error)?;
                Ok(rows.rows_affected() > 0)
            }
        }
    }

    /// Return the active journey for a session.
    pub fn get_active_journey_for_session(
        &self,
        session_id: &str,
    ) -> SiliCrewResult<Option<serde_json::Value>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let row = conn.query_row(
                    "SELECT ji.journey_instance_id, ji.journey_id, ji.current_state_id,
                            ji.status, ji.state_payload, ji.updated_at,
                            j.name as journey_name,
                            js.name as state_name, js.description as state_description
                     FROM journey_instances ji
                     LEFT JOIN journeys j ON j.journey_id = ji.journey_id
                     LEFT JOIN journey_states js ON js.state_id = ji.current_state_id
                     WHERE ji.session_id = ?1 AND ji.status = 'active'
                     ORDER BY ji.updated_at DESC LIMIT 1",
                    params![session_id],
                    |row| {
                        Ok(active_journey_json(
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4).unwrap_or_default(),
                            row.get::<_, String>(5).unwrap_or_default(),
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, Option<String>>(7)?,
                            row.get::<_, Option<String>>(8)?,
                        ))
                    },
                );
                match row {
                    Ok(v) => Ok(Some(v)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let session_id = session_id.to_string();
                let row = block_on(async move {
                    sqlx::query(
                        "SELECT ji.journey_instance_id, ji.journey_id, ji.current_state_id,
                                ji.status, ji.state_payload, ji.updated_at,
                                j.name as journey_name,
                                js.name as state_name, js.description as state_description
                         FROM journey_instances ji
                         LEFT JOIN journeys j ON j.journey_id = ji.journey_id
                         LEFT JOIN journey_states js ON js.state_id = ji.current_state_id
                         WHERE ji.session_id = $1 AND ji.status = 'active'
                         ORDER BY ji.updated_at DESC LIMIT 1",
                    )
                    .bind(session_id)
                    .fetch_optional(&*pool)
                    .await
                })
                .map_err(memory_error)?;

                match row {
                    Some(row) => Ok(Some(active_journey_json(
                        row.try_get("journey_instance_id").map_err(memory_error)?,
                        row.try_get("journey_id").map_err(memory_error)?,
                        row.try_get("current_state_id").map_err(memory_error)?,
                        row.try_get("status").map_err(memory_error)?,
                        row.try_get("state_payload").map_err(memory_error)?,
                        row.try_get("updated_at").map_err(memory_error)?,
                        row.try_get("journey_name").map_err(memory_error)?,
                        row.try_get("state_name").map_err(memory_error)?,
                        row.try_get("state_description").map_err(memory_error)?,
                    ))),
                    None => Ok(None),
                }
            }
        }
    }
}

fn scope_from_row(
    row: (String, String, String, String, String, String),
) -> SiliCrewResult<ControlScope> {
    Ok(ControlScope {
        scope_id: ScopeId::from(row.0),
        name: row.1,
        scope_type: row.2,
        status: row.3,
        created_at: parse_timestamp(&row.4)?,
        updated_at: parse_timestamp(&row.5)?,
    })
}

fn trace_from_row(
    row: (
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        String,
    ),
) -> SiliCrewResult<TurnTraceRecord> {
    Ok(TurnTraceRecord {
        trace_id: parse_uuid(&row.0)
            .map(TraceId)
            .map_err(memory_parse_error)?,
        scope_id: ScopeId::from(row.1),
        session_id: parse_uuid(&row.2)
            .map(SessionId)
            .map_err(memory_parse_error)?,
        agent_id: parse_uuid(&row.3)
            .map(AgentId)
            .map_err(memory_parse_error)?,
        channel_type: row.4,
        request_message_ref: row.5,
        compiled_context_hash: row.6,
        release_version: row.7,
        response_mode: ResponseMode::from_str(&row.8).map_err(SiliCrewError::Memory)?,
        created_at: parse_timestamp(&row.9)?,
    })
}

fn policy_match_record_from_row(
    row: (String, String, String, String, String),
) -> SiliCrewResult<PolicyMatchRecord> {
    Ok(PolicyMatchRecord {
        record_id: parse_uuid(&row.0)
            .map(TraceId)
            .map_err(memory_parse_error)?,
        trace_id: parse_uuid(&row.1)
            .map(TraceId)
            .map_err(memory_parse_error)?,
        observation_hits_json: row.2,
        guideline_hits_json: row.3,
        guideline_exclusions_json: row.4,
    })
}

fn session_binding_from_sqlite_row(
    row: &rusqlite::Row<'_>,
) -> Result<silicrew_types::control::SessionBinding, rusqlite::Error> {
    Ok(silicrew_types::control::SessionBinding {
        binding_id: row.get(0)?,
        scope_id: ScopeId::new(row.get::<_, String>(1)?),
        channel_type: row.get(2)?,
        external_user_id: row.get(3)?,
        external_chat_id: row.get(4)?,
        agent_id: row.get(5)?,
        session_id: row.get(6)?,
        manual_mode: row.get::<_, i64>(7)? != 0,
        active_journey_instance_id: row.get(8)?,
    })
}

fn session_binding_from_pg_row(
    row: &sqlx::postgres::PgRow,
) -> SiliCrewResult<silicrew_types::control::SessionBinding> {
    Ok(silicrew_types::control::SessionBinding {
        binding_id: row.try_get("binding_id").map_err(memory_error)?,
        scope_id: ScopeId::new(row.try_get::<String, _>("scope_id").map_err(memory_error)?),
        channel_type: row.try_get("channel_type").map_err(memory_error)?,
        external_user_id: row.try_get("external_user_id").map_err(memory_error)?,
        external_chat_id: row.try_get("external_chat_id").map_err(memory_error)?,
        agent_id: row.try_get("agent_id").map_err(memory_error)?,
        session_id: row.try_get("session_id").map_err(memory_error)?,
        manual_mode: row
            .try_get::<bool, _>("manual_mode")
            .map_err(memory_error)?,
        active_journey_instance_id: row
            .try_get("active_journey_instance_id")
            .map_err(memory_error)?,
    })
}

fn parse_timestamp(value: &str) -> SiliCrewResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").map(|dt| dt.and_utc())
        })
        .map_err(memory_parse_error)
}

fn parse_uuid(value: &str) -> Result<uuid::Uuid, uuid::Error> {
    uuid::Uuid::parse_str(value)
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn memory_error<E: std::fmt::Display>(error: E) -> SiliCrewError {
    SiliCrewError::Memory(error.to_string())
}

fn memory_parse_error<E: std::fmt::Display>(error: E) -> SiliCrewError {
    SiliCrewError::Memory(error.to_string())
}

fn parse_json_array(value: &str) -> serde_json::Value {
    serde_json::from_str(value).unwrap_or_else(|_| serde_json::json!([]))
}

fn parse_json_object(value: &str) -> serde_json::Value {
    serde_json::from_str(value).unwrap_or_else(|_| serde_json::json!({}))
}

fn enrich_trace_with_sqlite(
    conn: &rusqlite::Connection,
    trace: TurnTraceRecord,
) -> Result<serde_json::Value, rusqlite::Error> {
    let tid = trace.trace_id.0.to_string();
    let policy_row = conn
        .query_row(
            "SELECT observation_hits_json, guideline_hits_json, guideline_exclusions_json
             FROM policy_match_records WHERE trace_id = ?1 LIMIT 1",
            params![&tid],
            |row| {
                Ok((
                    row.get::<_, String>(0).unwrap_or_else(|_| "[]".into()),
                    row.get::<_, String>(1).unwrap_or_else(|_| "[]".into()),
                    row.get::<_, String>(2).unwrap_or_else(|_| "[]".into()),
                ))
            },
        )
        .ok();
    let tool_row = conn
        .query_row(
            "SELECT allowed_tools_json, authorization_reasons_json, approval_requirements_json
             FROM tool_authorization_records WHERE trace_id = ?1 LIMIT 1",
            params![&tid],
            |row| {
                Ok((
                    row.get::<_, String>(0).unwrap_or_else(|_| "[]".into()),
                    row.get::<_, String>(1).unwrap_or_else(|_| "{}".into()),
                    row.get::<_, String>(2).unwrap_or_else(|_| "{}".into()),
                ))
            },
        )
        .ok();
    let journey_row = conn
        .query_row(
            "SELECT before_state_id, after_state_id, decision_json
             FROM journey_transition_records WHERE trace_id = ?1 LIMIT 1",
            params![&tid],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2).unwrap_or_else(|_| "{}".into()),
                ))
            },
        )
        .ok();
    Ok(enriched_trace_json(
        trace,
        policy_row,
        tool_row,
        journey_row,
    ))
}

fn enrich_trace_with_postgres(
    pool: Arc<sqlx::PgPool>,
    trace: TurnTraceRecord,
) -> SiliCrewResult<serde_json::Value> {
    let tid = trace.trace_id.0.to_string();
    let policy_row = block_on({
        let pool = Arc::clone(&pool);
        let tid = tid.clone();
        async move {
            sqlx::query(
                "SELECT observation_hits_json, guideline_hits_json, guideline_exclusions_json
                 FROM policy_match_records WHERE trace_id = $1 LIMIT 1",
            )
            .bind(tid)
            .fetch_optional(&*pool)
            .await
        }
    })
    .map_err(memory_error)?
    .map(|row| {
        Ok::<_, SiliCrewError>((
            row.try_get("observation_hits_json").map_err(memory_error)?,
            row.try_get("guideline_hits_json").map_err(memory_error)?,
            row.try_get("guideline_exclusions_json")
                .map_err(memory_error)?,
        ))
    })
    .transpose()?;
    let tool_row = block_on({
        let pool = Arc::clone(&pool);
        let tid = tid.clone();
        async move {
            sqlx::query(
                "SELECT allowed_tools_json, authorization_reasons_json, approval_requirements_json
                 FROM tool_authorization_records WHERE trace_id = $1 LIMIT 1",
            )
            .bind(tid)
            .fetch_optional(&*pool)
            .await
        }
    })
    .map_err(memory_error)?
    .map(|row| {
        Ok::<_, SiliCrewError>((
            row.try_get("allowed_tools_json").map_err(memory_error)?,
            row.try_get("authorization_reasons_json")
                .map_err(memory_error)?,
            row.try_get("approval_requirements_json")
                .map_err(memory_error)?,
        ))
    })
    .transpose()?;
    let journey_row = block_on({
        let pool = Arc::clone(&pool);
        async move {
            sqlx::query(
                "SELECT before_state_id, after_state_id, decision_json
                 FROM journey_transition_records WHERE trace_id = $1 LIMIT 1",
            )
            .bind(tid)
            .fetch_optional(&*pool)
            .await
        }
    })
    .map_err(memory_error)?
    .map(|row| {
        Ok::<_, SiliCrewError>((
            row.try_get("before_state_id").map_err(memory_error)?,
            row.try_get("after_state_id").map_err(memory_error)?,
            row.try_get("decision_json").map_err(memory_error)?,
        ))
    })
    .transpose()?;
    Ok(enriched_trace_json(
        trace,
        policy_row,
        tool_row,
        journey_row,
    ))
}

fn enriched_trace_json(
    trace: TurnTraceRecord,
    policy_row: Option<(String, String, String)>,
    tool_row: Option<(String, String, String)>,
    journey_row: Option<(Option<String>, Option<String>, String)>,
) -> serde_json::Value {
    serde_json::json!({
        "trace_id": trace.trace_id,
        "scope_id": trace.scope_id,
        "session_id": trace.session_id,
        "agent_id": trace.agent_id,
        "channel_type": trace.channel_type,
        "release_version": trace.release_version,
        "response_mode": trace.response_mode,
        "created_at": trace.created_at,
        "observation_hits": policy_row.as_ref().map(|r| parse_json_array(&r.0)).unwrap_or_else(|| serde_json::json!([])),
        "guideline_hits": policy_row.as_ref().map(|r| parse_json_array(&r.1)).unwrap_or_else(|| serde_json::json!([])),
        "guideline_exclusions": policy_row.as_ref().map(|r| parse_json_array(&r.2)).unwrap_or_else(|| serde_json::json!([])),
        "allowed_tools": tool_row.as_ref().map(|r| parse_json_array(&r.0)).unwrap_or_else(|| serde_json::json!([])),
        "authorization_reasons": tool_row.as_ref().map(|r| parse_json_object(&r.1)).unwrap_or_else(|| serde_json::json!({})),
        "approval_required_tools": tool_row.as_ref().map(|r| parse_json_object(&r.2)).unwrap_or_else(|| serde_json::json!({})),
        "journey_before_state": journey_row.as_ref().and_then(|r| r.0.clone()),
        "journey_after_state": journey_row.as_ref().and_then(|r| r.1.clone()),
        "journey_decision": journey_row.as_ref().map(|r| parse_json_object(&r.2)).unwrap_or_else(|| serde_json::json!({})),
    })
}

fn retriever_json(
    retriever_id: String,
    scope_id: String,
    name: String,
    retriever_type: String,
    config_json: String,
    enabled: bool,
) -> serde_json::Value {
    serde_json::json!({
        "retriever_id": retriever_id,
        "scope_id": scope_id,
        "name": name,
        "retriever_type": retriever_type,
        "config_json": config_json,
        "enabled": enabled,
    })
}

fn retriever_binding_json(
    binding_id: String,
    scope_id: String,
    retriever_id: String,
    bind_type: String,
    bind_ref: String,
) -> serde_json::Value {
    serde_json::json!({
        "binding_id": binding_id,
        "scope_id": scope_id,
        "retriever_id": retriever_id,
        "bind_type": bind_type,
        "bind_ref": bind_ref,
    })
}

fn release_json(
    release_id: String,
    scope_id: String,
    version: String,
    status: String,
    published_by: String,
    created_at: String,
) -> serde_json::Value {
    serde_json::json!({
        "release_id": release_id,
        "scope_id": scope_id,
        "version": version,
        "status": status,
        "published_by": published_by,
        "created_at": created_at,
    })
}

fn handoff_json(
    handoff_id: String,
    scope_id: String,
    session_id: String,
    reason: String,
    summary: Option<String>,
    status: String,
    created_at: String,
    updated_at: String,
) -> serde_json::Value {
    serde_json::json!({
        "handoff_id": handoff_id,
        "scope_id": scope_id,
        "session_id": session_id,
        "reason": reason,
        "summary": summary,
        "status": status,
        "created_at": created_at,
        "updated_at": updated_at,
    })
}

fn glossary_term_json(
    term_id: String,
    scope_id: String,
    name: String,
    description: String,
    synonyms_json: String,
    enabled: bool,
    always_include: bool,
) -> serde_json::Value {
    serde_json::json!({
        "term_id": term_id,
        "scope_id": scope_id,
        "name": name,
        "description": description,
        "synonyms_json": synonyms_json,
        "enabled": enabled,
        "always_include": always_include,
    })
}

fn context_variable_json(
    variable_id: String,
    scope_id: String,
    name: String,
    value_source_type: String,
    value_source_config: String,
    enabled: bool,
    visibility_rule: Option<String>,
) -> serde_json::Value {
    serde_json::json!({
        "variable_id": variable_id,
        "scope_id": scope_id,
        "name": name,
        "value_source_type": value_source_type,
        "value_source_config": value_source_config,
        "enabled": enabled,
        "visibility_rule": visibility_rule,
    })
}

fn context_variable_value_json(
    value_id: String,
    variable_id: String,
    key: String,
    data_json: String,
    updated_at: String,
) -> serde_json::Value {
    let data = serde_json::from_str::<serde_json::Value>(&data_json)
        .unwrap_or_else(|_| serde_json::Value::String(data_json));
    serde_json::json!({
        "value_id": value_id,
        "variable_id": variable_id,
        "key": key,
        "data": data,
        "updated_at": updated_at,
    })
}

fn canned_response_json(
    response_id: String,
    scope_id: String,
    name: String,
    template_text: String,
    priority: i32,
    enabled: bool,
    trigger_rule: Option<String>,
) -> serde_json::Value {
    serde_json::json!({
        "response_id": response_id,
        "scope_id": scope_id,
        "name": name,
        "template_text": template_text,
        "priority": priority,
        "enabled": enabled,
        "trigger_rule": trigger_rule,
    })
}

fn guideline_relationship_json(
    relationship_id: String,
    scope_id: String,
    from_guideline_id: String,
    to_guideline_id: String,
    relation_type: String,
) -> serde_json::Value {
    serde_json::json!({
        "relationship_id": relationship_id,
        "scope_id": scope_id,
        "from_guideline_id": from_guideline_id,
        "to_guideline_id": to_guideline_id,
        "relation_type": relation_type,
    })
}

fn journey_state_json(
    state_id: String,
    journey_id: String,
    name: String,
    description: Option<String>,
    required_fields_json: String,
    guideline_actions_json: String,
) -> serde_json::Value {
    let required_fields: Vec<String> =
        serde_json::from_str(&required_fields_json).unwrap_or_default();
    let guideline_actions: Vec<String> =
        serde_json::from_str(&guideline_actions_json).unwrap_or_default();
    serde_json::json!({
        "state_id": state_id,
        "journey_id": journey_id,
        "name": name,
        "description": description,
        "required_fields": required_fields,
        "guideline_actions": guideline_actions,
    })
}

fn journey_transition_json(
    transition_id: String,
    journey_id: String,
    from_state_id: String,
    to_state_id: String,
    condition_config_json: String,
    transition_type: String,
) -> serde_json::Value {
    let condition_config: serde_json::Value =
        serde_json::from_str(&condition_config_json).unwrap_or_default();
    serde_json::json!({
        "transition_id": transition_id,
        "journey_id": journey_id,
        "from_state_id": from_state_id,
        "to_state_id": to_state_id,
        "condition_config": condition_config,
        "transition_type": transition_type,
    })
}

fn active_journey_json(
    journey_instance_id: String,
    journey_id: String,
    current_state_id: String,
    status: String,
    state_payload: String,
    updated_at: String,
    journey_name: Option<String>,
    state_name: Option<String>,
    state_description: Option<String>,
) -> serde_json::Value {
    serde_json::json!({
        "journey_instance_id": journey_instance_id,
        "journey_id": journey_id,
        "current_state_id": current_state_id,
        "status": status,
        "state_payload": state_payload,
        "updated_at": updated_at,
        "journey_name": journey_name,
        "state_name": state_name,
        "state_description": state_description,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use silicrew_memory::migration::run_migrations;
    use silicrew_types::control::SessionBinding;
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};

    fn test_store() -> ControlStore {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        ControlStore::new(Arc::new(Mutex::new(conn)))
    }

    #[test]
    fn scope_round_trips() {
        let store = test_store();
        let now = Utc::now();
        let scope = ControlScope {
            scope_id: ScopeId::from("tenant-acme"),
            name: "ACME".to_string(),
            scope_type: "tenant".to_string(),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        };

        store.upsert_scope(&scope).unwrap();
        let loaded = store.get_scope(&scope.scope_id).unwrap().unwrap();

        assert_eq!(loaded.scope_id, scope.scope_id);
        assert_eq!(loaded.name, "ACME");
        assert_eq!(store.list_scopes().unwrap().len(), 1);
    }

    #[test]
    fn turn_trace_round_trips_and_lists_by_session() {
        let store = test_store();
        let trace = TurnTraceRecord {
            trace_id: TraceId::new(),
            scope_id: ScopeId::from("default"),
            session_id: SessionId::new(),
            agent_id: AgentId::new(),
            channel_type: "web".to_string(),
            request_message_ref: Some("msg-1".to_string()),
            compiled_context_hash: Some("hash-1".to_string()),
            release_version: Some("v1".to_string()),
            response_mode: ResponseMode::Guided,
            created_at: Utc::now(),
        };

        store.upsert_turn_trace(&trace).unwrap();

        let loaded = store.get_turn_trace(trace.trace_id).unwrap().unwrap();
        let listed = store
            .list_turn_traces_by_session(trace.session_id, 10)
            .unwrap();

        assert_eq!(loaded.trace_id, trace.trace_id);
        assert_eq!(loaded.response_mode, ResponseMode::Guided);
        assert_eq!(loaded.release_version.as_deref(), Some("v1"));
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].compiled_context_hash.as_deref(), Some("hash-1"));
    }

    #[test]
    fn session_binding_upsert_updates_scope_by_session() {
        let store = test_store();
        let session_id = SessionId::new().to_string();
        let first = SessionBinding {
            binding_id: "binding-1".to_string(),
            scope_id: ScopeId::from("scope-a"),
            channel_type: "web".to_string(),
            external_user_id: None,
            external_chat_id: None,
            agent_id: AgentId::new().to_string(),
            session_id: session_id.clone(),
            manual_mode: false,
            active_journey_instance_id: None,
        };
        let second = SessionBinding {
            binding_id: "binding-2".to_string(),
            scope_id: ScopeId::from("scope-b"),
            channel_type: "telegram".to_string(),
            external_user_id: Some("user-1".to_string()),
            external_chat_id: Some("chat-1".to_string()),
            agent_id: AgentId::new().to_string(),
            session_id: session_id.clone(),
            manual_mode: true,
            active_journey_instance_id: Some("journey-1".to_string()),
        };

        store.upsert_session_binding(&first).unwrap();
        store.upsert_session_binding(&second).unwrap();

        let loaded = store.get_session_binding(&session_id).unwrap().unwrap();
        let conn = store.db.sqlite().unwrap();
        let conn = conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM session_bindings WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(count, 1);
        assert_eq!(loaded.binding_id, "binding-1");
        assert_eq!(loaded.scope_id, ScopeId::from("scope-b"));
        assert_eq!(loaded.channel_type, "telegram");
        assert_eq!(loaded.external_user_id.as_deref(), Some("user-1"));
        assert_eq!(loaded.external_chat_id.as_deref(), Some("chat-1"));
        assert!(loaded.manual_mode);
        assert_eq!(
            loaded.active_journey_instance_id.as_deref(),
            Some("journey-1")
        );
    }

    #[test]
    fn context_variable_values_round_trip_and_delete() {
        let store = test_store();
        let variable_id = "var-1";

        store
            .upsert_context_variable(
                variable_id,
                "scope-a",
                "customer_tier",
                "session_value",
                "{}",
                None,
                true,
            )
            .unwrap();
        store
            .upsert_context_variable_value("value-1", variable_id, "session-1", r#"{"tier":"vip"}"#)
            .unwrap();

        let loaded = store
            .get_context_variable_value(variable_id, "session-1")
            .unwrap()
            .unwrap();
        let listed = store.list_context_variable_values(variable_id).unwrap();

        assert_eq!(
            loaded.get("variable_id").and_then(|v| v.as_str()),
            Some(variable_id)
        );
        assert_eq!(
            loaded.get("key").and_then(|v| v.as_str()),
            Some("session-1")
        );
        assert_eq!(
            loaded
                .get("data")
                .and_then(|v| v.get("tier"))
                .and_then(|v| v.as_str()),
            Some("vip")
        );
        assert_eq!(listed.len(), 1);

        assert!(store
            .delete_context_variable_value(variable_id, "session-1")
            .unwrap());
        assert!(store
            .get_context_variable_value(variable_id, "session-1")
            .unwrap()
            .is_none());
    }

    #[test]
    fn deleting_context_variable_cascades_value_rows() {
        let store = test_store();
        let variable_id = "var-delete";

        store
            .upsert_context_variable(
                variable_id,
                "scope-a",
                "customer_status",
                "session_value",
                "{}",
                None,
                true,
            )
            .unwrap();
        store
            .upsert_context_variable_value("value-delete", variable_id, "session-1", "\"gold\"")
            .unwrap();

        assert!(store.delete_context_variable(variable_id).unwrap());
        assert!(store.get_context_variable(variable_id).unwrap().is_none());
        assert!(store
            .get_context_variable_value(variable_id, "session-1")
            .unwrap()
            .is_none());
    }

    #[test]
    fn canned_response_round_trip_and_delete() {
        let store = test_store();
        let response_id = "resp-1";

        store
            .upsert_canned_response(
                response_id,
                "scope-a",
                "handoff_message",
                "A human will take over shortly.",
                Some("contains:handoff"),
                10,
                true,
            )
            .unwrap();

        let loaded = store.get_canned_response(response_id).unwrap().unwrap();
        assert_eq!(
            loaded.get("template_text").and_then(|v| v.as_str()),
            Some("A human will take over shortly.")
        );

        assert!(store.delete_canned_response(response_id).unwrap());
        assert!(store.get_canned_response(response_id).unwrap().is_none());
    }

    #[test]
    fn glossary_term_round_trip_and_delete() {
        let store = test_store();
        store
            .upsert_glossary_term(
                "term-1",
                "scope-a",
                "SLA",
                "Response guarantee",
                "[\"service level agreement\"]",
                true,
                true,
            )
            .unwrap();

        let loaded = store.get_glossary_term("term-1").unwrap().unwrap();
        assert_eq!(loaded.get("name").and_then(|v| v.as_str()), Some("SLA"));

        assert!(store.delete_glossary_term("term-1").unwrap());
        assert!(store.get_glossary_term("term-1").unwrap().is_none());
    }

    #[test]
    fn journey_state_round_trip_update_and_delete() {
        let store = test_store();
        {
            let conn = store.db.sqlite().unwrap();
            let conn = conn.lock().unwrap();
            conn.execute(
                "INSERT INTO journeys (journey_id, scope_id, name, trigger_config, completion_rule, entry_state_id, enabled)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    "journey-state",
                    "scope-a",
                    "state_flow",
                    "{\"always\":true}",
                    Option::<String>::None,
                    "state-a",
                    1i64,
                ],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO journey_states (state_id, journey_id, name, description, required_fields, guideline_actions_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params!["state-a", "journey-state", "Start", Option::<String>::None, "[]", "[]"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO journey_states (state_id, journey_id, name, description, required_fields, guideline_actions_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params!["state-b", "journey-state", "Next", Option::<String>::None, "[]", "[]"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO journey_transitions (transition_id, journey_id, from_state_id, to_state_id, condition_config, transition_type)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params!["transition-state", "journey-state", "state-a", "state-b", "{}", "auto"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO journey_instances (journey_instance_id, scope_id, session_id, journey_id, current_state_id, status, state_payload, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'active', '{}', datetime('now'))",
                params!["instance-state", "scope-a", "session-state", "journey-state", "state-a"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO session_bindings (binding_id, scope_id, channel_type, external_user_id, external_chat_id, agent_id, session_id, manual_mode, active_journey_instance_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?5, 0, ?6, datetime('now'), datetime('now'))",
                params!["binding-state", "scope-a", "web", uuid::Uuid::new_v4().to_string(), "session-state", "instance-state"],
            )
            .unwrap();
        }

        assert!(store
            .update_journey_state(
                "state-a",
                "Start Updated",
                Some("Updated"),
                "[\"email\"]",
                "[\"Act\"]",
            )
            .unwrap());
        let loaded = store.get_journey_state("state-a").unwrap().unwrap();
        assert_eq!(
            loaded.get("name").and_then(|v| v.as_str()),
            Some("Start Updated")
        );

        assert!(store.delete_journey_state("state-a").unwrap());
        assert!(store.get_journey_state("state-a").unwrap().is_none());
    }

    #[test]
    fn journey_transition_round_trip_update_and_delete() {
        let store = test_store();
        {
            let conn = store.db.sqlite().unwrap();
            let conn = conn.lock().unwrap();
            conn.execute(
                "INSERT INTO journey_transitions (transition_id, journey_id, from_state_id, to_state_id, condition_config, transition_type)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params!["transition-a", "journey-a", "state-a", "state-b", "{}", "auto"],
            )
            .unwrap();
        }

        assert!(store
            .update_journey_transition(
                "transition-a",
                "state-a",
                "state-c",
                "{\"always\":true}",
                "manual",
            )
            .unwrap());
        let loaded = store
            .get_journey_transition("transition-a")
            .unwrap()
            .unwrap();
        assert_eq!(
            loaded.get("to_state_id").and_then(|v| v.as_str()),
            Some("state-c")
        );
        assert_eq!(
            loaded.get("transition_type").and_then(|v| v.as_str()),
            Some("manual")
        );

        assert!(store.delete_journey_transition("transition-a").unwrap());
        assert!(store
            .get_journey_transition("transition-a")
            .unwrap()
            .is_none());
    }

    #[test]
    fn retriever_round_trip_and_delete() {
        let store = test_store();
        let retriever = serde_json::json!({
            "retriever_id": "retriever-1",
            "scope_id": "scope-a",
            "name": "FAQ",
            "retriever_type": "static",
            "config_json": { "items": [{ "title": "Refund", "content": "Policy" }] },
            "enabled": true,
        });

        store.upsert_retriever(&retriever).unwrap();
        store
            .insert_retriever_binding(&ScopeId::from("scope-a"), "retriever-1", "always", "always")
            .unwrap();

        let loaded = store.get_retriever("retriever-1").unwrap().unwrap();
        assert_eq!(loaded.get("name").and_then(|v| v.as_str()), Some("FAQ"));

        assert!(store.delete_retriever("retriever-1").unwrap());
        assert!(store.get_retriever("retriever-1").unwrap().is_none());
        assert!(store
            .list_retriever_bindings(&ScopeId::from("scope-a"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn deleting_guideline_relationship_removes_record() {
        let store = test_store();
        store
            .create_guideline_relationship(
                "rel-1",
                "scope-a",
                "guideline-a",
                "guideline-b",
                "overrides",
            )
            .unwrap();

        assert!(store.delete_guideline_relationship("rel-1").unwrap());
        assert!(store
            .list_guideline_relationships("scope-a")
            .unwrap()
            .is_empty());
    }
}
