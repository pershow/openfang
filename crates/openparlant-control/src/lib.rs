//! Orchestrates control-plane compilation around the existing runtime loop.

mod store;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use openparlant_context::{KnowledgeBundle, KnowledgeCompiler, NoopKnowledgeCompiler};
use openparlant_journey::{JourneyRuntime, NoopJourneyRuntime};
use openparlant_policy::{
    NoopObservationMatcher, NoopPolicyResolver, NoopToolGate,
    ObservationMatcher, PolicyResolver, ToolGate,
};
use openparlant_types::control::{
    AuditMeta, CompiledTurnContext, PolicyMatchRecord, ResponseMode, ToolAuthorization,
    ToolAuthorizationRecord, TurnControlCoordinator, TurnInput, TurnOutcome,
};
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
        }
    }

    /// Attach a `ControlStore` so that turn traces are persisted.
    pub fn with_store(mut self, store: ControlStore) -> Self {
        self.store = Some(store);
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
impl<OM, PR, JR, KC, TG> TurnControlCoordinator for DefaultTurnControlCoordinator<OM, PR, JR, KC, TG>
where
    OM: ObservationMatcher,
    PR: PolicyResolver,
    JR: JourneyRuntime,
    KC: KnowledgeCompiler,
    TG: ToolGate,
{
    async fn compile_turn(&self, input: TurnInput) -> Result<CompiledTurnContext> {
        let observations = self
            .observation_matcher
            .match_observations(&input.scope_id, &input.message)
            .await?;
        let policy = self
            .policy_resolver
            .resolve_policy(&input.scope_id, &input.message, &observations)
            .await?;
        let journey = self
            .journey_runtime
            .resolve_journey(&input.scope_id, &input.message)
            .await?;
        let knowledge = self
            .knowledge_compiler
            .compile_knowledge(
                &input.scope_id,
                &input.message,
                journey.active_journey.as_ref(),
            )
            .await?;

        let allowed_tools: Vec<String> = policy
            .tool_authorizations
            .iter()
            .map(|auth| auth.tool_name.clone())
            .collect();

        // ── Tool Gate ─────────────────────────────────────────────────────────
        // Evaluate which tools should be visible this turn and which need approval.
        let active_guideline_names: Vec<String> = policy
            .active_guidelines
            .iter()
            .map(|g| g.name.clone())
            .collect();
        let gate_decisions = self
            .tool_gate
            .evaluate(
                &input.scope_id,
                &allowed_tools,
                &observations,
                &active_guideline_names,
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
        let audit_meta = AuditMeta {
            scope_id: input.scope_id.clone(),
            compiled_at: Utc::now(),
            ..AuditMeta::default()
        };

        debug!(
            scope = %input.scope_id,
            trace_id = %audit_meta.trace_id,
            observations = observations.len(),
            guidelines = policy.active_guidelines.len(),
            tools = allowed_tools.len(),
            "compiled turn context"
        );

        // Persist turn trace (best-effort – failure is logged, not propagated)
        if let Some(store) = &self.store {
            use openparlant_types::control::TurnTraceRecord;
            let trace = TurnTraceRecord {
                trace_id: audit_meta.trace_id,
                scope_id: input.scope_id.clone(),
                session_id: input.session_id,
                agent_id: input.agent_id,
                channel_type: input.message.channel_type.clone(),
                request_message_ref: input.message.external_message_id.clone(),
                compiled_context_hash: None,
                response_mode,
                created_at: audit_meta.compiled_at,
            };
            if let Err(e) = store.upsert_turn_trace(&trace) {
                warn!(trace_id = %audit_meta.trace_id, error = %e, "failed to persist turn trace");
            }
        }

        Ok(CompiledTurnContext {
            agent_id: input.agent_id,
            session_id: input.session_id,
            canonical_message: input.message,
            active_observations: observations,
            active_guidelines: policy.active_guidelines,
            excluded_guidelines: policy.excluded_guidelines,
            active_journey: journey.active_journey,
            retrieved_chunks: knowledge.retrieved_chunks,
            glossary_terms: knowledge.glossary_terms,
            context_variables: knowledge.context_variables,
            canned_response_candidates: knowledge.canned_response_candidates,
            allowed_tools: gated_tools,
            approval_required_tools,
            tool_authorizations: policy.tool_authorizations,
            response_mode,
            audit_meta,
        })
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
                    journey_instance_id: String::new(), // instance_id is inside JourneyUpdate if needed
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

            // ── Policy match record ────────────────────────────────────────────
            let rec_id = openparlant_types::control::TraceId::new();
            let pmr = PolicyMatchRecord {
                record_id: rec_id,
                trace_id: outcome.trace_id,
                observation_hits_json: "[]".to_string(),
                guideline_hits_json: "[]".to_string(),
                guideline_exclusions_json: "[]".to_string(),
            };
            if let Err(e) = store.upsert_policy_match_record(&pmr) {
                warn!(trace_id = %outcome.trace_id, error = %e, "failed to persist policy match record");
            }

            // ── Tool authorization record ──────────────────────────────────────
            let tar = ToolAuthorizationRecord {
                record_id: openparlant_types::control::TraceId::new(),
                trace_id: outcome.trace_id,
                allowed_tools_json: serde_json::to_string(
                    &outcome
                        .tool_calls
                        .iter()
                        .map(|tc| &tc.tool_name)
                        .collect::<Vec<_>>(),
                )
                .unwrap_or_default(),
                authorization_reasons_json: "{}".to_string(),
                approval_requirements_json: "{}".to_string(),
            };
            if let Err(e) = store.upsert_tool_authorization_record(&tar) {
                warn!(trace_id = %outcome.trace_id, error = %e, "failed to persist tool auth record");
            }
        }

        Ok(())
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
        };

        let ctx = coordinator.compile_turn(input).await.unwrap();
        assert!(ctx.active_observations.is_empty());
        assert!(ctx.allowed_tools.is_empty());
        assert_eq!(ctx.response_mode, ResponseMode::Freeform);
    }
}
