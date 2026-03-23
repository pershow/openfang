//! Control-plane types shared across the conversational control layer.

use crate::agent::{AgentId, SessionId};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub Uuid);

        impl $name {
            /// Generate a new random ID.
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(Uuid::parse_str(s)?))
            }
        }
    };
}

/// Stable namespace for control-plane records.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScopeId(pub String);

impl ScopeId {
    /// Create a new scope identifier.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Return the single-tenant default scope.
    pub fn default_scope() -> Self {
        Self("default".to_string())
    }
}

impl Default for ScopeId {
    fn default() -> Self {
        Self::default_scope()
    }
}

impl std::fmt::Display for ScopeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for ScopeId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ScopeId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

uuid_id!(ObservationId);
uuid_id!(GuidelineId);
uuid_id!(JourneyId);
uuid_id!(TraceId);

/// Persisted control scope definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControlScope {
    pub scope_id: ScopeId,
    pub name: String,
    pub scope_type: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Persisted observation matcher definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObservationDefinition {
    pub observation_id: ObservationId,
    pub scope_id: ScopeId,
    pub name: String,
    pub matcher_type: String,
    pub matcher_config: serde_json::Value,
    pub priority: i32,
    pub enabled: bool,
}

/// Persisted guideline definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuidelineDefinition {
    pub guideline_id: GuidelineId,
    pub scope_id: ScopeId,
    pub name: String,
    pub condition_ref: String,
    pub action_text: String,
    pub composition_mode: String,
    pub priority: i32,
    pub enabled: bool,
}

/// Persisted journey definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JourneyDefinition {
    pub journey_id: JourneyId,
    pub scope_id: ScopeId,
    pub name: String,
    pub trigger_config: serde_json::Value,
    pub completion_rule: Option<String>,
    #[serde(default)]
    pub entry_state_id: Option<String>,
    pub enabled: bool,
}

/// Persisted trace record for a compiled turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnTraceRecord {
    pub trace_id: TraceId,
    pub scope_id: ScopeId,
    pub session_id: SessionId,
    pub agent_id: AgentId,
    pub channel_type: String,
    pub request_message_ref: Option<String>,
    pub compiled_context_hash: Option<String>,
    pub release_version: Option<String>,
    pub response_mode: ResponseMode,
    pub created_at: DateTime<Utc>,
}

/// Canonicalized inbound message used by the control plane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalMessage {
    /// Inbound channel type (web, feishu, slack, etc.).
    pub channel_type: String,
    /// Logical control-plane scope.
    pub scope_id: ScopeId,
    /// External user identifier if known.
    pub external_user_id: Option<String>,
    /// External chat/conversation identifier if known.
    pub external_chat_id: Option<String>,
    /// External message identifier if known.
    pub external_message_id: Option<String>,
    /// Sender type (user, system, operator, bot, etc.).
    pub sender_type: Option<String>,
    /// Primary text body.
    pub text: String,
    /// Opaque attachment payloads.
    pub attachments: Vec<serde_json::Value>,
    /// Mentioned handles or IDs.
    pub mentions: Vec<String>,
    /// Full raw payload from the channel.
    pub raw_payload: Option<serde_json::Value>,
    /// Receipt timestamp.
    pub received_at: DateTime<Utc>,
}

impl CanonicalMessage {
    /// Construct a simple text message.
    pub fn text(
        scope_id: ScopeId,
        channel_type: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            channel_type: channel_type.into(),
            scope_id,
            external_user_id: None,
            external_chat_id: None,
            external_message_id: None,
            sender_type: Some("user".to_string()),
            text: text.into(),
            attachments: Vec::new(),
            mentions: Vec::new(),
            raw_payload: None,
            received_at: Utc::now(),
        }
    }
}

impl Default for CanonicalMessage {
    fn default() -> Self {
        Self::text(ScopeId::default(), "web", "")
    }
}

/// Observation matched for the current turn.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ObservationHit {
    pub observation_id: ObservationId,
    pub name: String,
    pub confidence: Option<f32>,
    pub matched_by: String,
}

/// Guideline activated for the current turn.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuidelineActivation {
    pub guideline_id: GuidelineId,
    pub name: String,
    pub action_text: String,
    #[serde(default)]
    pub composition_mode: Option<String>,
    pub priority: i32,
    pub source_observations: Vec<ObservationId>,
}

/// Guideline excluded during conflict resolution.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExcludedGuideline {
    pub guideline_id: GuidelineId,
    pub name: String,
    pub reason: String,
}

/// Active journey resolution for the current turn.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JourneyActivation {
    pub journey_id: JourneyId,
    pub name: String,
    pub current_state: String,
    pub missing_fields: Vec<String>,
    pub allowed_next_actions: Vec<String>,
}

/// Retrieved knowledge chunk.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RetrievedChunk {
    pub source: String,
    pub content: String,
    pub score: Option<f32>,
    pub metadata: Option<serde_json::Value>,
}

/// Glossary entry selected for the turn.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlossaryEntry {
    pub name: String,
    pub description: String,
    pub synonyms: Vec<String>,
}

/// Resolved variable injected into the turn context.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResolvedVariable {
    pub name: String,
    pub value: String,
    pub source: String,
}

/// Approved canned response candidate.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CannedResponseCandidate {
    pub name: String,
    pub template_text: String,
    pub priority: i32,
}

/// Why a tool is available for the current turn.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolAuthorization {
    pub tool_name: String,
    pub reasons: Vec<String>,
    pub requires_approval: bool,
}

/// Response composition mode for the current turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResponseMode {
    /// Free-form model response with guarded tools.
    #[default]
    Freeform,
    /// Model response guided by active policy and journey context.
    Guided,
    /// Strict response constrained by policy/journey rules.
    Strict,
    /// Response must be selected/composed from canned responses only.
    CannedOnly,
}

impl ResponseMode {
    /// Stable snake_case storage representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Freeform => "freeform",
            Self::Guided => "guided",
            Self::Strict => "strict",
            Self::CannedOnly => "canned_only",
        }
    }
}

impl FromStr for ResponseMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "freeform" => Ok(Self::Freeform),
            "guided" => Ok(Self::Guided),
            "strict" => Ok(Self::Strict),
            "canned_only" => Ok(Self::CannedOnly),
            other => Err(format!("unknown response mode: {other}")),
        }
    }
}

/// Trace metadata emitted at compile time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditMeta {
    pub trace_id: TraceId,
    pub scope_id: ScopeId,
    pub compiled_at: DateTime<Utc>,
    pub labels: Vec<String>,
    pub release_version: Option<String>,
}

impl Default for AuditMeta {
    fn default() -> Self {
        Self {
            trace_id: TraceId::new(),
            scope_id: ScopeId::default(),
            compiled_at: Utc::now(),
            labels: Vec::new(),
            release_version: None,
        }
    }
}

/// Full control-plane output for a turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledTurnContext {
    pub agent_id: AgentId,
    pub session_id: SessionId,
    pub canonical_message: CanonicalMessage,
    pub active_observations: Vec<ObservationHit>,
    pub active_guidelines: Vec<GuidelineActivation>,
    pub excluded_guidelines: Vec<ExcludedGuideline>,
    pub active_journey: Option<JourneyActivation>,
    pub retrieved_chunks: Vec<RetrievedChunk>,
    pub glossary_terms: Vec<GlossaryEntry>,
    pub context_variables: Vec<ResolvedVariable>,
    pub canned_response_candidates: Vec<CannedResponseCandidate>,
    /// When true, this scope is using control-plane-managed tool authorization.
    ///
    /// In this mode, an empty `allowed_tools` means "deny all tools this turn".
    /// When false, legacy runtime visibility rules remain in effect.
    #[serde(default)]
    pub tool_control_active: bool,
    /// Tools that are allowed this turn (after ToolGate evaluation).
    pub allowed_tools: Vec<String>,
    /// Subset of `allowed_tools` that require human approval before execution.
    #[serde(default)]
    pub approval_required_tools: Vec<String>,
    pub tool_authorizations: Vec<ToolAuthorization>,
    pub response_mode: ResponseMode,
    pub audit_meta: AuditMeta,
}

impl Default for CompiledTurnContext {
    fn default() -> Self {
        Self {
            agent_id: AgentId::new(),
            session_id: SessionId::new(),
            canonical_message: CanonicalMessage::default(),
            active_observations: Vec::new(),
            active_guidelines: Vec::new(),
            excluded_guidelines: Vec::new(),
            active_journey: None,
            retrieved_chunks: Vec::new(),
            glossary_terms: Vec::new(),
            context_variables: Vec::new(),
            canned_response_candidates: Vec::new(),
            tool_control_active: false,
            allowed_tools: Vec::new(),
            approval_required_tools: Vec::new(),
            tool_authorizations: Vec::new(),
            response_mode: ResponseMode::default(),
            audit_meta: AuditMeta::default(),
        }
    }
}

/// Inputs required to compile a turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnInput {
    pub scope_id: ScopeId,
    pub agent_id: AgentId,
    pub session_id: SessionId,
    pub message: CanonicalMessage,
    /// Runtime-visible candidate tools for this turn.
    #[serde(default)]
    pub candidate_tools: Vec<ToolCandidate>,
    /// Tool calls produced in the previous preparation iteration (empty on the first call).
    ///
    /// When non-empty, the coordinator will use these results to enrich the message context
    /// and re-evaluate guidelines — implementing Parlant's "preparation iteration" loop.
    #[serde(default)]
    pub prior_tool_calls: Vec<ToolCallRecord>,
}

/// Agent/session context passed into knowledge compilation (glossary selection, `agent_kv` variables).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCompileContext {
    pub agent_id: AgentId,
    pub session_id: SessionId,
}

/// Tool call outcome captured after the runtime loop finishes.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub approved: Option<bool>,
    pub success: bool,
    /// Serialized result or error message from the tool call (used in preparation iteration).
    #[serde(default)]
    pub result: Option<String>,
}

/// Runtime-visible tool candidate for control-plane gating.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ToolCandidate {
    pub tool_name: String,
    /// Skill group this tool belongs to, if it is provided by a skill.
    #[serde(default)]
    pub skill_ref: Option<String>,
}

/// Session-level flags resolved from `session_bindings` (control plane).
#[derive(Debug, Clone, Default)]
pub struct SessionBindingFlags {
    /// Scope from an existing binding, if any.
    pub scope_id: Option<ScopeId>,
    /// Channel type resolved from an existing binding, if any.
    pub channel_type: Option<String>,
    /// External user identifier resolved from an existing binding, if any.
    pub external_user_id: Option<String>,
    /// External chat/thread identifier resolved from an existing binding, if any.
    pub external_chat_id: Option<String>,
    /// When true, the AI must not respond (human operator mode).
    pub manual_mode: bool,
}

/// Serialized fields from `compile_turn` so `after_response` can persist explainability rows.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ControlExplainabilitySnapshot {
    pub observation_hits_json: String,
    pub guideline_hits_json: String,
    pub guideline_exclusions_json: String,
    pub allowed_tools_json: String,
    /// Object shape: `{ "tool_reasons": { "<tool>": ["…"] } }` (may be extended at persist time).
    pub authorization_reasons_json: String,
    /// Typically `{ "tools": ["…"] }` listing tools that require approval this turn.
    pub approval_requirements_json: String,
}

impl ControlExplainabilitySnapshot {
    /// Build from a compiled turn (pre–agent-loop).
    pub fn from_compiled(ctx: &CompiledTurnContext) -> Self {
        let tool_reasons: serde_json::Map<String, serde_json::Value> = ctx
            .tool_authorizations
            .iter()
            .map(|a| {
                (
                    a.tool_name.clone(),
                    serde_json::to_value(&a.reasons).unwrap_or_else(|_| serde_json::json!([])),
                )
            })
            .collect();
        let authorization_reasons_json = serde_json::json!({
            "tool_reasons": tool_reasons,
            "tool_control_active": ctx.tool_control_active,
            "tool_control_mode": if ctx.tool_control_active {
                "managed_deny_by_default"
            } else {
                "legacy_open_by_default"
            },
        })
        .to_string();
        Self {
            observation_hits_json: serde_json::to_string(&ctx.active_observations)
                .unwrap_or_else(|_| "[]".into()),
            guideline_hits_json: serde_json::to_string(&ctx.active_guidelines)
                .unwrap_or_else(|_| "[]".into()),
            guideline_exclusions_json: serde_json::to_string(&ctx.excluded_guidelines)
                .unwrap_or_else(|_| "[]".into()),
            allowed_tools_json: serde_json::to_string(&ctx.allowed_tools)
                .unwrap_or_else(|_| "[]".into()),
            authorization_reasons_json,
            approval_requirements_json: serde_json::json!({ "tools": ctx.approval_required_tools })
                .to_string(),
        }
    }
}

/// High-level runtime outcome returned to the control plane after a turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnOutcome {
    pub trace_id: TraceId,
    pub scope_id: ScopeId,
    /// Session this turn belongs to (journey instances, bindings, etc.).
    pub session_id: crate::agent::SessionId,
    pub response_text: String,
    pub tool_calls: Vec<ToolCallRecord>,
    pub handoff_suggested: bool,
    /// When `compile_turn` succeeded, carries JSON snapshots for policy / tool explainability rows.
    #[serde(default)]
    pub explainability: Option<ControlExplainabilitySnapshot>,
}

impl Default for TurnOutcome {
    fn default() -> Self {
        Self {
            trace_id: TraceId::new(),
            scope_id: ScopeId::default(),
            session_id: crate::agent::SessionId::default(),
            response_text: String::new(),
            tool_calls: Vec::new(),
            handoff_suggested: false,
            explainability: None,
        }
    }
}

/// Coordinator interface for pre-turn compilation and post-turn recording.
#[async_trait]
pub trait TurnControlCoordinator: Send + Sync {
    /// Compile all control-plane inputs needed before calling the runtime loop.
    async fn compile_turn(&self, input: TurnInput) -> Result<CompiledTurnContext>;

    /// Re-compile the turn after tool results are available.
    ///
    /// Implementations that do not support iterative preparation can reuse
    /// `compile_turn`, which preserves backward compatibility for callers.
    async fn compile_turn_iterative(&self, input: TurnInput) -> Result<CompiledTurnContext> {
        self.compile_turn(input).await
    }

    /// Persist control-plane state after the runtime loop completes.
    async fn after_response(&self, outcome: &TurnOutcome) -> Result<()>;

    /// Resolve optional scope and manual-mode from `session_bindings` when a store is wired.
    fn session_binding_flags(&self, _session_id: SessionId) -> SessionBindingFlags {
        SessionBindingFlags::default()
    }
}

// ─── Explainability / trace sub-records ──────────────────────────────────────

/// Persisted per-turn policy match record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyMatchRecord {
    pub record_id: TraceId,
    pub trace_id: TraceId,
    pub observation_hits_json: String,
    pub guideline_hits_json: String,
    pub guideline_exclusions_json: String,
}

/// Persisted per-turn journey transition record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JourneyTransitionRecord {
    pub record_id: TraceId,
    pub trace_id: TraceId,
    pub journey_instance_id: String,
    pub before_state_id: Option<String>,
    pub after_state_id: Option<String>,
    pub decision_json: String,
}

/// Persisted per-turn tool authorization record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAuthorizationRecord {
    pub record_id: TraceId,
    pub trace_id: TraceId,
    pub allowed_tools_json: String,
    pub authorization_reasons_json: String,
    pub approval_requirements_json: String,
}

// ─── Phase 3: Tool Gate / Approval / Handoff ──────────────────────────────────

/// Approval mode for a tool-exposure policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    /// Tool is always allowed without any approval.
    None,
    /// Tool requires explicit human approval before execution.
    Required,
    /// Tool requires approval only when the journey is in a sensitive state.
    Conditional,
}

impl ApprovalMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalMode::None => "none",
            ApprovalMode::Required => "required",
            ApprovalMode::Conditional => "conditional",
        }
    }
}

impl std::fmt::Display for ApprovalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for ApprovalMode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "none" => Ok(ApprovalMode::None),
            "required" => Ok(ApprovalMode::Required),
            "conditional" => Ok(ApprovalMode::Conditional),
            other => Err(anyhow::anyhow!("Unknown approval_mode: {}", other)),
        }
    }
}

impl Default for ApprovalMode {
    fn default() -> Self {
        ApprovalMode::None
    }
}

/// A tool-exposure policy entry.
///
/// Defines when a particular tool is visible to the model, and whether it
/// requires human approval before execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExposurePolicy {
    pub policy_id: String,
    pub scope_id: ScopeId,
    /// The exact tool name to match (e.g. "shell", "file_write").
    pub tool_name: String,
    /// Optional skill reference that groups this tool.
    pub skill_ref: Option<String>,
    /// The policy is only active when this observation is matched.
    pub observation_ref: Option<String>,
    /// The policy is only active when the journey is in this state.
    pub journey_state_ref: Option<String>,
    /// The policy is only active when this guideline is active.
    pub guideline_ref: Option<String>,
    pub approval_mode: ApprovalMode,
    pub enabled: bool,
}

/// Session-level binding of a scope+channel to a running session.
///
/// Holds `manual_mode` and the active journey instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBinding {
    pub binding_id: String,
    pub scope_id: ScopeId,
    pub channel_type: String,
    pub external_user_id: Option<String>,
    pub external_chat_id: Option<String>,
    pub agent_id: String,
    pub session_id: String,
    /// When `true` the AI response is suppressed; a human operator handles this session.
    pub manual_mode: bool,
    pub active_journey_instance_id: Option<String>,
}

/// Status of a handoff record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffStatus {
    Pending,
    Accepted,
    Resolved,
    Cancelled,
}

impl HandoffStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            HandoffStatus::Pending => "pending",
            HandoffStatus::Accepted => "accepted",
            HandoffStatus::Resolved => "resolved",
            HandoffStatus::Cancelled => "cancelled",
        }
    }
}

impl Default for HandoffStatus {
    fn default() -> Self {
        HandoffStatus::Pending
    }
}

/// A human-handoff record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffRecord {
    pub handoff_id: String,
    pub scope_id: ScopeId,
    pub session_id: String,
    pub reason: String,
    pub summary: Option<String>,
    pub status: HandoffStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Outcome from the ToolGate for a single tool name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolGateDecision {
    pub tool_name: String,
    /// True if the tool should be visible to the model this turn.
    pub allowed: bool,
    /// Reason for the decision (for explainability).
    pub reason: String,
    /// If true, executing the tool also requires human approval.
    pub requires_approval: bool,
}

// ─── LLM caller abstraction for control-plane semantic matching ───────────────

/// Minimal LLM call abstraction for control-plane semantic matching.
///
/// Deliberately thin — the concrete implementation in `silicrew-api` bridges
/// this to the existing `LlmDriver` inside the kernel.
#[async_trait]
pub trait ControlLlmCaller: Send + Sync {
    /// Call the LLM with a single user prompt (the system role is implied) and
    /// return the raw text response.
    async fn call(&self, prompt: &str) -> anyhow::Result<String>;
}

/// Minimal embedding abstraction for control-plane vector retrieval.
///
/// Returns a unit-normalised `f32` embedding vector for the given text.
#[async_trait]
pub trait ControlEmbedder: Send + Sync {
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_default_is_stable() {
        assert_eq!(ScopeId::default().0, "default");
    }

    #[test]
    fn canonical_message_text_constructor_sets_defaults() {
        let msg = CanonicalMessage::text(ScopeId::default(), "web", "hello");
        assert_eq!(msg.channel_type, "web");
        assert_eq!(msg.text, "hello");
        assert!(msg.attachments.is_empty());
        assert_eq!(msg.sender_type.as_deref(), Some("user"));
    }

    #[test]
    fn response_mode_round_trips_to_storage_string() {
        assert_eq!(ResponseMode::Guided.as_str(), "guided");
        assert_eq!(
            ResponseMode::from_str("canned_only").unwrap(),
            ResponseMode::CannedOnly
        );
        assert!(ResponseMode::from_str("invalid").is_err());
    }

    #[test]
    fn explainability_snapshot_from_compiled_includes_policy_and_tools() {
        use crate::agent::{AgentId, SessionId};

        let mut ctx = CompiledTurnContext::default();
        ctx.agent_id = AgentId::new();
        ctx.session_id = SessionId::new();
        ctx.active_observations.push(ObservationHit {
            observation_id: ObservationId::new(),
            name: "billing".to_string(),
            confidence: Some(1.0),
            matched_by: "test".to_string(),
        });
        ctx.active_guidelines.push(GuidelineActivation {
            guideline_id: GuidelineId::new(),
            name: "g1".to_string(),
            action_text: "be polite".to_string(),
            composition_mode: Some("strict".to_string()),
            priority: 1,
            source_observations: vec![],
        });
        ctx.excluded_guidelines.push(ExcludedGuideline {
            guideline_id: GuidelineId::new(),
            name: "g2".to_string(),
            reason: "lower priority".to_string(),
        });
        ctx.tool_control_active = true;
        ctx.allowed_tools = vec!["search".to_string()];
        ctx.tool_authorizations.push(ToolAuthorization {
            tool_name: "search".to_string(),
            reasons: vec!["guideline g1".to_string()],
            requires_approval: false,
        });
        ctx.approval_required_tools.push("delete_all".to_string());

        let snap = ControlExplainabilitySnapshot::from_compiled(&ctx);
        assert!(snap.observation_hits_json.contains("billing"));
        assert!(snap.guideline_hits_json.contains("g1"));
        assert!(snap.guideline_exclusions_json.contains("g2"));
        assert!(snap.allowed_tools_json.contains("search"));
        let auth: serde_json::Value =
            serde_json::from_str(&snap.authorization_reasons_json).unwrap();
        assert!(auth.get("tool_reasons").is_some());
        assert_eq!(
            auth.get("tool_control_mode").and_then(|mode| mode.as_str()),
            Some("managed_deny_by_default")
        );
        let appr: serde_json::Value =
            serde_json::from_str(&snap.approval_requirements_json).unwrap();
        assert_eq!(
            appr.get("tools")
                .and_then(|t| t.as_array())
                .map(|a| a.len()),
            Some(1)
        );
    }
}
