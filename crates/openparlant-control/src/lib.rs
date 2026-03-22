//! Orchestrates control-plane compilation around the existing runtime loop.

mod store;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use openparlant_context::{KnowledgeBundle, KnowledgeCompiler, NoopKnowledgeCompiler};
use openparlant_journey::{JourneyRuntime, NoopJourneyRuntime};
use openparlant_policy::{
    NoopObservationMatcher, NoopPolicyResolver, NoopToolGate, ObservationMatcher, PolicyResolver,
    ToolGate,
};
use openparlant_types::agent::SessionId;
use openparlant_types::control::{
    AuditMeta, CompiledTurnContext, PolicyMatchRecord, ResponseMode, SessionBindingFlags,
    ToolAuthorization, ToolAuthorizationRecord, TurnControlCoordinator, TurnInput, TurnOutcome,
};
use sha2::{Digest, Sha256};
pub use store::ControlStore;
use tracing::{debug, warn};

/// Default coordinator wiring together policy, journey, context, and tool-gate compilation.
///
/// `store` is optional – if supplied, every `compile_turn` writes a `turn_trace`
/// and every `after_response` writes the three explainability sub-records.
pub struct DefaultTurnControlCoordinator<OM, PR, JR, KC, TG = NoopToolGate> {
    observation_matcher: OM,
    policy_resolver: PR,
    journey_runtime: JR,
    knowledge_compiler: KC,
    tool_gate: TG,
    store: Option<ControlStore>,
    /// Optional LLM caller for semantic matching (passed through to matchers/resolvers if needed).
    #[allow(dead_code)]
    llm_caller: Option<std::sync::Arc<dyn openparlant_types::control::ControlLlmCaller>>,
}

impl<OM, PR, JR, KC, TG> DefaultTurnControlCoordinator<OM, PR, JR, KC, TG> {
    pub fn new_with_gate(
        observation_matcher: OM,
        policy_resolver: PR,
        journey_runtime: JR,
        knowledge_compiler: KC,
        tool_gate: TG,
    ) -> Self {
        Self {
            observation_matcher,
            policy_resolver,
            journey_runtime,
            knowledge_compiler,
            tool_gate,
            store: None,
            llm_caller: None,
        }
    }

    /// Attach a `ControlStore` so that turn traces are persisted.
    pub fn with_store(mut self, store: ControlStore) -> Self {
        self.store = Some(store);
        self
    }

    /// Attach an LLM caller for semantic matching / embedding.
    pub fn with_llm_caller(
        mut self,
        caller: std::sync::Arc<dyn openparlant_types::control::ControlLlmCaller>,
    ) -> Self {
        self.llm_caller = Some(caller);
        self
    }
}

impl<OM, PR, JR, KC> DefaultTurnControlCoordinator<OM, PR, JR, KC, NoopToolGate> {
    pub fn new(
        observation_matcher: OM,
        policy_resolver: PR,
        journey_runtime: JR,
        knowledge_compiler: KC,
    ) -> Self {
        Self {
            observation_matcher,
            policy_resolver,
            journey_runtime,
            knowledge_compiler,
            tool_gate: NoopToolGate,
            store: None,
            llm_caller: None,
        }
    }
}

impl
    DefaultTurnControlCoordinator<
        NoopObservationMatcher,
        NoopPolicyResolver,
        NoopJourneyRuntime,
        NoopKnowledgeCompiler,
        NoopToolGate,
    >
{
    /// Create a no-op coordinator useful for incremental integration.
    pub fn noop() -> Self {
        Self::new(
            NoopObservationMatcher,
            NoopPolicyResolver,
            NoopJourneyRuntime,
            NoopKnowledgeCompiler,
        )
    }
}

#[async_trait]
impl<OM, PR, JR, KC, TG> TurnControlCoordinator
    for DefaultTurnControlCoordinator<OM, PR, JR, KC, TG>
where
    OM: ObservationMatcher,
    PR: PolicyResolver,
    JR: JourneyRuntime,
    KC: KnowledgeCompiler,
    TG: ToolGate,
{
    async fn compile_turn(&self, input: TurnInput) -> Result<CompiledTurnContext> {
        self.compile_turn_once(input, None, true).await
    }

    async fn compile_turn_iterative(&self, input: TurnInput) -> Result<CompiledTurnContext> {
        self.compile_turn_iterative_impl(input).await
    }

    async fn after_response(&self, outcome: &TurnOutcome) -> Result<()> {
        let journey_update = self
            .journey_runtime
            .apply_outcome(&outcome.scope_id, outcome)
            .await?;

        debug!(
            trace_id = %outcome.trace_id,
            tool_calls = outcome.tool_calls.len(),
            handoff_suggested = outcome.handoff_suggested,
            journey_updated = journey_update.is_some(),
            "recorded turn outcome"
        );

        // Persist explainability sub-records (best-effort)
        if let Some(store) = &self.store {
            use openparlant_types::control::{JourneyTransitionRecord, TraceId};

            // ── Journey transition record ──────────────────────────────────────
            if let Some(ref update) = journey_update {
                let jtr = JourneyTransitionRecord {
                    record_id: TraceId::new(),
                    trace_id: outcome.trace_id,
                    journey_instance_id: update.journey_instance_id.clone().unwrap_or_default(),
                    before_state_id: update.before_state.clone(),
                    after_state_id: update.after_state.clone(),
                    decision_json: serde_json::to_string(&serde_json::json!({
                        "handoff_requested": update.handoff_requested,
                        "completed": update.completed,
                    }))
                    .unwrap_or_default(),
                };
                if let Err(e) = store.upsert_journey_transition_record(&jtr) {
                    warn!(trace_id = %outcome.trace_id, error = %e, "failed to persist journey transition record");
                }
            }

            // ── Policy match record (from compile_turn snapshot) ─────────────────
            let rec_id = openparlant_types::control::TraceId::new();
            let (obs_j, g_hit_j, g_excl_j) = outcome
                .explainability
                .as_ref()
                .map(|e| {
                    (
                        e.observation_hits_json.clone(),
                        e.guideline_hits_json.clone(),
                        e.guideline_exclusions_json.clone(),
                    )
                })
                .unwrap_or_else(|| ("[]".into(), "[]".into(), "[]".into()));
            let pmr = PolicyMatchRecord {
                record_id: rec_id,
                trace_id: outcome.trace_id,
                observation_hits_json: obs_j,
                guideline_hits_json: g_hit_j,
                guideline_exclusions_json: g_excl_j,
            };
            if let Err(e) = store.upsert_policy_match_record(&pmr) {
                warn!(trace_id = %outcome.trace_id, error = %e, "failed to persist policy match record");
            }

            // ── Tool authorization record ──────────────────────────────────────
            let mut auth_json: serde_json::Value = outcome
                .explainability
                .as_ref()
                .and_then(|e| serde_json::from_str(&e.authorization_reasons_json).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            if let Some(obj) = auth_json.as_object_mut() {
                let _ = obj.insert(
                    "runtime_tool_calls".to_string(),
                    serde_json::to_value(&outcome.tool_calls).unwrap_or_default(),
                );
            }
            let (allowed_j, approval_j) = outcome
                .explainability
                .as_ref()
                .map(|e| {
                    (
                        e.allowed_tools_json.clone(),
                        e.approval_requirements_json.clone(),
                    )
                })
                .unwrap_or_else(|| ("[]".into(), "{}".into()));
            let tar = ToolAuthorizationRecord {
                record_id: openparlant_types::control::TraceId::new(),
                trace_id: outcome.trace_id,
                allowed_tools_json: allowed_j,
                authorization_reasons_json: auth_json.to_string(),
                approval_requirements_json: approval_j,
            };
            if let Err(e) = store.upsert_tool_authorization_record(&tar) {
                warn!(trace_id = %outcome.trace_id, error = %e, "failed to persist tool auth record");
            }
        }

        Ok(())
    }

    fn session_binding_flags(&self, session_id: SessionId) -> SessionBindingFlags {
        let Some(store) = &self.store else {
            return SessionBindingFlags::default();
        };
        match store.get_session_binding(&session_id.to_string()) {
            Ok(Some(b)) => SessionBindingFlags {
                scope_id: Some(b.scope_id),
                channel_type: Some(b.channel_type),
                external_user_id: b.external_user_id,
                external_chat_id: b.external_chat_id,
                manual_mode: b.manual_mode,
            },
            _ => SessionBindingFlags::default(),
        }
    }
}

fn infer_response_mode(
    knowledge: &KnowledgeBundle,
    tool_authorizations: &[ToolAuthorization],
) -> ResponseMode {
    if !knowledge.canned_response_candidates.is_empty() {
        return ResponseMode::CannedOnly;
    }
    if tool_authorizations
        .iter()
        .any(|auth| auth.requires_approval)
    {
        return ResponseMode::Strict;
    }
    if !knowledge.glossary_terms.is_empty() || !knowledge.context_variables.is_empty() {
        return ResponseMode::Guided;
    }
    ResponseMode::Freeform
}

// ─── Preparation iteration helpers ───────────────────────────────────────────

/// Check whether two compiled turn contexts have reached a stable state
/// (same active guideline set and same allowed tools).
fn guidelines_stable(prev: &CompiledTurnContext, next: &CompiledTurnContext) -> bool {
    use std::collections::HashSet;
    let prev_ids: HashSet<String> = prev
        .active_guidelines
        .iter()
        .map(|g| g.guideline_id.0.to_string())
        .collect();
    let next_ids: HashSet<String> = next
        .active_guidelines
        .iter()
        .map(|g| g.guideline_id.0.to_string())
        .collect();
    prev_ids == next_ids && prev.allowed_tools == next.allowed_tools
}

impl<OM, PR, JR, KC, TG> DefaultTurnControlCoordinator<OM, PR, JR, KC, TG>
where
    OM: ObservationMatcher,
    PR: PolicyResolver,
    JR: JourneyRuntime,
    KC: KnowledgeCompiler,
    TG: ToolGate,
{
    async fn compile_turn_once(
        &self,
        input: TurnInput,
        prior_audit_meta: Option<AuditMeta>,
        persist_trace: bool,
    ) -> Result<CompiledTurnContext> {
        let observations = self
            .observation_matcher
            .match_observations(&input.scope_id, &input.message)
            .await?;
        let journey = self
            .journey_runtime
            .resolve_journey(&input.scope_id, &input.session_id, &input.message)
            .await?;
        let policy = self
            .policy_resolver
            .resolve_policy(
                &input.scope_id,
                &input.message,
                &observations,
                journey
                    .active_journey
                    .as_ref()
                    .map(|j| j.current_state.as_str()),
                &input.candidate_tools,
            )
            .await?;
        let knowledge = self
            .knowledge_compiler
            .compile_knowledge(
                &input.scope_id,
                &input.message,
                journey.active_journey.as_ref(),
                &policy
                    .active_guidelines
                    .iter()
                    .map(|g| g.name.clone())
                    .collect::<Vec<_>>(),
            )
            .await?;

        let allowed_tools: Vec<String> = policy
            .tool_authorizations
            .iter()
            .map(|auth| auth.tool_name.clone())
            .collect();

        // ── Merge journey-projected guidelines into active guidelines ─────────
        // Journey state nodes can carry action_text guidelines that are treated
        // as first-class active guidelines this turn (Parlant journey projection).
        let active_guideline_names: Vec<String> = policy
            .active_guidelines
            .iter()
            .map(|g| g.name.clone())
            .collect();
        let mut all_active_guidelines = policy.active_guidelines;
        all_active_guidelines.extend(journey.projected_guidelines);
        let authorized_candidates = if input.candidate_tools.is_empty() {
            allowed_tools
                .iter()
                .map(|tool_name| openparlant_types::control::ToolCandidate {
                    tool_name: tool_name.clone(),
                    skill_ref: None,
                })
                .collect::<Vec<_>>()
        } else {
            input
                .candidate_tools
                .iter()
                .filter(|candidate| {
                    allowed_tools
                        .iter()
                        .any(|tool| tool == &candidate.tool_name)
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        let gate_decisions = self
            .tool_gate
            .evaluate(
                &input.scope_id,
                &authorized_candidates,
                &observations,
                &active_guideline_names,
                journey
                    .active_journey
                    .as_ref()
                    .map(|j| j.current_state.as_str()),
            )
            .await
            .unwrap_or_default();

        // Filter to only tools that the gate allows.
        let gated_tools: Vec<String> = gate_decisions
            .iter()
            .filter(|d| d.allowed)
            .map(|d| d.tool_name.clone())
            .collect();
        let approval_required_tools: Vec<String> = gate_decisions
            .iter()
            .filter(|d| d.allowed && d.requires_approval)
            .map(|d| d.tool_name.clone())
            .collect();

        if !approval_required_tools.is_empty() {
            debug!(
                scope = %input.scope_id,
                tools = ?approval_required_tools,
                "tool gate: approval required for these tools"
            );
        }

        let response_mode = infer_response_mode(&knowledge, &policy.tool_authorizations);
        let release_version = prior_audit_meta
            .as_ref()
            .and_then(|meta| meta.release_version.clone())
            .or_else(|| {
                self.store.as_ref().and_then(|store| {
                    store
                        .current_release_version(&input.scope_id)
                        .ok()
                        .flatten()
                })
            });
        let audit_meta = if let Some(mut meta) = prior_audit_meta {
            meta.scope_id = input.scope_id.clone();
            meta.compiled_at = Utc::now();
            if meta.release_version.is_none() {
                meta.release_version = release_version.clone();
            }
            meta
        } else {
            AuditMeta {
                scope_id: input.scope_id.clone(),
                release_version,
                compiled_at: Utc::now(),
                ..AuditMeta::default()
            }
        };

        let compiled_context_hash = serde_json::to_string(&serde_json::json!({
            "scope_id": &input.scope_id,
            "channel_type": &input.message.channel_type,
            "active_observations": &observations,
            "active_guidelines": &all_active_guidelines,
            "excluded_guidelines": &policy.excluded_guidelines,
            "active_journey": &journey.active_journey,
            "retrieved_chunks": &knowledge.retrieved_chunks,
            "glossary_terms": &knowledge.glossary_terms,
            "context_variables": &knowledge.context_variables,
            "canned_response_candidates": &knowledge.canned_response_candidates,
            "tool_control_active": policy.tool_control_active,
            "allowed_tools": &gated_tools,
            "approval_required_tools": &approval_required_tools,
            "response_mode": response_mode,
            "release_version": &audit_meta.release_version,
        }))
        .ok()
        .map(|json| {
            let mut hasher = Sha256::new();
            hasher.update(json.as_bytes());
            format!("{:x}", hasher.finalize())
        });

        debug!(
            scope = %input.scope_id,
            trace_id = %audit_meta.trace_id,
            observations = observations.len(),
            guidelines = all_active_guidelines.len(),
            tools = allowed_tools.len(),
            tool_control_active = policy.tool_control_active,
            "compiled turn context"
        );

        // Persist turn trace (best-effort – failure is logged, not propagated)
        if persist_trace {
            if let Some(store) = &self.store {
                use openparlant_types::control::TurnTraceRecord;
                let trace = TurnTraceRecord {
                    trace_id: audit_meta.trace_id,
                    scope_id: input.scope_id.clone(),
                    session_id: input.session_id,
                    agent_id: input.agent_id,
                    channel_type: input.message.channel_type.clone(),
                    request_message_ref: input.message.external_message_id.clone(),
                    compiled_context_hash: compiled_context_hash.clone(),
                    release_version: audit_meta.release_version.clone(),
                    response_mode,
                    created_at: audit_meta.compiled_at,
                };
                if let Err(e) = store.upsert_turn_trace(&trace) {
                    warn!(trace_id = %audit_meta.trace_id, error = %e, "failed to persist turn trace");
                }
            }
        }

        Ok(CompiledTurnContext {
            agent_id: input.agent_id,
            session_id: input.session_id,
            canonical_message: input.message,
            active_observations: observations,
            active_guidelines: all_active_guidelines,
            excluded_guidelines: policy.excluded_guidelines,
            active_journey: journey.active_journey,
            retrieved_chunks: knowledge.retrieved_chunks,
            glossary_terms: knowledge.glossary_terms,
            context_variables: knowledge.context_variables,
            canned_response_candidates: knowledge.canned_response_candidates,
            tool_control_active: policy.tool_control_active,
            allowed_tools: gated_tools,
            approval_required_tools,
            tool_authorizations: policy.tool_authorizations,
            response_mode,
            audit_meta,
        })
    }

    async fn compile_turn_iterative_impl(
        &self,
        mut input: TurnInput,
    ) -> anyhow::Result<CompiledTurnContext> {
        const MAX_PREP_ITERATIONS: usize = 3;

        let mut last_ctx = self.compile_turn_once(input.clone(), None, true).await?;
        let original_message_text = input.message.text.clone();

        for _iter in 1..MAX_PREP_ITERATIONS {
            if input.prior_tool_calls.is_empty() {
                break;
            }

            // Enrich message text with tool call results for re-evaluation.
            let tool_summary = input
                .prior_tool_calls
                .iter()
                .map(|tc| {
                    format!(
                        "[tool:{} result:{}]",
                        tc.tool_name,
                        tc.result.as_deref().unwrap_or("(empty)")
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");

            input.message.text = format!(
                "{}\n\nTool call results: {tool_summary}",
                original_message_text
            );

            let new_ctx = self
                .compile_turn_once(input.clone(), Some(last_ctx.audit_meta.clone()), false)
                .await?;

            // Stop iterating once the active guidelines and allowed tools stabilize.
            if guidelines_stable(&last_ctx, &new_ctx) {
                last_ctx = new_ctx;
                break;
            }

            last_ctx = new_ctx;
            // Prior calls have been incorporated; clear to avoid re-appending next round.
            input.prior_tool_calls.clear();
        }

        Ok(last_ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openparlant_types::agent::{AgentId, SessionId};
    use openparlant_types::control::{CanonicalMessage, ScopeId, TurnInput};

    #[tokio::test]
    async fn noop_coordinator_compiles_empty_context() {
        let coordinator = DefaultTurnControlCoordinator::noop();
        let input = TurnInput {
            scope_id: ScopeId::default(),
            agent_id: AgentId::new(),
            session_id: SessionId::new(),
            message: CanonicalMessage::text(ScopeId::default(), "web", "hello"),
            candidate_tools: Vec::new(),
            prior_tool_calls: Vec::new(),
        };

        let ctx = coordinator.compile_turn(input).await.unwrap();
        assert!(ctx.active_observations.is_empty());
        assert!(ctx.allowed_tools.is_empty());
        assert_eq!(ctx.response_mode, ResponseMode::Freeform);
    }
}
