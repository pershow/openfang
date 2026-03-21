//! Policy matching traits and baseline implementations for the control plane.

mod store;

use anyhow::Result;
use async_trait::async_trait;
use openparlant_types::control::{
    CanonicalMessage, ExcludedGuideline, GuidelineActivation, ObservationHit, ScopeId,
    ToolAuthorization,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
pub use store::PolicyStore;

/// Output of policy resolution for a single turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyResolution {
    pub active_guidelines: Vec<GuidelineActivation>,
    pub excluded_guidelines: Vec<ExcludedGuideline>,
    pub tool_authorizations: Vec<ToolAuthorization>,
}

/// Match structured observations from an inbound message.
#[async_trait]
pub trait ObservationMatcher: Send + Sync {
    async fn match_observations(
        &self,
        scope_id: &ScopeId,
        message: &CanonicalMessage,
    ) -> Result<Vec<ObservationHit>>;
}

/// Resolve active guidelines and tool authorizations from matched observations.
#[async_trait]
pub trait PolicyResolver: Send + Sync {
    async fn resolve_policy(
        &self,
        scope_id: &ScopeId,
        message: &CanonicalMessage,
        observations: &[ObservationHit],
    ) -> Result<PolicyResolution>;
}

// ─── SQLite-backed implementations ───────────────────────────────────────────

pub struct SqliteObservationMatcher {
    store: PolicyStore,
}

impl SqliteObservationMatcher {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            store: PolicyStore::new(conn),
        }
    }
}

#[async_trait]
impl ObservationMatcher for SqliteObservationMatcher {
    async fn match_observations(
        &self,
        scope_id: &ScopeId,
        message: &CanonicalMessage,
    ) -> Result<Vec<ObservationHit>> {
        let observations = self.store.list_observations(scope_id, true)?;
        let mut hits = Vec::new();

        for obs in observations {
            if !obs.enabled {
                continue;
            }
            // MVP: keyword match on observation name against message text.
            // Later this will dispatch on `obs.matcher_type` + `obs.matcher_config`.
            if message.text.to_lowercase().contains(&obs.name.to_lowercase()) {
                hits.push(ObservationHit {
                    observation_id: obs.observation_id,
                    name: obs.name.clone(),
                    confidence: Some(1.0),
                    matched_by: "sqlite_observation_matcher (keyword)".to_string(),
                });
            }
        }

        Ok(hits)
    }
}

pub struct SqlitePolicyResolver {
    store: PolicyStore,
}

impl SqlitePolicyResolver {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            store: PolicyStore::new(conn),
        }
    }
}

#[async_trait]
impl PolicyResolver for SqlitePolicyResolver {
    async fn resolve_policy(
        &self,
        scope_id: &ScopeId,
        _message: &CanonicalMessage,
        observations: &[ObservationHit],
    ) -> Result<PolicyResolution> {
        let guidelines = self.store.list_guidelines(scope_id, true)?;

        let mut active_guidelines: Vec<GuidelineActivation> = Vec::new();
        let excluded_guidelines: Vec<ExcludedGuideline> = Vec::new();

        for guideline in guidelines {
            if !guideline.enabled {
                continue;
            }

            let is_active =
                guideline.condition_ref.is_empty() || guideline.condition_ref == "always"
                || observations.iter().any(|obs| obs.name == guideline.condition_ref);

            if !is_active {
                continue;
            }

            // MVP: no relationship graph yet – everything that matches is active.
            active_guidelines.push(GuidelineActivation {
                guideline_id: guideline.guideline_id,
                name: guideline.name,
                action_text: guideline.action_text,
                priority: guideline.priority,
                source_observations: observations
                    .iter()
                    .filter(|obs| obs.name == guideline.condition_ref)
                    .map(|obs| obs.observation_id)
                    .collect(),
            });
        }

        Ok(PolicyResolution {
            active_guidelines,
            excluded_guidelines,
            tool_authorizations: Vec::new(),
        })
    }
}

// ─── No-op implementations (used during incremental bring-up) ─────────────────

/// Default no-op observation matcher.
#[derive(Debug, Default)]
pub struct NoopObservationMatcher;

#[async_trait]
impl ObservationMatcher for NoopObservationMatcher {
    async fn match_observations(
        &self,
        _scope_id: &ScopeId,
        _message: &CanonicalMessage,
    ) -> Result<Vec<ObservationHit>> {
        Ok(Vec::new())
    }
}

/// Default no-op policy resolver.
#[derive(Debug, Default)]
pub struct NoopPolicyResolver;

#[async_trait]
impl PolicyResolver for NoopPolicyResolver {
    async fn resolve_policy(
        &self,
        _scope_id: &ScopeId,
        _message: &CanonicalMessage,
        _observations: &[ObservationHit],
    ) -> Result<PolicyResolution> {
        Ok(PolicyResolution::default())
    }
}

// ─── Tool Gate ────────────────────────────────────────────────────────────────

use openparlant_types::control::{ApprovalMode, ToolGateDecision};

/// Trait for resolving which tools are visible and whether they need approval.
#[async_trait]
pub trait ToolGate: Send + Sync {
    /// Given the current observations and active guidelines, compute the
    /// tool-gate decisions for the provided candidate tool names.
    async fn evaluate(
        &self,
        scope_id: &ScopeId,
        candidate_tools: &[String],
        observations: &[ObservationHit],
        active_guideline_names: &[String],
    ) -> Result<Vec<ToolGateDecision>>;
}

/// SQLite-backed tool gate.
///
/// Evaluates `tool_exposure_policies` for each candidate tool.
/// When a policy is found and its preconditions match, the policy's
/// `approval_mode` determines whether approval is needed.
/// Tools with no policy entry are allowed by default (open-by-default MVP).
pub struct SqliteToolGate {
    store: PolicyStore,
}

impl SqliteToolGate {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            store: PolicyStore::new(conn),
        }
    }
}

#[async_trait]
impl ToolGate for SqliteToolGate {
    async fn evaluate(
        &self,
        scope_id: &ScopeId,
        candidate_tools: &[String],
        observations: &[ObservationHit],
        active_guideline_names: &[String],
    ) -> Result<Vec<ToolGateDecision>> {
        let policies = self.store.list_tool_exposure_policies(scope_id)?;
        let obs_names: Vec<&str> = observations.iter().map(|o| o.name.as_str()).collect();

        let mut decisions = Vec::new();
        for tool in candidate_tools {
            // Find matching enabled policy for this tool name.
            let matching = policies
                .iter()
                .filter(|p| p.enabled && p.tool_name == *tool)
                .find(|p| {
                    // Check observation_ref precondition (if any).
                    if let Some(ref obs_ref) = p.observation_ref {
                        if !obs_names.contains(&obs_ref.as_str()) {
                            return false;
                        }
                    }
                    // Check guideline_ref precondition (if any).
                    if let Some(ref g_ref) = p.guideline_ref {
                        if !active_guideline_names.contains(g_ref) {
                            return false;
                        }
                    }
                    true
                });

            let decision = if let Some(policy) = matching {
                let requires_approval = matches!(
                    policy.approval_mode,
                    ApprovalMode::Required | ApprovalMode::Conditional
                );
                ToolGateDecision {
                    tool_name: tool.clone(),
                    allowed: true,
                    reason: format!(
                        "policy '{}' (approval_mode={})",
                        policy.policy_id, policy.approval_mode
                    ),
                    requires_approval,
                }
            } else {
                // No policy → open by default.
                ToolGateDecision {
                    tool_name: tool.clone(),
                    allowed: true,
                    reason: "no policy (open by default)".to_string(),
                    requires_approval: false,
                }
            };
            decisions.push(decision);
        }
        Ok(decisions)
    }
}

/// No-op tool gate (allows everything, requires no approval).
#[derive(Debug, Default)]
pub struct NoopToolGate;

#[async_trait]
impl ToolGate for NoopToolGate {
    async fn evaluate(
        &self,
        _scope_id: &ScopeId,
        candidate_tools: &[String],
        _observations: &[ObservationHit],
        _active_guideline_names: &[String],
    ) -> Result<Vec<ToolGateDecision>> {
        Ok(candidate_tools
            .iter()
            .map(|t| ToolGateDecision {
                tool_name: t.clone(),
                allowed: true,
                reason: "noop gate".to_string(),
                requires_approval: false,
            })
            .collect())
    }
}

