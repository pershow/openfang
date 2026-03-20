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
    pub allowed_tools: Vec<String>,
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
            allowed_tools: Vec::new(),
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
}

/// Tool call outcome captured after the runtime loop finishes.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub approved: Option<bool>,
    pub success: bool,
}

/// High-level runtime outcome returned to the control plane after a turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnOutcome {
    pub trace_id: TraceId,
    pub response_text: String,
    pub tool_calls: Vec<ToolCallRecord>,
    pub handoff_suggested: bool,
}

impl Default for TurnOutcome {
    fn default() -> Self {
        Self {
            trace_id: TraceId::new(),
            response_text: String::new(),
            tool_calls: Vec::new(),
            handoff_suggested: false,
        }
    }
}

/// Coordinator interface for pre-turn compilation and post-turn recording.
#[async_trait]
pub trait TurnControlCoordinator: Send + Sync {
    /// Compile all control-plane inputs needed before calling the runtime loop.
    async fn compile_turn(&self, input: TurnInput) -> Result<CompiledTurnContext>;

    /// Persist control-plane state after the runtime loop completes.
    async fn after_response(&self, outcome: &TurnOutcome) -> Result<()>;
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
}
