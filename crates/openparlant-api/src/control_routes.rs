//! Control-plane REST handlers — /api/control/*
//!
//! Phase 1 目标：把 observations / guidelines / journeys / glossary /
//! context_variables / canned_responses 的 CRUD 暴露出来，
//! 同时提供 POST /api/control/test/compile-turn 调试端点。

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use openparlant_types::control::{
    CanonicalMessage, ControlScope, GuidelineDefinition, GuidelineId, JourneyDefinition, JourneyId,
    ObservationDefinition, ObservationId, ScopeId, TurnInput,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::routes::AppState;

// ─── Helper ───────────────────────────────────────────────────────────────────

fn internal(e: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": e.to_string()})),
    )
}

fn not_found(what: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": format!("{what} not found")})),
    )
}

/// Join `turn_traces` rows with policy / tool / journey explainability tables.
pub fn enrich_turn_traces_json(
    conn: &rusqlite::Connection,
    traces: Vec<openparlant_types::control::TurnTraceRecord>,
) -> Vec<serde_json::Value> {
    use openparlant_types::control::TurnTraceRecord;

    traces
        .into_iter()
        .map(|trace: TurnTraceRecord| {
            let tid = trace.trace_id.0.to_string();

            let policy_row = conn
                .query_row(
                    "SELECT observation_hits_json, guideline_hits_json, guideline_exclusions_json
                     FROM policy_match_records WHERE trace_id = ?1 LIMIT 1",
                    rusqlite::params![&tid],
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
                    rusqlite::params![&tid],
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
                    rusqlite::params![&tid],
                    |row| {
                        Ok((
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, String>(2).unwrap_or_else(|_| "{}".into()),
                        ))
                    },
                )
                .ok();

            let parse_arr = |s: &str| -> serde_json::Value {
                serde_json::from_str(s).unwrap_or(serde_json::json!([]))
            };
            let parse_obj = |s: &str| -> serde_json::Value {
                serde_json::from_str(s).unwrap_or(serde_json::json!({}))
            };

            serde_json::json!({
                "trace_id": trace.trace_id,
                "scope_id": trace.scope_id,
                "session_id": trace.session_id,
                "agent_id": trace.agent_id,
                "channel_type": trace.channel_type,
                "release_version": trace.release_version,
                "response_mode": trace.response_mode,
                "created_at": trace.created_at,
                "observation_hits": policy_row.as_ref().map(|r| parse_arr(&r.0)).unwrap_or(serde_json::json!([])),
                "guideline_hits": policy_row.as_ref().map(|r| parse_arr(&r.1)).unwrap_or(serde_json::json!([])),
                "guideline_exclusions": policy_row.as_ref().map(|r| parse_arr(&r.2)).unwrap_or(serde_json::json!([])),
                "allowed_tools": tool_row.as_ref().map(|r| parse_arr(&r.0)).unwrap_or(serde_json::json!([])),
                "authorization_reasons": tool_row.as_ref().map(|r| parse_obj(&r.1)).unwrap_or(serde_json::json!({})),
                "approval_required_tools": tool_row.as_ref().map(|r| parse_obj(&r.2)).unwrap_or(serde_json::json!({})),
                "journey_before_state": journey_row.as_ref().and_then(|r| r.0.clone()),
                "journey_after_state": journey_row.as_ref().and_then(|r| r.1.clone()),
                "journey_decision": journey_row.as_ref().map(|r| parse_obj(&r.2)).unwrap_or(serde_json::json!({})),
            })
        })
        .collect()
}

// ─── Control Scopes ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateScopeRequest {
    pub name: String,
    #[serde(default = "default_scope_type")]
    pub scope_type: String,
}
fn default_scope_type() -> String {
    "agent".to_string()
}

/// POST /api/control/scopes
pub async fn create_scope(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateScopeRequest>,
) -> impl IntoResponse {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "scope name is required"})),
        );
    }
    let now = Utc::now();
    let scope_id = ScopeId::new(uuid::Uuid::new_v4().to_string());
    let scope = ControlScope {
        scope_id: scope_id.clone(),
        name,
        scope_type: req.scope_type,
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
    };
    match state.control_store.upsert_scope(&scope) {
        Ok(()) => (StatusCode::CREATED, Json(serde_json::json!(scope))),
        Err(e) => internal(e),
    }
}

/// GET /api/control/scopes/:scope_id
pub async fn get_scope(
    State(state): State<Arc<AppState>>,
    Path(scope_id): Path<String>,
) -> impl IntoResponse {
    let sid = ScopeId::new(scope_id);
    match state.control_store.get_scope(&sid) {
        Ok(Some(s)) => (StatusCode::OK, Json(serde_json::json!(s))),
        Ok(None) => not_found("scope"),
        Err(e) => internal(e),
    }
}

/// GET /api/control/scopes
pub async fn list_scopes(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.control_store.list_scopes() {
        Ok(scopes) => (StatusCode::OK, Json(serde_json::json!(scopes))),
        Err(e) => internal(e),
    }
}

// ─── Observations ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateObservationRequest {
    pub scope_id: String,
    pub name: String,
    #[serde(default = "default_matcher_type")]
    pub matcher_type: String,
    #[serde(default)]
    pub matcher_config: serde_json::Value,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
}
fn default_matcher_type() -> String {
    "keyword".to_string()
}
fn default_true() -> bool {
    true
}

/// POST /api/control/observations
pub async fn create_observation(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateObservationRequest>,
) -> impl IntoResponse {
    let obs = ObservationDefinition {
        observation_id: ObservationId(uuid::Uuid::new_v4()),
        scope_id: ScopeId::new(req.scope_id),
        name: req.name,
        matcher_type: req.matcher_type,
        matcher_config: req.matcher_config,
        priority: req.priority,
        enabled: req.enabled,
    };
    match state.policy_store.upsert_observation(&obs) {
        Ok(()) => (StatusCode::CREATED, Json(serde_json::json!(obs))),
        Err(e) => internal(e),
    }
}

/// GET /api/control/scopes/:scope_id/observations
pub async fn list_observations(
    State(state): State<Arc<AppState>>,
    Path(scope_id): Path<String>,
) -> impl IntoResponse {
    let sid = ScopeId::new(scope_id);
    match state.policy_store.list_observations(&sid, false) {
        Ok(obs) => (StatusCode::OK, Json(serde_json::json!(obs))),
        Err(e) => internal(e),
    }
}

/// GET /api/control/observations/:observation_id
pub async fn get_observation(
    State(state): State<Arc<AppState>>,
    Path(observation_id): Path<String>,
) -> impl IntoResponse {
    let oid = match uuid::Uuid::parse_str(&observation_id) {
        Ok(u) => ObservationId(u),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid observation ID"})),
            )
        }
    };
    match state.policy_store.get_observation(oid) {
        Ok(Some(o)) => (StatusCode::OK, Json(serde_json::json!(o))),
        Ok(None) => not_found("observation"),
        Err(e) => internal(e),
    }
}

// ─── Guidelines ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateGuidelineRequest {
    pub scope_id: String,
    pub name: String,
    #[serde(default)]
    pub condition_ref: String,
    pub action_text: String,
    #[serde(default = "default_composition_mode")]
    pub composition_mode: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
}
fn default_composition_mode() -> String {
    "append".to_string()
}

/// POST /api/control/guidelines
pub async fn create_guideline(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateGuidelineRequest>,
) -> impl IntoResponse {
    let g = GuidelineDefinition {
        guideline_id: GuidelineId(uuid::Uuid::new_v4()),
        scope_id: ScopeId::new(req.scope_id),
        name: req.name,
        condition_ref: req.condition_ref,
        action_text: req.action_text,
        composition_mode: req.composition_mode,
        priority: req.priority,
        enabled: req.enabled,
    };
    match state.policy_store.upsert_guideline(&g) {
        Ok(()) => (StatusCode::CREATED, Json(serde_json::json!(g))),
        Err(e) => internal(e),
    }
}

/// GET /api/control/scopes/:scope_id/guidelines
pub async fn list_guidelines(
    State(state): State<Arc<AppState>>,
    Path(scope_id): Path<String>,
) -> impl IntoResponse {
    let sid = ScopeId::new(scope_id);
    match state.policy_store.list_guidelines(&sid, false) {
        Ok(gs) => (StatusCode::OK, Json(serde_json::json!(gs))),
        Err(e) => internal(e),
    }
}

/// GET /api/control/guidelines/:guideline_id
pub async fn get_guideline(
    State(state): State<Arc<AppState>>,
    Path(guideline_id): Path<String>,
) -> impl IntoResponse {
    let gid = match uuid::Uuid::parse_str(&guideline_id) {
        Ok(u) => GuidelineId(u),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid guideline ID"})),
            )
        }
    };
    match state.policy_store.get_guideline(gid) {
        Ok(Some(g)) => (StatusCode::OK, Json(serde_json::json!(g))),
        Ok(None) => not_found("guideline"),
        Err(e) => internal(e),
    }
}

// ─── Journeys ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateJourneyRequest {
    pub scope_id: String,
    pub name: String,
    #[serde(default)]
    pub trigger_config: serde_json::Value,
    pub completion_rule: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// POST /api/control/journeys
pub async fn create_journey(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateJourneyRequest>,
) -> impl IntoResponse {
    let j = JourneyDefinition {
        journey_id: JourneyId(uuid::Uuid::new_v4()),
        scope_id: ScopeId::new(req.scope_id),
        name: req.name,
        trigger_config: req.trigger_config,
        completion_rule: req.completion_rule,
        entry_state_id: None,
        enabled: req.enabled,
    };
    match state.journey_store.upsert_journey(&j) {
        Ok(()) => (StatusCode::CREATED, Json(serde_json::json!(j))),
        Err(e) => internal(e),
    }
}

/// GET /api/control/scopes/:scope_id/journeys
pub async fn list_journeys(
    State(state): State<Arc<AppState>>,
    Path(scope_id): Path<String>,
) -> impl IntoResponse {
    let sid = ScopeId::new(scope_id);
    match state.journey_store.list_journeys_sync(&sid, false) {
        Ok(js) => (StatusCode::OK, Json(serde_json::json!(js))),
        Err(e) => internal(e),
    }
}

/// GET /api/control/journeys/:journey_id
pub async fn get_journey(
    State(state): State<Arc<AppState>>,
    Path(journey_id): Path<String>,
) -> impl IntoResponse {
    let jid = match uuid::Uuid::parse_str(&journey_id) {
        Ok(u) => JourneyId(u),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid journey ID"})),
            )
        }
    };
    match state.journey_store.get_journey_sync(&jid) {
        Ok(Some(j)) => (StatusCode::OK, Json(serde_json::json!(j))),
        Ok(None) => not_found("journey"),
        Err(e) => internal(e),
    }
}

// ─── Glossary terms ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct GlossaryTermRequest {
    pub scope_id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub synonyms: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// When true, the term is always injected into the compiled glossary for this scope (pinned).
    #[serde(default)]
    pub always_include: bool,
}

/// POST /api/control/glossary-terms
pub async fn create_glossary_term(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GlossaryTermRequest>,
) -> impl IntoResponse {
    let synonyms_json = serde_json::to_string(&req.synonyms).unwrap_or_else(|_| "[]".to_string());
    let term_id = uuid::Uuid::new_v4().to_string();
    let scope_id_clone = req.scope_id.clone();
    let name_clone = req.name.clone();
    match state.control_store.upsert_glossary_term(
        &term_id,
        &req.scope_id,
        &req.name,
        &req.description,
        &synonyms_json,
        req.enabled,
        req.always_include,
    ) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(
                serde_json::json!({"term_id": term_id, "scope_id": scope_id_clone, "name": name_clone}),
            ),
        ),
        Err(e) => internal(e),
    }
}

// ─── Context variables ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ContextVariableRequest {
    pub scope_id: String,
    pub name: String,
    #[serde(default = "default_static")]
    pub value_source_type: String,
    /// For static: `{"value": "..."}`.  For other types: provider-specific config.
    #[serde(default)]
    pub value_source_config: serde_json::Value,
    pub visibility_rule: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}
fn default_static() -> String {
    "static".to_string()
}

/// POST /api/control/context-variables
pub async fn create_context_variable(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ContextVariableRequest>,
) -> impl IntoResponse {
    let config_json =
        serde_json::to_string(&req.value_source_config).unwrap_or_else(|_| "{}".to_string());
    let var_id = uuid::Uuid::new_v4().to_string();
    let scope_id_clone = req.scope_id.clone();
    let name_clone = req.name.clone();
    match state.control_store.upsert_context_variable(
        &var_id,
        &req.scope_id,
        &req.name,
        &req.value_source_type,
        &config_json,
        req.visibility_rule.as_deref(),
        req.enabled,
    ) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(
                serde_json::json!({"variable_id": var_id, "scope_id": scope_id_clone, "name": name_clone}),
            ),
        ),
        Err(e) => internal(e),
    }
}

// ─── Canned responses ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct CannedResponseRequest {
    pub scope_id: String,
    pub name: String,
    pub template_text: String,
    pub trigger_rule: Option<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// POST /api/control/canned-responses
pub async fn create_canned_response(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CannedResponseRequest>,
) -> impl IntoResponse {
    let resp_id = uuid::Uuid::new_v4().to_string();
    let scope_id_clone = req.scope_id.clone();
    let name_clone = req.name.clone();
    match state.control_store.upsert_canned_response(
        &resp_id,
        &req.scope_id,
        &req.name,
        &req.template_text,
        req.trigger_rule.as_deref(),
        req.priority,
        req.enabled,
    ) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(
                serde_json::json!({"response_id": resp_id, "scope_id": scope_id_clone, "name": name_clone}),
            ),
        ),
        Err(e) => internal(e),
    }
}

// ─── Test: compile-turn ───────────────────────────────────────────────────────

/// GET /api/control/scopes/:scope_id/glossary-terms
pub async fn list_glossary_terms(
    State(state): State<Arc<AppState>>,
    Path(scope_id): Path<String>,
) -> impl IntoResponse {
    match state.control_store.list_glossary_terms(&scope_id) {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))),
        Err(e) => internal(e),
    }
}

/// GET /api/control/scopes/:scope_id/context-variables
pub async fn list_context_variables(
    State(state): State<Arc<AppState>>,
    Path(scope_id): Path<String>,
) -> impl IntoResponse {
    match state.control_store.list_context_variables(&scope_id) {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))),
        Err(e) => internal(e),
    }
}

/// GET /api/control/scopes/:scope_id/canned-responses
pub async fn list_canned_responses(
    State(state): State<Arc<AppState>>,
    Path(scope_id): Path<String>,
) -> impl IntoResponse {
    match state.control_store.list_canned_responses(&scope_id) {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))),
        Err(e) => internal(e),
    }
}

#[derive(Debug, Deserialize)]
pub struct CompileTurnRequest {
    pub scope_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub message: String,
    #[serde(default = "default_channel")]
    pub channel_type: String,
}
fn default_channel() -> String {
    "web".to_string()
}

/// POST /api/control/test/compile-turn
///
/// Dry-run the control-plane compilation and return `CompiledTurnContext`
/// without calling the agent loop.  Useful for debugging policy / journey /
/// knowledge injection.
pub async fn test_compile_turn(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CompileTurnRequest>,
) -> impl IntoResponse {
    let scope_id = ScopeId::new(req.scope_id.clone());
    let agent_id: openparlant_types::agent::AgentId = match req.agent_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            )
        }
    };
    let session_id = match uuid::Uuid::parse_str(&req.session_id) {
        Ok(u) => openparlant_types::agent::SessionId(u),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid session ID"})),
            )
        }
    };

    let message = CanonicalMessage::text(scope_id.clone(), req.channel_type.clone(), req.message);
    let input = TurnInput {
        scope_id,
        agent_id,
        session_id,
        message,
        candidate_tools: state.kernel.control_candidate_tools(agent_id),
        prior_tool_calls: Vec::new(),
    };

    match state.control_coordinator.compile_turn(input).await {
        Ok(ctx) => (StatusCode::OK, Json(serde_json::json!(ctx))),
        Err(e) => internal(e),
    }
}

// ─── Trace / explainability ───────────────────────────────────────────────────

/// GET /api/sessions/:session_id/control-trace
///
/// Returns turn traces for this session, each enriched with the
/// policy-match record (observations / guideline hits / exclusions)
/// and the tool-authorization record (allowed tools / approval requirements).
pub async fn session_control_trace(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let sid = match uuid::Uuid::parse_str(&session_id) {
        Ok(u) => openparlant_types::agent::SessionId(u),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid session ID"})),
            )
        }
    };

    // Load raw traces from the control store.
    let traces = match state.control_store.list_turn_traces_by_session(sid, 50) {
        Ok(t) => t,
        Err(e) => return internal(e),
    };

    match state.control_store.enrich_turn_traces_json(traces) {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))),
        Err(e) => internal(e),
    }
}

// ─── Guideline Relationships ──────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct GuidelineRelationshipRequest {
    pub scope_id: String,
    pub from_guideline_id: String,
    pub to_guideline_id: String,
    /// Accepted values:
    /// - "overrides" | "prioritizes_over"
    /// - "conflicts_with" | "excludes"
    /// - "requires" | "depends_on"
    /// - "complements"
    pub relation_type: String,
}

/// POST /api/control/guideline-relationships
pub async fn create_guideline_relationship(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GuidelineRelationshipRequest>,
) -> impl IntoResponse {
    let Some(canonical_relation_type) =
        openparlant_policy::canonical_guideline_relation_type(&req.relation_type)
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Invalid relation_type. Use overrides/prioritizes_over, conflicts_with/excludes, requires/depends_on, or complements."
            })),
        );
    };
    let rel_id = uuid::Uuid::new_v4().to_string();
    match state.control_store.create_guideline_relationship(
        &rel_id,
        &req.scope_id,
        &req.from_guideline_id,
        &req.to_guideline_id,
        canonical_relation_type,
    ) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "relationship_id": rel_id,
                "scope_id": req.scope_id,
                "from_guideline_id": req.from_guideline_id,
                "to_guideline_id": req.to_guideline_id,
                "relation_type": canonical_relation_type,
            })),
        ),
        Err(e) => internal(e),
    }
}

/// GET /api/control/scopes/:scope_id/guideline-relationships
pub async fn list_guideline_relationships(
    State(state): State<Arc<AppState>>,
    Path(scope_id): Path<String>,
) -> impl IntoResponse {
    match state.control_store.list_guideline_relationships(&scope_id) {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))),
        Err(e) => internal(e),
    }
}

// ─── Journey States ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct JourneyStateRequest {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub required_fields: Vec<String>,
    /// Optional guideline action texts projected when this state is active.
    /// Each string becomes a [`GuidelineActivation`] this turn.
    #[serde(default)]
    pub guideline_actions: Vec<String>,
}

/// POST /api/control/journeys/:journey_id/states
pub async fn create_journey_state(
    State(state): State<Arc<AppState>>,
    Path(journey_id): Path<String>,
    Json(req): Json<JourneyStateRequest>,
) -> impl IntoResponse {
    let state_id = uuid::Uuid::new_v4().to_string();
    let req_fields_json =
        serde_json::to_string(&req.required_fields).unwrap_or_else(|_| "[]".into());
    let guideline_actions_json =
        serde_json::to_string(&req.guideline_actions).unwrap_or_else(|_| "[]".into());
    let name_clone = req.name.clone();
    if let Err(e) = state.control_store.upsert_journey_state(
        &state_id,
        &journey_id,
        &req.name,
        req.description.as_deref(),
        &req_fields_json,
        &guideline_actions_json,
    ) {
        return internal(e);
    }

    if let Ok(journey_uuid) = uuid::Uuid::parse_str(&journey_id) {
        let journey_id_obj = JourneyId(journey_uuid);
        if let Ok(Some(journey)) = state.journey_store.get_journey_sync(&journey_id_obj) {
            if journey.entry_state_id.is_none() {
                if let Err(e) = state
                    .journey_store
                    .set_entry_state(&journey_id_obj, Some(&state_id))
                {
                    return internal(e);
                }
            }
        }
    }

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "state_id": state_id,
            "journey_id": journey_id,
            "name": name_clone,
        })),
    )
}

/// GET /api/control/journeys/:journey_id/states
pub async fn list_journey_states(
    State(state): State<Arc<AppState>>,
    Path(journey_id): Path<String>,
) -> impl IntoResponse {
    match state.control_store.list_journey_states(&journey_id) {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))),
        Err(e) => internal(e),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JourneyEntryStateRequest {
    pub state_id: String,
}

/// POST /api/control/journeys/:journey_id/entry-state
pub async fn set_journey_entry_state(
    State(state): State<Arc<AppState>>,
    Path(journey_id): Path<String>,
    Json(req): Json<JourneyEntryStateRequest>,
) -> impl IntoResponse {
    let journey_id = match uuid::Uuid::parse_str(&journey_id) {
        Ok(id) => JourneyId(id),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid journey ID"})),
            );
        }
    };

    match state
        .journey_store
        .set_entry_state(&journey_id, Some(&req.state_id))
    {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "journey_id": journey_id,
                "entry_state_id": req.state_id,
            })),
        ),
        Err(e) => {
            let message = e.to_string();
            if message.contains("entry state") && message.contains("journey") {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": message })),
                )
            } else {
                internal(message)
            }
        }
    }
}

// ─── Journey Transitions ──────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct JourneyTransitionRequest {
    pub from_state_id: String,
    pub to_state_id: String,
    #[serde(default)]
    pub condition_config: serde_json::Value,
    /// "auto" | "observation" | "manual"
    #[serde(default = "default_transition_type")]
    pub transition_type: String,
}
fn default_transition_type() -> String {
    "auto".to_string()
}

/// POST /api/control/journeys/:journey_id/transitions
pub async fn create_journey_transition(
    State(state): State<Arc<AppState>>,
    Path(journey_id): Path<String>,
    Json(req): Json<JourneyTransitionRequest>,
) -> impl IntoResponse {
    let trans_id = uuid::Uuid::new_v4().to_string();
    let cond_json = serde_json::to_string(&req.condition_config).unwrap_or_else(|_| "{}".into());
    match state.control_store.upsert_journey_transition(
        &trans_id,
        &journey_id,
        &req.from_state_id,
        &req.to_state_id,
        &cond_json,
        &req.transition_type,
    ) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "transition_id": trans_id,
                "journey_id": journey_id,
                "from_state_id": req.from_state_id,
                "to_state_id": req.to_state_id,
                "transition_type": req.transition_type,
            })),
        ),
        Err(e) => internal(e),
    }
}

/// GET /api/control/journeys/:journey_id/transitions
pub async fn list_journey_transitions(
    State(state): State<Arc<AppState>>,
    Path(journey_id): Path<String>,
) -> impl IntoResponse {
    match state.control_store.list_journey_transitions(&journey_id) {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))),
        Err(e) => internal(e),
    }
}

// ─── Session Journey State ────────────────────────────────────────────────────

/// GET /api/sessions/:session_id/journey-state
///
/// Returns the active journey instance (if any) for this session,
/// including the current state details.
pub async fn session_journey_state(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state
        .control_store
        .get_active_journey_for_session(&session_id)
    {
        Ok(Some(data)) => (
            StatusCode::OK,
            Json(serde_json::json!({"active": true, "journey": data})),
        ),
        Ok(None) => (
            StatusCode::OK,
            Json(serde_json::json!({"active": false, "journey": null})),
        ),
        Err(e) => internal(e),
    }
}

// ─── Tool Exposure Policies ───────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateToolPolicyRequest {
    pub scope_id: String,
    pub tool_name: String,
    pub skill_ref: Option<String>,
    pub observation_ref: Option<String>,
    pub journey_state_ref: Option<String>,
    pub guideline_ref: Option<String>,
    /// "none" | "required" | "conditional"
    #[serde(default = "default_approval_mode")]
    pub approval_mode: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}
fn default_approval_mode() -> String {
    "none".to_string()
}

/// POST /api/control/tool-policies
pub async fn create_tool_policy(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateToolPolicyRequest>,
) -> impl IntoResponse {
    use openparlant_types::control::{ApprovalMode, ScopeId, ToolExposurePolicy};
    let policy_id = uuid::Uuid::new_v4().to_string();
    let approval_mode: ApprovalMode = req.approval_mode.parse().unwrap_or_default();
    let policy = ToolExposurePolicy {
        policy_id: policy_id.clone(),
        scope_id: ScopeId::new(req.scope_id.clone()),
        tool_name: req.tool_name.clone(),
        skill_ref: req.skill_ref.clone(),
        observation_ref: req.observation_ref.clone(),
        journey_state_ref: req.journey_state_ref.clone(),
        guideline_ref: req.guideline_ref.clone(),
        approval_mode,
        enabled: req.enabled,
    };
    match state.policy_store.upsert_tool_exposure_policy(&policy) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "policy_id": policy_id,
                "scope_id": req.scope_id,
                "tool_name": req.tool_name,
                "skill_ref": req.skill_ref,
                "observation_ref": req.observation_ref,
                "journey_state_ref": req.journey_state_ref,
                "guideline_ref": req.guideline_ref,
                "approval_mode": req.approval_mode,
                "enabled": req.enabled,
            })),
        ),
        Err(e) => internal(e),
    }
}

/// GET /api/control/scopes/:scope_id/tool-policies
pub async fn list_tool_policies(
    State(state): State<Arc<AppState>>,
    Path(scope_id): Path<String>,
) -> impl IntoResponse {
    use openparlant_types::control::ScopeId;
    match state
        .policy_store
        .list_tool_exposure_policies(&ScopeId::new(scope_id))
    {
        Ok(policies) => {
            let items: Vec<_> = policies
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "policy_id": p.policy_id,
                        "scope_id": p.scope_id.0,
                        "tool_name": p.tool_name,
                        "skill_ref": p.skill_ref,
                        "observation_ref": p.observation_ref,
                        "journey_state_ref": p.journey_state_ref,
                        "guideline_ref": p.guideline_ref,
                        "approval_mode": p.approval_mode.as_str(),
                        "enabled": p.enabled,
                    })
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!(items)))
        }
        Err(e) => internal(e),
    }
}

/// GET /api/control/tool-policies/:policy_id
pub async fn get_tool_policy(
    State(state): State<Arc<AppState>>,
    Path(policy_id): Path<String>,
) -> impl IntoResponse {
    match state.policy_store.get_tool_exposure_policy(&policy_id) {
        Ok(Some(p)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "policy_id": p.policy_id,
                "scope_id": p.scope_id.0,
                "tool_name": p.tool_name,
                "skill_ref": p.skill_ref,
                "observation_ref": p.observation_ref,
                "journey_state_ref": p.journey_state_ref,
                "guideline_ref": p.guideline_ref,
                "approval_mode": p.approval_mode.as_str(),
                "enabled": p.enabled,
            })),
        ),
        Ok(None) => not_found("policy"),
        Err(e) => internal(e),
    }
}

// ─── Session Binding + Manual Mode / Handoff ─────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateSessionBindingRequest {
    pub scope_id: String,
    pub channel_type: String,
    pub agent_id: String,
    pub session_id: String,
    pub external_user_id: Option<String>,
    pub external_chat_id: Option<String>,
}

/// POST /api/control/session-bindings
pub async fn create_session_binding(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSessionBindingRequest>,
) -> impl IntoResponse {
    use openparlant_types::control::{ScopeId, SessionBinding};
    let binding = SessionBinding {
        binding_id: uuid::Uuid::new_v4().to_string(),
        scope_id: ScopeId::new(req.scope_id.clone()),
        channel_type: req.channel_type.clone(),
        external_user_id: req.external_user_id.clone(),
        external_chat_id: req.external_chat_id.clone(),
        agent_id: req.agent_id.clone(),
        session_id: req.session_id.clone(),
        manual_mode: false,
        active_journey_instance_id: None,
    };
    match state.control_store.upsert_session_binding(&binding) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "binding_id": binding.binding_id,
                "scope_id": req.scope_id,
                "session_id": req.session_id,
                "manual_mode": false,
            })),
        ),
        Err(e) => internal(e),
    }
}

/// GET /api/sessions/:session_id/binding
pub async fn get_session_binding(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.control_store.get_session_binding(&session_id) {
        Ok(Some(b)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "binding_id": b.binding_id,
                "scope_id": b.scope_id.0,
                "channel_type": b.channel_type,
                "agent_id": b.agent_id,
                "session_id": b.session_id,
                "manual_mode": b.manual_mode,
                "active_journey_instance_id": b.active_journey_instance_id,
            })),
        ),
        Ok(None) => not_found("session_binding"),
        Err(e) => internal(e),
    }
}

/// POST /api/sessions/:session_id/manual-mode
///
/// Switches the session into manual (human-operator) mode.
/// The AI will not respond until `resume-ai` is called.
pub async fn enable_manual_mode(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.control_store.set_manual_mode(&session_id, true) {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "session_id": session_id,
                "manual_mode": true,
                "message": "Session switched to manual mode. AI responses suppressed."
            })),
        ),
        Ok(false) => not_found("session_binding"),
        Err(e) => internal(e),
    }
}

/// POST /api/sessions/:session_id/resume-ai
///
/// Resumes AI responses for a session that was in manual mode.
pub async fn resume_ai(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.control_store.set_manual_mode(&session_id, false) {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "session_id": session_id,
                "manual_mode": false,
                "message": "AI responses resumed."
            })),
        ),
        Ok(false) => not_found("session_binding"),
        Err(e) => internal(e),
    }
}

#[derive(Debug, Deserialize)]
pub struct HandoffRequest {
    pub reason: String,
    pub summary: Option<String>,
}

/// POST /api/sessions/:session_id/handoff
///
/// Creates a handoff record and automatically enables manual mode.
pub async fn create_handoff(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Json(req): Json<HandoffRequest>,
) -> impl IntoResponse {
    use chrono::Utc;
    use openparlant_types::control::{HandoffRecord, HandoffStatus, ScopeId};

    let scope_id = state
        .control_store
        .get_session_binding(&session_id)
        .ok()
        .flatten()
        .map(|b| b.scope_id)
        .unwrap_or_else(ScopeId::default_scope);

    let handoff_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now();
    let record = HandoffRecord {
        handoff_id: handoff_id.clone(),
        scope_id,
        session_id: session_id.clone(),
        reason: req.reason.clone(),
        summary: req.summary.clone(),
        status: HandoffStatus::Pending,
        created_at: now,
        updated_at: now,
    };

    if let Err(e) = state.control_store.create_handoff(&record) {
        return internal(e);
    }

    let _ = state.control_store.set_manual_mode(&session_id, true);

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "handoff_id": handoff_id,
            "session_id": session_id,
            "reason": req.reason,
            "summary": req.summary,
            "status": "pending",
            "manual_mode": true,
        })),
    )
}

/// GET /api/sessions/:session_id/handoffs
pub async fn list_handoffs(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state
        .control_store
        .list_handoffs_by_session(&session_id, 20)
    {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))),
        Err(e) => internal(e),
    }
}

#[derive(Debug, Deserialize)]
pub struct HandoffStatusUpdate {
    /// "accepted" | "resolved" | "cancelled"
    pub status: String,
}

/// PATCH /api/control/handoffs/:handoff_id/status
pub async fn update_handoff_status(
    State(state): State<Arc<AppState>>,
    Path(handoff_id): Path<String>,
    Json(req): Json<HandoffStatusUpdate>,
) -> impl IntoResponse {
    use openparlant_types::control::HandoffStatus;
    let status = match req.status.as_str() {
        "accepted" => HandoffStatus::Accepted,
        "resolved" => HandoffStatus::Resolved,
        "cancelled" => HandoffStatus::Cancelled,
        other => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Unknown status: {}", other)})),
            )
        }
    };
    match state
        .control_store
        .update_handoff_status(&handoff_id, &status)
    {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "handoff_id": handoff_id,
                "status": req.status,
            })),
        ),
        Ok(false) => not_found("handoff"),
        Err(e) => internal(e),
    }
}

// ─── Retrievers ───────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct CreateRetrieverRequest {
    pub scope_id: String,
    pub name: String,
    #[serde(default = "default_retriever_type")]
    pub retriever_type: String,
    #[serde(default)]
    pub config_json: serde_json::Value,
    #[serde(default = "default_true")]
    pub enabled: bool,
}
fn default_retriever_type() -> String {
    "static".to_string()
}

/// POST /api/control/retrievers
pub async fn create_retriever(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateRetrieverRequest>,
) -> impl IntoResponse {
    let retriever_id = uuid::Uuid::new_v4().to_string();
    let record = serde_json::json!({
        "retriever_id": retriever_id,
        "scope_id": req.scope_id,
        "name": req.name,
        "retriever_type": req.retriever_type,
        "config_json": req.config_json,
        "enabled": req.enabled,
    });
    match state.control_store.upsert_retriever(&record) {
        Ok(()) => (StatusCode::CREATED, Json(record)),
        Err(e) => internal(e),
    }
}

/// GET /api/control/scopes/:scope_id/retrievers
pub async fn list_retrievers(
    State(state): State<Arc<AppState>>,
    Path(scope_id): Path<String>,
) -> impl IntoResponse {
    use openparlant_types::control::ScopeId;
    match state.control_store.list_retrievers(&ScopeId::new(scope_id)) {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))),
        Err(e) => internal(e),
    }
}

#[derive(serde::Deserialize)]
pub struct CreateRetrieverBindingRequest {
    pub scope_id: String,
    pub retriever_id: String,
    /// One of: `guideline`, `journey_state`, `scope`, `always` (see runtime `run_retrievers`).
    pub bind_type: String,
    /// Guideline **name**, journey state id, or scope id string depending on `bind_type`.
    pub bind_ref: String,
}

/// POST /api/control/retriever-bindings
pub async fn create_retriever_binding(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateRetrieverBindingRequest>,
) -> impl IntoResponse {
    use openparlant_types::control::ScopeId;
    match state.control_store.insert_retriever_binding(
        &ScopeId::new(req.scope_id.clone()),
        &req.retriever_id,
        &req.bind_type,
        &req.bind_ref,
    ) {
        Ok(binding_id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "binding_id": binding_id,
                "scope_id": req.scope_id,
                "retriever_id": req.retriever_id,
                "bind_type": req.bind_type,
                "bind_ref": req.bind_ref,
            })),
        ),
        Err(e) => internal(e),
    }
}

/// GET /api/control/scopes/:scope_id/retriever-bindings
pub async fn list_retriever_bindings(
    State(state): State<Arc<AppState>>,
    Path(scope_id): Path<String>,
) -> impl IntoResponse {
    use openparlant_types::control::ScopeId;
    match state
        .control_store
        .list_retriever_bindings(&ScopeId::new(scope_id))
    {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))),
        Err(e) => internal(e),
    }
}

/// DELETE /api/control/retriever-bindings/:binding_id
pub async fn delete_retriever_binding(
    State(state): State<Arc<AppState>>,
    Path(binding_id): Path<String>,
) -> impl IntoResponse {
    match state.control_store.delete_retriever_binding(&binding_id) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found("retriever binding").into_response(),
        Err(e) => internal(e).into_response(),
    }
}

// ─── Releases ─────────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct PublishReleaseRequest {
    pub scope_id: String,
    pub version: String,
    #[serde(default = "default_system_user")]
    pub published_by: String,
}
fn default_system_user() -> String {
    "system".to_string()
}

/// POST /api/control/releases/publish
pub async fn publish_release(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PublishReleaseRequest>,
) -> impl IntoResponse {
    use openparlant_types::control::ScopeId;
    let release_id = uuid::Uuid::new_v4().to_string();
    match state.control_store.publish_release(
        &release_id,
        &ScopeId::new(req.scope_id.clone()),
        &req.version,
        &req.published_by,
    ) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "release_id": release_id,
                "scope_id": req.scope_id,
                "version": req.version,
                "status": "published",
                "published_by": req.published_by,
            })),
        ),
        Err(e) => internal(e),
    }
}

#[derive(serde::Deserialize)]
pub struct RollbackReleaseRequest {
    pub scope_id: String,
}

/// POST /api/control/releases/rollback
pub async fn rollback_release(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RollbackReleaseRequest>,
) -> impl IntoResponse {
    use openparlant_types::control::ScopeId;
    match state
        .control_store
        .rollback_release(&ScopeId::new(req.scope_id.clone()))
    {
        Ok(Some(rid)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "rolled_back_release_id": rid,
                "scope_id": req.scope_id,
            })),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "No published release found to roll back",
                "scope_id": req.scope_id,
            })),
        ),
        Err(e) => internal(e),
    }
}

/// GET /api/control/scopes/:scope_id/releases
pub async fn list_releases(
    State(state): State<Arc<AppState>>,
    Path(scope_id): Path<String>,
) -> impl IntoResponse {
    use openparlant_types::control::ScopeId;
    match state.control_store.list_releases(&ScopeId::new(scope_id)) {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))),
        Err(e) => internal(e),
    }
}
