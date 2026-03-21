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
        let pre_relationship_excluded: Vec<ExcludedGuideline> = Vec::new();
        let _ = pre_relationship_excluded; // extended below via relationship graph

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

        // ── Guideline relationship graph ──────────────────────────────────────
        // Apply depends_on / excludes / prioritizes_over rules to produce the
        // final active and excluded sets.
        let relationships = self
            .store
            .list_guideline_relationships(scope_id)
            .unwrap_or_default();

        // Build a fast lookup: guideline_id → index in active_guidelines.
        use std::collections::{HashMap, HashSet};
        let id_to_idx: HashMap<String, usize> = active_guidelines
            .iter()
            .enumerate()
            .map(|(i, g)| (g.guideline_id.0.to_string(), i))
            .collect();

        let mut to_exclude: HashSet<String> = HashSet::new();
        let mut exclusion_reasons: HashMap<String, String> = HashMap::new();

        for (_, from_id, to_id, relation_type) in &relationships {
            match relation_type.as_str() {
                "depends_on" => {
                    // `from` requires `to` to be active; if `to` is not active, exclude `from`.
                    if id_to_idx.contains_key(from_id) && !id_to_idx.contains_key(to_id) {
                        to_exclude.insert(from_id.clone());
                        exclusion_reasons.insert(
                            from_id.clone(),
                            format!("depends_on {to_id} which is not active"),
                        );
                    }
                }
                "excludes" => {
                    // Mutual exclusion: keep the higher-priority one.
                    let from_idx = id_to_idx.get(from_id);
                    let to_idx = id_to_idx.get(to_id);
                    if let (Some(&fi), Some(&ti)) = (from_idx, to_idx) {
                        let from_priority = active_guidelines[fi].priority;
                        let to_priority = active_guidelines[ti].priority;
                        if from_priority >= to_priority {
                            to_exclude.insert(to_id.clone());
                            exclusion_reasons.insert(
                                to_id.clone(),
                                format!("excluded by {from_id} (priority {from_priority} >= {to_priority})"),
                            );
                        } else {
                            to_exclude.insert(from_id.clone());
                            exclusion_reasons.insert(
                                from_id.clone(),
                                format!("excluded by {to_id} (priority {to_priority} > {from_priority})"),
                            );
                        }
                    }
                }
                "prioritizes_over" => {
                    // `from` explicitly wins over `to`; exclude `to` if both active.
                    if id_to_idx.contains_key(from_id) && id_to_idx.contains_key(to_id) {
                        to_exclude.insert(to_id.clone());
                        exclusion_reasons.insert(
                            to_id.clone(),
                            format!("prioritized_over by {from_id}"),
                        );
                    }
                }
                _ => {}
            }
        }

        // Separate active from excluded after applying relationships.
        let mut excluded_guidelines: Vec<ExcludedGuideline> = Vec::new();
        let active_guidelines: Vec<GuidelineActivation> = active_guidelines
            .into_iter()
            .filter(|g| {
                let gid = g.guideline_id.0.to_string();
                if to_exclude.contains(&gid) {
                    excluded_guidelines.push(ExcludedGuideline {
                        guideline_id: g.guideline_id,
                        name: g.name.clone(),
                        reason: exclusion_reasons
                            .get(&gid)
                            .cloned()
                            .unwrap_or_else(|| "relationship conflict".to_string()),
                    });
                    false
                } else {
                    true
                }
            })
            .collect();

        // Build tool_authorizations from tool_exposure_policies.
        // Each enabled policy whose preconditions match the current observations / guidelines
        // contributes a ToolAuthorization entry.
        let tool_policies = self.store.list_tool_exposure_policies(scope_id).unwrap_or_default();
        let obs_names: Vec<&str> = observations.iter().map(|o| o.name.as_str()).collect();
        let guideline_names: Vec<&str> = active_guidelines.iter().map(|g| g.name.as_str()).collect();

        let mut tool_authorizations: Vec<ToolAuthorization> = Vec::new();
        for policy in &tool_policies {
            if !policy.enabled {
                continue;
            }
            // Check observation_ref precondition.
            if let Some(ref obs_ref) = policy.observation_ref {
                if !obs_names.contains(&obs_ref.as_str()) {
                    continue;
                }
            }
            // Check guideline_ref precondition.
            if let Some(ref g_ref) = policy.guideline_ref {
                if !guideline_names.contains(&g_ref.as_str()) {
                    continue;
                }
            }
            let requires_approval = matches!(
                policy.approval_mode,
                openparlant_types::control::ApprovalMode::Required
                    | openparlant_types::control::ApprovalMode::Conditional
            );
            let mut reason = format!("policy:{}", policy.policy_id);
            if let Some(ref obs_ref) = policy.observation_ref {
                reason.push_str(&format!(" (observation:{obs_ref})"));
            }
            if let Some(ref g_ref) = policy.guideline_ref {
                reason.push_str(&format!(" (guideline:{g_ref})"));
            }
            tool_authorizations.push(ToolAuthorization {
                tool_name: policy.tool_name.clone(),
                reasons: vec![reason],
                requires_approval,
            });
        }

        Ok(PolicyResolution {
            active_guidelines,
            excluded_guidelines,
            tool_authorizations,
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

