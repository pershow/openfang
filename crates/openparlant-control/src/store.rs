use chrono::{DateTime, Utc};
use openparlant_types::agent::{AgentId, SessionId};
use openparlant_types::control::{
    ControlScope, JourneyTransitionRecord, PolicyMatchRecord, ResponseMode, ScopeId,
    ToolAuthorizationRecord, TraceId, TurnTraceRecord,
};
use openparlant_types::error::{OpenFangError, OpenFangResult};
use rusqlite::{params, Connection};
use serde_json;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

/// SQLite-backed store for control scopes and turn traces.
#[derive(Clone)]
pub struct ControlStore {
    conn: Arc<Mutex<Connection>>,
}

impl ControlStore {
    /// Create a new control store wrapping the shared SQLite connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert or update a control scope.
    pub fn upsert_scope(&self, scope: &ControlScope) -> OpenFangResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
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
        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(())
    }

    /// Fetch a control scope by ID.
    pub fn get_scope(&self, scope_id: &ScopeId) -> OpenFangResult<Option<ControlScope>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT scope_id, name, scope_type, status, created_at, updated_at
                 FROM control_scopes WHERE scope_id = ?1",
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;

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
            Err(e) => Err(OpenFangError::Memory(e.to_string())),
        }
    }

    /// List all control scopes.
    pub fn list_scopes(&self) -> OpenFangResult<Vec<ControlScope>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT scope_id, name, scope_type, status, created_at, updated_at
                 FROM control_scopes ORDER BY name ASC, scope_id ASC",
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
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
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;

        let mut scopes = Vec::new();
        for row in rows {
            scopes.push(scope_from_row(
                row.map_err(|e| OpenFangError::Memory(e.to_string()))?,
            )?);
        }
        Ok(scopes)
    }

    /// Insert or update a turn trace record.
    pub fn upsert_turn_trace(&self, trace: &TurnTraceRecord) -> OpenFangResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
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
        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(())
    }

    /// Fetch a turn trace by ID.
    pub fn get_turn_trace(&self, trace_id: TraceId) -> OpenFangResult<Option<TurnTraceRecord>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT trace_id, scope_id, session_id, agent_id, channel_type,
                        request_message_ref, compiled_context_hash, release_version, response_mode, created_at
                 FROM turn_traces WHERE trace_id = ?1",
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;

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
            Err(e) => Err(OpenFangError::Memory(e.to_string())),
        }
    }

    /// List recent traces for a session, newest first.
    pub fn list_turn_traces_by_session(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> OpenFangResult<Vec<TurnTraceRecord>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT trace_id, scope_id, session_id, agent_id, channel_type,
                        request_message_ref, compiled_context_hash, release_version, response_mode, created_at
                 FROM turn_traces
                 WHERE session_id = ?1
                 ORDER BY created_at DESC
                 LIMIT ?2",
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
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
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;

        let mut traces = Vec::new();
        for row in rows {
            traces.push(trace_from_row(
                row.map_err(|e| OpenFangError::Memory(e.to_string()))?,
            )?);
        }
        Ok(traces)
    }

    // ─── Explainability sub-records ───────────────────────────────────────────

    /// Persist the policy match record for a turn.
    pub fn upsert_policy_match_record(&self, rec: &PolicyMatchRecord) -> OpenFangResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
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
        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(())
    }

    /// Fetch the policy match record for a trace.
    pub fn get_policy_match_record(
        &self,
        trace_id: TraceId,
    ) -> OpenFangResult<Option<PolicyMatchRecord>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT record_id, trace_id, observation_hits_json,
                        guideline_hits_json, guideline_exclusions_json
                 FROM policy_match_records WHERE trace_id = ?1",
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;

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
            Ok(r) => Ok(Some(PolicyMatchRecord {
                record_id: parse_uuid(&r.0).map(TraceId).map_err(memory_parse_error)?,
                trace_id: parse_uuid(&r.1).map(TraceId).map_err(memory_parse_error)?,
                observation_hits_json: r.2,
                guideline_hits_json: r.3,
                guideline_exclusions_json: r.4,
            })),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(OpenFangError::Memory(e.to_string())),
        }
    }

    /// Persist the journey transition record for a turn.
    pub fn upsert_journey_transition_record(
        &self,
        rec: &JourneyTransitionRecord,
    ) -> OpenFangResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
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
        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(())
    }

    /// Persist the tool authorization record for a turn.
    pub fn upsert_tool_authorization_record(
        &self,
        rec: &ToolAuthorizationRecord,
    ) -> OpenFangResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
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
        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(())
    }

    // ── Session Bindings ──────────────────────────────────────────────────────

    /// Upsert a session binding (creates or updates).
    pub fn upsert_session_binding(
        &self,
        binding: &openparlant_types::control::SessionBinding,
    ) -> OpenFangResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO session_bindings
                (binding_id, scope_id, channel_type, external_user_id, external_chat_id,
                 agent_id, session_id, manual_mode, active_journey_instance_id, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))
             ON CONFLICT(binding_id) DO UPDATE SET
                manual_mode = excluded.manual_mode,
                active_journey_instance_id = excluded.active_journey_instance_id,
                updated_at = datetime('now')",
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
            ],
        )
        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(())
    }

    /// Look up the session binding for a session_id (first match).
    pub fn get_session_binding(
        &self,
        session_id: &str,
    ) -> OpenFangResult<Option<openparlant_types::control::SessionBinding>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let row = conn.query_row(
            "SELECT binding_id, scope_id, channel_type, external_user_id, external_chat_id,
                    agent_id, session_id, manual_mode, active_journey_instance_id
             FROM session_bindings WHERE session_id = ?1 LIMIT 1",
            params![session_id],
            |row| {
                Ok(openparlant_types::control::SessionBinding {
                    binding_id: row.get(0)?,
                    scope_id: openparlant_types::control::ScopeId::new(row.get::<_, String>(1)?),
                    channel_type: row.get(2)?,
                    external_user_id: row.get(3)?,
                    external_chat_id: row.get(4)?,
                    agent_id: row.get(5)?,
                    session_id: row.get(6)?,
                    manual_mode: row.get::<_, i64>(7)? != 0,
                    active_journey_instance_id: row.get(8)?,
                })
            },
        );
        match row {
            Ok(b) => Ok(Some(b)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(OpenFangError::Memory(e.to_string())),
        }
    }

    /// Enable or disable manual mode for a session binding.
    pub fn set_manual_mode(&self, session_id: &str, manual: bool) -> OpenFangResult<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let rows = conn
            .execute(
                "UPDATE session_bindings SET manual_mode = ?1, updated_at = datetime('now')
                 WHERE session_id = ?2",
                params![manual as i64, session_id],
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(rows > 0)
    }

    // ── Retrievers ────────────────────────────────────────────────────────────

    /// Insert or update a retriever definition.
    pub fn upsert_retriever(&self, r: &serde_json::Value) -> OpenFangResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        conn.execute(
            "INSERT INTO retrievers (retriever_id, scope_id, name, retriever_type, config_json, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(retriever_id) DO UPDATE SET
                name = excluded.name,
                retriever_type = excluded.retriever_type,
                config_json = excluded.config_json,
                enabled = excluded.enabled",
            params![
                r["retriever_id"].as_str().unwrap_or(""),
                r["scope_id"].as_str().unwrap_or(""),
                r["name"].as_str().unwrap_or(""),
                r["retriever_type"].as_str().unwrap_or("static"),
                r["config_json"].to_string(),
                r["enabled"].as_bool().unwrap_or(true) as i64,
            ],
        ).map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(())
    }

    /// List retrievers for a scope.
    pub fn list_retrievers(&self, scope_id: &ScopeId) -> OpenFangResult<Vec<serde_json::Value>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT retriever_id, scope_id, name, retriever_type, config_json, enabled
             FROM retrievers WHERE scope_id = ?1 ORDER BY name ASC",
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        let rows = stmt
            .query_map(params![scope_id.0.as_str()], |row| {
                Ok(serde_json::json!({
                    "retriever_id": row.get::<_, String>(0)?,
                    "scope_id":     row.get::<_, String>(1)?,
                    "name":         row.get::<_, String>(2)?,
                    "retriever_type": row.get::<_, String>(3)?,
                    "config_json":  row.get::<_, String>(4)?,
                    "enabled":      row.get::<_, i64>(5)? != 0,
                }))
            })
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── Control Releases ──────────────────────────────────────────────────────

    /// Publish a new release snapshot for a scope.
    pub fn publish_release(
        &self,
        release_id: &str,
        scope_id: &ScopeId,
        version: &str,
        published_by: &str,
    ) -> OpenFangResult<()> {
        // Mark any current "published" release as "superseded" first.
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        conn.execute(
            "UPDATE control_releases SET status = 'superseded'
             WHERE scope_id = ?1 AND status = 'published'",
            params![scope_id.0.as_str()],
        )
        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        conn.execute(
            "INSERT INTO control_releases (release_id, scope_id, version, status, published_by, created_at)
             VALUES (?1, ?2, ?3, 'published', ?4, datetime('now'))",
            params![release_id, scope_id.0.as_str(), version, published_by],
        ).map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(())
    }

    /// Rollback: mark the latest published release as rolled_back and re-activate the previous one.
    pub fn rollback_release(&self, scope_id: &ScopeId) -> OpenFangResult<Option<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        // Find the currently published release.
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
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
            // Promote the most recent "superseded" release back to "published".
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
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        }
        Ok(current)
    }

    /// List releases for a scope (newest first).
    pub fn list_releases(&self, scope_id: &ScopeId) -> OpenFangResult<Vec<serde_json::Value>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT release_id, scope_id, version, status, published_by, created_at
             FROM control_releases WHERE scope_id = ?1 ORDER BY created_at DESC",
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        let rows = stmt
            .query_map(params![scope_id.0.as_str()], |row| {
                Ok(serde_json::json!({
                    "release_id":   row.get::<_, String>(0)?,
                    "scope_id":     row.get::<_, String>(1)?,
                    "version":      row.get::<_, String>(2)?,
                    "status":       row.get::<_, String>(3)?,
                    "published_by": row.get::<_, String>(4)?,
                    "created_at":   row.get::<_, String>(5)?,
                }))
            })
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Return the currently published release version for a scope, if any.
    pub fn current_release_version(&self, scope_id: &ScopeId) -> OpenFangResult<Option<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
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
            Err(e) => Err(OpenFangError::Memory(e.to_string())),
        }
    }

    // ── Handoff Records ───────────────────────────────────────────────────────

    /// Insert a new handoff record.
    pub fn create_handoff(
        &self,
        handoff: &openparlant_types::control::HandoffRecord,
    ) -> OpenFangResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
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
        .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(())
    }

    /// Update the status of a handoff record.
    pub fn update_handoff_status(
        &self,
        handoff_id: &str,
        status: &openparlant_types::control::HandoffStatus,
    ) -> OpenFangResult<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let rows = conn
            .execute(
                "UPDATE handoff_records SET status = ?1, updated_at = datetime('now')
                 WHERE handoff_id = ?2",
                params![status.as_str(), handoff_id],
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        Ok(rows > 0)
    }

    /// List handoff records for a session (newest first).
    pub fn list_handoffs_by_session(
        &self,
        session_id: &str,
        limit: usize,
    ) -> OpenFangResult<Vec<serde_json::Value>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| OpenFangError::Internal(e.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT handoff_id, scope_id, session_id, reason, summary, status, created_at, updated_at
                 FROM handoff_records WHERE session_id = ?1
                 ORDER BY created_at DESC LIMIT ?2",
            )
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        let rows = stmt
            .query_map(params![session_id, limit as i64], |row| {
                Ok(serde_json::json!({
                    "handoff_id": row.get::<_, String>(0)?,
                    "scope_id": row.get::<_, String>(1)?,
                    "session_id": row.get::<_, String>(2)?,
                    "reason": row.get::<_, String>(3)?,
                    "summary": row.get::<_, Option<String>>(4)?,
                    "status": row.get::<_, String>(5)?,
                    "created_at": row.get::<_, String>(6)?,
                    "updated_at": row.get::<_, String>(7)?,
                }))
            })
            .map_err(|e| OpenFangError::Memory(e.to_string()))?;
        let items: Vec<_> = rows.filter_map(|r| r.ok()).collect();
        Ok(items)
    }
}

fn scope_from_row(
    row: (String, String, String, String, String, String),
) -> OpenFangResult<ControlScope> {
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
) -> OpenFangResult<TurnTraceRecord> {
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
        response_mode: ResponseMode::from_str(&row.8).map_err(OpenFangError::Memory)?,
        created_at: parse_timestamp(&row.9)?,
    })
}

fn parse_timestamp(value: &str) -> OpenFangResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(memory_parse_error)
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
}
