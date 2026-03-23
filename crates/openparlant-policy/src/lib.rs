//! Policy matching traits and baseline implementations for the control plane.

mod store;

use anyhow::Result;
use async_trait::async_trait;
use openparlant_memory::db::SharedDb;
use openparlant_types::control::{
    CanonicalMessage, ExcludedGuideline, GuidelineActivation, ObservationDefinition,
    ObservationHit, ScopeId, ToolAuthorization, ToolCandidate, ToolExposurePolicy,
};
#[cfg(test)]
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::sync::{Arc, Mutex};
pub use store::PolicyStore;

/// Output of policy resolution for a single turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyResolution {
    pub active_guidelines: Vec<GuidelineActivation>,
    pub excluded_guidelines: Vec<ExcludedGuideline>,
    pub tool_authorizations: Vec<ToolAuthorization>,
    /// When true, this scope is using control-plane-managed tool authorization.
    ///
    /// Managed scopes switch to deny-by-default for tools; unmanaged scopes
    /// keep the legacy runtime-visible tool set for backward compatibility.
    pub tool_control_active: bool,
}

/// Normalize relationship names coming from control-plane APIs or legacy data.
///
/// The control-plane UI/API historically used friendlier names such as
/// `"overrides"` / `"conflicts_with"` / `"requires"`, while the resolver
/// logic expects canonical internal names such as
/// `"prioritizes_over"` / `"excludes"` / `"depends_on"`.
pub fn canonical_guideline_relation_type(relation_type: &str) -> Option<&'static str> {
    match relation_type.trim().to_ascii_lowercase().as_str() {
        "requires" | "depends_on" => Some("depends_on"),
        "conflicts_with" | "excludes" => Some("excludes"),
        "overrides" | "prioritizes_over" => Some("prioritizes_over"),
        // Keep complements as a recognised no-op relation so older payloads
        // round-trip cleanly even though the current MVP resolver has no
        // additional semantics for it yet.
        "complements" => Some("complements"),
        _ => None,
    }
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
        active_journey_state: Option<&str>,
        candidate_tools: &[ToolCandidate],
    ) -> Result<PolicyResolution>;
}

// ─── Shared database-backed implementations ──────────────────────────────────

pub struct SqliteObservationMatcher {
    store: PolicyStore,
}

impl SqliteObservationMatcher {
    pub fn new(db: impl Into<SharedDb>) -> Self {
        Self {
            store: PolicyStore::new(db),
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
            if let Some(hit) = match_observation_deterministic(&obs, &message.text) {
                hits.push(hit);
            }
        }

        Ok(hits)
    }
}

/// Deterministic observation match: dispatches on `matcher_type` + `matcher_config`.
///
/// - `always` — always matches.
/// - `keyword` / `contains` / empty — `matcher_config.contains` (any term), `matcher_config.all`
///   (all terms), or fallback: observation `name` appears in message (legacy).
/// - `regex` — `matcher_config.pattern` against full message text.
/// - `semantic` — not implemented yet; never matches (use separate pipeline later).
fn match_observation_deterministic(
    obs: &ObservationDefinition,
    text: &str,
) -> Option<ObservationHit> {
    let mt = obs.matcher_type.to_lowercase();
    let text_lc = text.to_lowercase();

    let hit = |matched_by: &str| ObservationHit {
        observation_id: obs.observation_id,
        name: obs.name.clone(),
        confidence: Some(1.0),
        matched_by: matched_by.to_string(),
    };

    match mt.as_str() {
        "always" => return Some(hit("deterministic_observation_matcher (always)")),

        "semantic" => {
            tracing::trace!(
                observation = %obs.name,
                "semantic observation matcher not implemented; skipping"
            );
            return None;
        }

        "regex" => {
            let pattern = obs.matcher_config.get("pattern").and_then(|v| v.as_str())?;
            match regex_lite::Regex::new(pattern) {
                Ok(re) => {
                    if re.is_match(text) {
                        return Some(hit("deterministic_observation_matcher (regex)"));
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        observation = %obs.name,
                        error = %e,
                        "invalid regex in observation matcher_config.pattern"
                    );
                }
            }
            return None;
        }

        "keyword" | "contains" | "" => {}

        other => {
            tracing::warn!(
                matcher_type = %other,
                observation = %obs.name,
                "unknown observation matcher_type; falling back to keyword rules"
            );
        }
    }

    // keyword / contains / default
    if let Some(s) = obs.matcher_config.get("substring").and_then(|v| v.as_str()) {
        if text_lc.contains(&s.to_lowercase()) {
            return Some(hit("deterministic_observation_matcher (keyword:substring)"));
        }
        return None;
    }

    if let Some(arr) = obs
        .matcher_config
        .get("contains")
        .and_then(|v| v.as_array())
    {
        let terms: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
            .collect();
        if !terms.is_empty() && terms.iter().any(|t| text_lc.contains(t)) {
            return Some(hit("deterministic_observation_matcher (keyword:contains)"));
        }
        if !terms.is_empty() {
            return None;
        }
    }

    if let Some(arr) = obs.matcher_config.get("all").and_then(|v| v.as_array()) {
        let terms: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
            .collect();
        if !terms.is_empty() && terms.iter().all(|t| text_lc.contains(t)) {
            return Some(hit("deterministic_observation_matcher (keyword:all)"));
        }
        if !terms.is_empty() {
            return None;
        }
    }

    if text_lc.contains(&obs.name.to_lowercase()) {
        return Some(hit("deterministic_observation_matcher (keyword:name)"));
    }

    None
}

#[cfg(test)]
mod observation_deterministic_tests {
    use super::match_observation_deterministic;
    use openparlant_types::control::{ObservationDefinition, ObservationId, ScopeId};
    use serde_json::json;

    fn obs(name: &str, matcher_type: &str, config: serde_json::Value) -> ObservationDefinition {
        ObservationDefinition {
            observation_id: ObservationId::new(),
            scope_id: ScopeId::default(),
            name: name.to_string(),
            matcher_type: matcher_type.to_string(),
            matcher_config: config,
            priority: 0,
            enabled: true,
        }
    }

    #[test]
    fn keyword_contains_matches_any_term() {
        let o = obs("vip_user", "keyword", json!({ "contains": ["vip"] }));
        assert!(match_observation_deterministic(&o, "I am VIP").is_some());
        assert!(match_observation_deterministic(&o, "hello").is_none());
    }

    #[test]
    fn keyword_name_fallback_when_no_config_terms() {
        let o = obs("refund", "keyword", json!({}));
        assert!(match_observation_deterministic(&o, "refund please").is_some());
    }

    #[test]
    fn substring_config() {
        let o = obs("x", "keyword", json!({ "substring": "urgent" }));
        assert!(match_observation_deterministic(&o, "URGENT help").is_some());
        assert!(match_observation_deterministic(&o, "later").is_none());
    }

    #[test]
    fn regex_pattern() {
        let o = obs("code", "regex", json!({ "pattern": r"\b\d{3}\b" }));
        assert!(match_observation_deterministic(&o, "code 404").is_some());
        assert!(match_observation_deterministic(&o, "no digits").is_none());
    }

    #[test]
    fn always_matches() {
        let o = obs("x", "always", json!({}));
        assert!(match_observation_deterministic(&o, "").is_some());
    }

    #[test]
    fn all_terms_requires_every_token() {
        let o = obs("combo", "keyword", json!({ "all": ["a", "b"] }));
        assert!(match_observation_deterministic(&o, "a and b").is_some());
        assert!(match_observation_deterministic(&o, "only a").is_none());
    }
}

fn tool_policy_preconditions_match(
    policy: &ToolExposurePolicy,
    obs_names: &[&str],
    active_guideline_names: &[String],
    active_journey_state: Option<&str>,
) -> bool {
    if !policy.enabled {
        return false;
    }
    if let Some(ref obs_ref) = policy.observation_ref {
        if !obs_names.contains(&obs_ref.as_str()) {
            return false;
        }
    }
    if let Some(ref g_ref) = policy.guideline_ref {
        if !active_guideline_names.contains(g_ref) {
            return false;
        }
    }
    if let Some(ref journey_state_ref) = policy.journey_state_ref {
        if active_journey_state != Some(journey_state_ref.as_str()) {
            return false;
        }
    }
    true
}

fn candidate_matches_policy(candidate: &ToolCandidate, policy: &ToolExposurePolicy) -> bool {
    let tool_matches = if policy.skill_ref.is_some() {
        !policy.tool_name.trim().is_empty()
            && policy.tool_name != "*"
            && policy.tool_name == candidate.tool_name
    } else {
        policy.tool_name == "*"
            || (!policy.tool_name.trim().is_empty() && policy.tool_name == candidate.tool_name)
    };
    let skill_matches = policy
        .skill_ref
        .as_ref()
        .zip(candidate.skill_ref.as_ref())
        .is_some_and(|(policy_skill, candidate_skill)| policy_skill == candidate_skill);
    tool_matches || skill_matches
}

fn tool_policy_reason(policy: &ToolExposurePolicy) -> String {
    let mut reason = format!("policy:{}", policy.policy_id);
    if let Some(ref skill_ref) = policy.skill_ref {
        reason.push_str(&format!(" (skill:{skill_ref})"));
    }
    if let Some(ref obs_ref) = policy.observation_ref {
        reason.push_str(&format!(" (observation:{obs_ref})"));
    }
    if let Some(ref g_ref) = policy.guideline_ref {
        reason.push_str(&format!(" (guideline:{g_ref})"));
    }
    if let Some(ref journey_state_ref) = policy.journey_state_ref {
        reason.push_str(&format!(" (journey_state:{journey_state_ref})"));
    }
    reason
}

fn scope_uses_managed_tool_control(policies: &[ToolExposurePolicy]) -> bool {
    !policies.is_empty()
}

fn build_tool_authorizations(
    policies: &[ToolExposurePolicy],
    candidate_tools: &[ToolCandidate],
    observations: &[ObservationHit],
    active_guideline_names: &[String],
    active_journey_state: Option<&str>,
) -> Vec<ToolAuthorization> {
    let obs_names: Vec<&str> = observations.iter().map(|o| o.name.as_str()).collect();

    if candidate_tools.is_empty() {
        return policies
            .iter()
            .filter(|policy| {
                tool_policy_preconditions_match(
                    policy,
                    &obs_names,
                    active_guideline_names,
                    active_journey_state,
                )
            })
            .map(|policy| ToolAuthorization {
                tool_name: policy.tool_name.clone(),
                reasons: vec![tool_policy_reason(policy)],
                requires_approval: matches!(
                    policy.approval_mode,
                    openparlant_types::control::ApprovalMode::Required
                        | openparlant_types::control::ApprovalMode::Conditional
                ),
            })
            .collect();
    }

    let mut authorizations = Vec::new();
    for candidate in candidate_tools {
        let matching: Vec<&ToolExposurePolicy> = policies
            .iter()
            .filter(|policy| {
                tool_policy_preconditions_match(
                    policy,
                    &obs_names,
                    active_guideline_names,
                    active_journey_state,
                ) && candidate_matches_policy(candidate, policy)
            })
            .collect();

        if matching.is_empty() {
            continue;
        }

        authorizations.push(ToolAuthorization {
            tool_name: candidate.tool_name.clone(),
            reasons: matching
                .iter()
                .map(|policy| tool_policy_reason(policy))
                .collect(),
            requires_approval: matching.iter().any(|policy| {
                matches!(
                    policy.approval_mode,
                    openparlant_types::control::ApprovalMode::Required
                        | openparlant_types::control::ApprovalMode::Conditional
                )
            }),
        });
    }

    authorizations
}

pub struct SqlitePolicyResolver {
    store: PolicyStore,
}

/// Backend-agnostic alias for the default store-backed observation matcher.
pub type StoreObservationMatcher = SqliteObservationMatcher;

/// Backend-agnostic alias for the default store-backed policy resolver.
pub type StorePolicyResolver = SqlitePolicyResolver;

impl SqlitePolicyResolver {
    pub fn new(db: impl Into<SharedDb>) -> Self {
        Self {
            store: PolicyStore::new(db),
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
        active_journey_state: Option<&str>,
        candidate_tools: &[ToolCandidate],
    ) -> Result<PolicyResolution> {
        let guidelines = self.store.list_guidelines(scope_id, true)?;

        let mut active_guidelines: Vec<GuidelineActivation> = Vec::new();
        let pre_relationship_excluded: Vec<ExcludedGuideline> = Vec::new();
        let _ = pre_relationship_excluded; // extended below via relationship graph

        for guideline in guidelines {
            if !guideline.enabled {
                continue;
            }

            let is_active = guideline.condition_ref.is_empty()
                || guideline.condition_ref == "always"
                || observations
                    .iter()
                    .any(|obs| obs.name == guideline.condition_ref);

            if !is_active {
                continue;
            }

            // MVP: no relationship graph yet – everything that matches is active.
            active_guidelines.push(GuidelineActivation {
                guideline_id: guideline.guideline_id,
                name: guideline.name,
                action_text: guideline.action_text,
                composition_mode: Some(guideline.composition_mode),
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
            match canonical_guideline_relation_type(relation_type).unwrap_or("") {
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
                        exclusion_reasons
                            .insert(to_id.clone(), format!("prioritized_over by {from_id}"));
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
        let tool_policies = self
            .store
            .list_tool_exposure_policies(scope_id)
            .unwrap_or_default();
        let tool_control_active = scope_uses_managed_tool_control(&tool_policies);
        let guideline_names: Vec<String> =
            active_guidelines.iter().map(|g| g.name.clone()).collect();
        let tool_authorizations = build_tool_authorizations(
            &tool_policies,
            candidate_tools,
            observations,
            &guideline_names,
            active_journey_state,
        );

        Ok(PolicyResolution {
            active_guidelines,
            excluded_guidelines,
            tool_authorizations,
            tool_control_active,
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
        _active_journey_state: Option<&str>,
        _candidate_tools: &[ToolCandidate],
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
        candidate_tools: &[ToolCandidate],
        observations: &[ObservationHit],
        active_guideline_names: &[String],
        active_journey_state: Option<&str>,
    ) -> Result<Vec<ToolGateDecision>>;
}

/// Shared database-backed tool gate.
///
/// Evaluates `tool_exposure_policies` for each candidate tool.
/// When a scope has any tool policy entries, the scope is treated as
/// control-plane-managed and switches to deny-by-default.
/// Scopes with no tool policies keep the legacy open-by-default behavior.
pub struct SqliteToolGate {
    store: PolicyStore,
}

impl SqliteToolGate {
    pub fn new(db: impl Into<SharedDb>) -> Self {
        Self {
            store: PolicyStore::new(db),
        }
    }
}

#[async_trait]
impl ToolGate for SqliteToolGate {
    async fn evaluate(
        &self,
        scope_id: &ScopeId,
        candidate_tools: &[ToolCandidate],
        observations: &[ObservationHit],
        active_guideline_names: &[String],
        active_journey_state: Option<&str>,
    ) -> Result<Vec<ToolGateDecision>> {
        let policies = self.store.list_tool_exposure_policies(scope_id)?;
        let tool_control_active = scope_uses_managed_tool_control(&policies);
        let obs_names: Vec<&str> = observations.iter().map(|o| o.name.as_str()).collect();

        let mut decisions = Vec::new();
        for candidate in candidate_tools {
            let matching: Vec<&ToolExposurePolicy> = policies
                .iter()
                .filter(|policy| {
                    tool_policy_preconditions_match(
                        policy,
                        &obs_names,
                        active_guideline_names,
                        active_journey_state,
                    ) && candidate_matches_policy(candidate, policy)
                })
                .collect();

            let decision = if !matching.is_empty() {
                let requires_approval = matching.iter().any(|policy| {
                    matches!(
                        policy.approval_mode,
                        ApprovalMode::Required | ApprovalMode::Conditional
                    )
                });
                ToolGateDecision {
                    tool_name: candidate.tool_name.clone(),
                    allowed: true,
                    reason: matching
                        .iter()
                        .map(|policy| {
                            format!(
                                "{} (approval_mode={})",
                                tool_policy_reason(policy),
                                policy.approval_mode
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("; "),
                    requires_approval,
                }
            } else {
                let (allowed, reason) = if tool_control_active {
                    (
                        false,
                        "scope managed by control plane; no matching tool authorization"
                            .to_string(),
                    )
                } else {
                    (true, "no policy (legacy open by default)".to_string())
                };
                ToolGateDecision {
                    tool_name: candidate.tool_name.clone(),
                    allowed,
                    reason,
                    requires_approval: false,
                }
            };
            decisions.push(decision);
        }
        Ok(decisions)
    }
}

/// Backend-agnostic alias for the default store-backed tool gate.
pub type StoreToolGate = SqliteToolGate;

/// No-op tool gate (allows everything, requires no approval).
#[derive(Debug, Default)]
pub struct NoopToolGate;

#[async_trait]
impl ToolGate for NoopToolGate {
    async fn evaluate(
        &self,
        _scope_id: &ScopeId,
        candidate_tools: &[ToolCandidate],
        _observations: &[ObservationHit],
        _active_guideline_names: &[String],
        _active_journey_state: Option<&str>,
    ) -> Result<Vec<ToolGateDecision>> {
        Ok(candidate_tools
            .iter()
            .map(|t| ToolGateDecision {
                tool_name: t.tool_name.clone(),
                allowed: true,
                reason: "noop gate".to_string(),
                requires_approval: false,
            })
            .collect())
    }
}

// ─── LLM-based semantic matchers ──────────────────────────────────────────────

use openparlant_types::control::ControlLlmCaller;

/// Observation matcher that handles `matcher_type = "semantic"` via an LLM call.
///
/// All other matcher types are delegated to [`StoreObservationMatcher`] (deterministic).
/// Semantic observations are evaluated by asking the LLM whether the condition is met.
pub struct LlmObservationMatcher {
    store: PolicyStore,
    llm: std::sync::Arc<dyn ControlLlmCaller>,
}

impl LlmObservationMatcher {
    pub fn new(db: impl Into<SharedDb>, llm: std::sync::Arc<dyn ControlLlmCaller>) -> Self {
        Self {
            store: PolicyStore::new(db),
            llm,
        }
    }
}

#[async_trait]
impl ObservationMatcher for LlmObservationMatcher {
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
            if obs.matcher_type.to_lowercase() == "semantic" {
                // Ask the LLM whether this observation's condition is met.
                let condition = obs
                    .matcher_config
                    .get("condition")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&obs.name);
                let prompt = format!(
                    "You are evaluating whether a conversational observation condition is currently triggered.\n\
                     \n\
                     User message: \"{msg}\"\n\
                     \n\
                     Condition to evaluate: \"{cond}\"\n\
                     \n\
                     Reply with ONLY a JSON object on one line, no markdown:\n\
                     {{\"triggered\": true, \"rationale\": \"brief reason\"}}\n\
                     or\n\
                     {{\"triggered\": false, \"rationale\": \"brief reason\"}}",
                    msg = message.text,
                    cond = condition,
                );
                match self.llm.call(&prompt).await {
                    Ok(raw) => {
                        // Strip possible markdown fences
                        let json_str = raw
                            .trim()
                            .trim_start_matches("```json")
                            .trim_start_matches("```")
                            .trim_end_matches("```")
                            .trim();
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                            if v.get("triggered")
                                .and_then(|t| t.as_bool())
                                .unwrap_or(false)
                            {
                                let rationale = v
                                    .get("rationale")
                                    .and_then(|r| r.as_str())
                                    .unwrap_or("llm_semantic")
                                    .to_string();
                                hits.push(ObservationHit {
                                    observation_id: obs.observation_id,
                                    name: obs.name.clone(),
                                    confidence: Some(1.0),
                                    matched_by: format!("llm_semantic: {rationale}"),
                                });
                            }
                        } else {
                            tracing::warn!(
                                observation = %obs.name,
                                raw = %raw,
                                "LLM semantic matcher returned invalid JSON; skipping"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            observation = %obs.name,
                            error = %e,
                            "LLM semantic matcher call failed; skipping"
                        );
                    }
                }
            } else if let Some(hit) = match_observation_deterministic(&obs, &message.text) {
                hits.push(hit);
            }
        }

        Ok(hits)
    }
}

/// Policy resolver that uses LLM semantic scoring for guidelines that have no
/// observation `condition_ref` (i.e., they rely on free-form condition descriptions).
///
/// Guidelines with a non-empty `condition_ref` are evaluated deterministically
/// (matched if their referenced observation was hit). Guidelines with an empty
/// `condition_ref` **and** a non-trivial `action_text` are sent to the LLM for
/// semantic evaluation — matching Parlant's behaviour where every guideline's
/// condition can be evaluated by an LLM against the current conversation.
pub struct LlmPolicyResolver {
    store: PolicyStore,
    llm: std::sync::Arc<dyn ControlLlmCaller>,
}

impl LlmPolicyResolver {
    pub fn new(db: impl Into<SharedDb>, llm: std::sync::Arc<dyn ControlLlmCaller>) -> Self {
        Self {
            store: PolicyStore::new(db),
            llm,
        }
    }
}

#[async_trait]
impl PolicyResolver for LlmPolicyResolver {
    async fn resolve_policy(
        &self,
        scope_id: &ScopeId,
        message: &CanonicalMessage,
        observations: &[ObservationHit],
        active_journey_state: Option<&str>,
        candidate_tools: &[ToolCandidate],
    ) -> Result<PolicyResolution> {
        let guidelines = self.store.list_guidelines(scope_id, true)?;

        // Separate into deterministic (have condition_ref) and semantic (empty condition_ref).
        let mut deterministic_active: Vec<GuidelineActivation> = Vec::new();
        let mut semantic_candidates: Vec<&openparlant_types::control::GuidelineDefinition> =
            Vec::new();

        for guideline in &guidelines {
            if !guideline.enabled {
                continue;
            }
            if !guideline.condition_ref.is_empty() && guideline.condition_ref != "always" {
                // Deterministic: active if any matched observation matches condition_ref
                let is_active = observations
                    .iter()
                    .any(|obs| obs.name == guideline.condition_ref);
                if is_active {
                    deterministic_active.push(GuidelineActivation {
                        guideline_id: guideline.guideline_id,
                        name: guideline.name.clone(),
                        action_text: guideline.action_text.clone(),
                        composition_mode: Some(guideline.composition_mode.clone()),
                        priority: guideline.priority,
                        source_observations: observations
                            .iter()
                            .filter(|obs| obs.name == guideline.condition_ref)
                            .map(|obs| obs.observation_id)
                            .collect(),
                    });
                }
            } else if guideline.condition_ref == "always" {
                deterministic_active.push(GuidelineActivation {
                    guideline_id: guideline.guideline_id,
                    name: guideline.name.clone(),
                    action_text: guideline.action_text.clone(),
                    composition_mode: Some(guideline.composition_mode.clone()),
                    priority: guideline.priority,
                    source_observations: Vec::new(),
                });
            } else {
                // Empty condition_ref → candidate for LLM semantic evaluation
                semantic_candidates.push(guideline);
            }
        }

        // Batch-evaluate all semantic candidates in one LLM call (cheaper than N calls).
        let mut llm_active: Vec<GuidelineActivation> = Vec::new();
        if !semantic_candidates.is_empty() {
            let candidates_json: Vec<serde_json::Value> = semantic_candidates
                .iter()
                .map(|g| {
                    serde_json::json!({
                        "id": g.guideline_id.0.to_string(),
                        "condition": g.name,
                        "action": g.action_text
                    })
                })
                .collect();
            let prompt = format!(
                "You are deciding which behavioral guidelines apply to the current conversation turn.\n\
                 \n\
                 User message: \"{msg}\"\n\
                 \n\
                 Guidelines to evaluate (JSON array):\n{candidates}\n\
                 \n\
                 For each guideline determine if it applies given the user message.\n\
                 Reply with ONLY a JSON object on one line, no markdown:\n\
                 {{\"results\": [{{\"id\": \"...\", \"applies\": true, \"score\": 0.9, \"rationale\": \"...\"}}]}}",
                msg = message.text,
                candidates = serde_json::to_string_pretty(&candidates_json).unwrap_or_default(),
            );
            match self.llm.call(&prompt).await {
                Ok(raw) => {
                    let json_str = raw
                        .trim()
                        .trim_start_matches("```json")
                        .trim_start_matches("```")
                        .trim_end_matches("```")
                        .trim();
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                        if let Some(results) = v.get("results").and_then(|r| r.as_array()) {
                            for result in results {
                                let id_str =
                                    result.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                let applies = result
                                    .get("applies")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                if !applies {
                                    continue;
                                }
                                if let Some(g) = semantic_candidates
                                    .iter()
                                    .find(|g| g.guideline_id.0.to_string() == id_str)
                                {
                                    llm_active.push(GuidelineActivation {
                                        guideline_id: g.guideline_id,
                                        name: g.name.clone(),
                                        action_text: g.action_text.clone(),
                                        composition_mode: Some(g.composition_mode.clone()),
                                        priority: g.priority,
                                        source_observations: Vec::new(),
                                    });
                                }
                            }
                        }
                    } else {
                        tracing::warn!(
                            raw = %raw,
                            "LLM policy resolver returned invalid JSON; falling back to empty semantic matches"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "LLM policy resolver call failed; no semantic guidelines activated"
                    );
                }
            }
        }

        let mut active_guidelines = deterministic_active;
        active_guidelines.extend(llm_active);

        // Apply relationship graph (reuse same logic as StorePolicyResolver)
        let relationships = self
            .store
            .list_guideline_relationships(scope_id)
            .unwrap_or_default();

        use std::collections::{HashMap, HashSet};
        let id_to_idx: HashMap<String, usize> = active_guidelines
            .iter()
            .enumerate()
            .map(|(i, g)| (g.guideline_id.0.to_string(), i))
            .collect();

        let mut to_exclude: HashSet<String> = HashSet::new();
        let mut exclusion_reasons: HashMap<String, String> = HashMap::new();

        for (_, from_id, to_id, relation_type) in &relationships {
            match canonical_guideline_relation_type(relation_type).unwrap_or("") {
                "depends_on" => {
                    if id_to_idx.contains_key(from_id) && !id_to_idx.contains_key(to_id) {
                        to_exclude.insert(from_id.clone());
                        exclusion_reasons.insert(
                            from_id.clone(),
                            format!("depends_on {to_id} which is not active"),
                        );
                    }
                }
                "excludes" => {
                    let from_idx = id_to_idx.get(from_id);
                    let to_idx = id_to_idx.get(to_id);
                    if let (Some(&fi), Some(&ti)) = (from_idx, to_idx) {
                        let from_p = active_guidelines[fi].priority;
                        let to_p = active_guidelines[ti].priority;
                        if from_p >= to_p {
                            to_exclude.insert(to_id.clone());
                            exclusion_reasons.insert(
                                to_id.clone(),
                                format!("excluded by {from_id} (priority {from_p} >= {to_p})"),
                            );
                        } else {
                            to_exclude.insert(from_id.clone());
                            exclusion_reasons.insert(
                                from_id.clone(),
                                format!("excluded by {to_id} (priority {to_p} > {from_p})"),
                            );
                        }
                    }
                }
                "prioritizes_over" => {
                    if id_to_idx.contains_key(from_id) && id_to_idx.contains_key(to_id) {
                        to_exclude.insert(to_id.clone());
                        exclusion_reasons
                            .insert(to_id.clone(), format!("prioritized_over by {from_id}"));
                    }
                }
                _ => {}
            }
        }

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

        // Tool authorizations (same as StorePolicyResolver)
        let tool_policies = self
            .store
            .list_tool_exposure_policies(scope_id)
            .unwrap_or_default();
        let tool_control_active = scope_uses_managed_tool_control(&tool_policies);
        let guideline_names: Vec<String> =
            active_guidelines.iter().map(|g| g.name.clone()).collect();
        let tool_authorizations = build_tool_authorizations(
            &tool_policies,
            candidate_tools,
            observations,
            &guideline_names,
            active_journey_state,
        );

        Ok(PolicyResolution {
            active_guidelines,
            excluded_guidelines,
            tool_authorizations,
            tool_control_active,
        })
    }
}

#[cfg(test)]
mod tool_policy_skill_tests {
    use super::*;
    use openparlant_types::control::{ApprovalMode, GuidelineId, ObservationId};

    fn setup_conn() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE guidelines (
                guideline_id TEXT PRIMARY KEY,
                scope_id TEXT NOT NULL,
                name TEXT NOT NULL,
                condition_ref TEXT NOT NULL,
                action_text TEXT NOT NULL,
                composition_mode TEXT NOT NULL,
                priority INTEGER NOT NULL,
                enabled INTEGER NOT NULL
            );
            CREATE TABLE guideline_relationships (
                relationship_id TEXT PRIMARY KEY,
                scope_id TEXT NOT NULL,
                from_guideline_id TEXT NOT NULL,
                to_guideline_id TEXT NOT NULL,
                relation_type TEXT NOT NULL
            );
            CREATE TABLE tool_exposure_policies (
                policy_id TEXT PRIMARY KEY,
                scope_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                skill_ref TEXT,
                observation_ref TEXT,
                journey_state_ref TEXT,
                guideline_ref TEXT,
                approval_mode TEXT NOT NULL,
                enabled INTEGER NOT NULL
            );
            "#,
        )
        .unwrap();
        Arc::new(Mutex::new(conn))
    }

    fn browser_candidates() -> Vec<ToolCandidate> {
        vec![
            ToolCandidate {
                tool_name: "browser_navigate".to_string(),
                skill_ref: Some("browser".to_string()),
            },
            ToolCandidate {
                tool_name: "browser_click".to_string(),
                skill_ref: Some("browser".to_string()),
            },
            ToolCandidate {
                tool_name: "file_read".to_string(),
                skill_ref: None,
            },
        ]
    }

    fn browser_observation() -> ObservationHit {
        ObservationHit {
            observation_id: ObservationId::new(),
            name: "needs_browser".to_string(),
            confidence: Some(1.0),
            matched_by: "test".to_string(),
        }
    }

    fn write_guideline(
        store: &PolicyStore,
        id: GuidelineId,
        name: &str,
        condition_ref: &str,
        action_text: &str,
        priority: i32,
    ) {
        store
            .upsert_guideline(&openparlant_types::control::GuidelineDefinition {
                guideline_id: id,
                scope_id: ScopeId::default(),
                name: name.to_string(),
                condition_ref: condition_ref.to_string(),
                action_text: action_text.to_string(),
                composition_mode: "append".to_string(),
                priority,
                enabled: true,
            })
            .unwrap();
    }

    #[test]
    fn sqlite_policy_resolver_expands_skill_ref_to_candidate_tools() {
        futures::executor::block_on(async {
            let conn = setup_conn();
            let store = PolicyStore::new(conn.clone());
            store
                .upsert_tool_exposure_policy(&ToolExposurePolicy {
                    policy_id: "policy-browser".to_string(),
                    scope_id: ScopeId::default(),
                    tool_name: "*".to_string(),
                    skill_ref: Some("browser".to_string()),
                    observation_ref: Some("needs_browser".to_string()),
                    journey_state_ref: None,
                    guideline_ref: None,
                    approval_mode: ApprovalMode::Required,
                    enabled: true,
                })
                .unwrap();

            let resolver = SqlitePolicyResolver::new(conn);
            let resolution = resolver
                .resolve_policy(
                    &ScopeId::default(),
                    &CanonicalMessage::text(ScopeId::default(), "web", "open the page"),
                    &[browser_observation()],
                    None,
                    &browser_candidates(),
                )
                .await
                .unwrap();

            let allowed: Vec<String> = resolution
                .tool_authorizations
                .iter()
                .map(|auth| auth.tool_name.clone())
                .collect();
            assert_eq!(allowed, vec!["browser_navigate", "browser_click"]);
            assert!(resolution
                .tool_authorizations
                .iter()
                .all(|auth| auth.requires_approval));
        });
    }

    #[test]
    fn sqlite_tool_gate_applies_approval_by_skill_group() {
        futures::executor::block_on(async {
            let conn = setup_conn();
            let store = PolicyStore::new(conn.clone());
            store
                .upsert_tool_exposure_policy(&ToolExposurePolicy {
                    policy_id: "policy-browser".to_string(),
                    scope_id: ScopeId::default(),
                    tool_name: "*".to_string(),
                    skill_ref: Some("browser".to_string()),
                    observation_ref: Some("needs_browser".to_string()),
                    journey_state_ref: None,
                    guideline_ref: None,
                    approval_mode: ApprovalMode::Required,
                    enabled: true,
                })
                .unwrap();

            let gate = SqliteToolGate::new(conn);
            let decisions = gate
                .evaluate(
                    &ScopeId::default(),
                    &browser_candidates()[..2],
                    &[browser_observation()],
                    &[],
                    None,
                )
                .await
                .unwrap();

            assert_eq!(decisions.len(), 2);
            assert!(decisions.iter().all(|decision| decision.allowed));
            assert!(decisions.iter().all(|decision| decision.requires_approval));
        });
    }

    #[test]
    fn sqlite_policy_resolver_marks_scope_as_managed_when_any_tool_policy_exists() {
        futures::executor::block_on(async {
            let conn = setup_conn();
            let store = PolicyStore::new(conn.clone());
            store
                .upsert_tool_exposure_policy(&ToolExposurePolicy {
                    policy_id: "policy-browser".to_string(),
                    scope_id: ScopeId::default(),
                    tool_name: "browser_navigate".to_string(),
                    skill_ref: None,
                    observation_ref: Some("different_observation".to_string()),
                    journey_state_ref: None,
                    guideline_ref: None,
                    approval_mode: ApprovalMode::None,
                    enabled: true,
                })
                .unwrap();

            let resolver = SqlitePolicyResolver::new(conn);
            let resolution = resolver
                .resolve_policy(
                    &ScopeId::default(),
                    &CanonicalMessage::text(ScopeId::default(), "web", "open the page"),
                    &[browser_observation()],
                    None,
                    &browser_candidates(),
                )
                .await
                .unwrap();

            assert!(resolution.tool_control_active);
            assert!(resolution.tool_authorizations.is_empty());
        });
    }

    #[test]
    fn sqlite_tool_gate_keeps_legacy_open_mode_for_unmanaged_scope() {
        futures::executor::block_on(async {
            let conn = setup_conn();
            let gate = SqliteToolGate::new(conn);
            let decisions = gate
                .evaluate(
                    &ScopeId::default(),
                    &browser_candidates(),
                    &[browser_observation()],
                    &[],
                    None,
                )
                .await
                .unwrap();

            assert_eq!(decisions.len(), 3);
            assert!(decisions.iter().all(|decision| decision.allowed));
            assert!(decisions
                .iter()
                .all(|decision| decision.reason.contains("legacy open by default")));
        });
    }

    #[test]
    fn sqlite_tool_gate_denies_unmatched_tools_once_scope_is_managed() {
        futures::executor::block_on(async {
            let conn = setup_conn();
            let store = PolicyStore::new(conn.clone());
            store
                .upsert_tool_exposure_policy(&ToolExposurePolicy {
                    policy_id: "policy-browser".to_string(),
                    scope_id: ScopeId::default(),
                    tool_name: "browser_navigate".to_string(),
                    skill_ref: None,
                    observation_ref: Some("needs_browser".to_string()),
                    journey_state_ref: None,
                    guideline_ref: None,
                    approval_mode: ApprovalMode::None,
                    enabled: true,
                })
                .unwrap();

            let gate = SqliteToolGate::new(conn);
            let decisions = gate
                .evaluate(
                    &ScopeId::default(),
                    &[ToolCandidate {
                        tool_name: "file_read".to_string(),
                        skill_ref: None,
                    }],
                    &[browser_observation()],
                    &[],
                    None,
                )
                .await
                .unwrap();

            assert_eq!(decisions.len(), 1);
            assert!(!decisions[0].allowed);
            assert!(decisions[0]
                .reason
                .contains("scope managed by control plane"));
        });
    }

    #[test]
    fn canonical_relation_type_supports_legacy_control_api_aliases() {
        assert_eq!(
            canonical_guideline_relation_type("requires"),
            Some("depends_on")
        );
        assert_eq!(
            canonical_guideline_relation_type("conflicts_with"),
            Some("excludes")
        );
        assert_eq!(
            canonical_guideline_relation_type("overrides"),
            Some("prioritizes_over")
        );
        assert_eq!(
            canonical_guideline_relation_type("complements"),
            Some("complements")
        );
        assert_eq!(canonical_guideline_relation_type("unknown"), None);
    }

    #[test]
    fn sqlite_policy_resolver_honors_legacy_override_relationship_alias() {
        futures::executor::block_on(async {
            let conn = setup_conn();
            let store = PolicyStore::new(conn.clone());
            let primary_id = GuidelineId::new();
            let secondary_id = GuidelineId::new();

            write_guideline(
                &store,
                primary_id,
                "primary",
                "needs_browser",
                "Use the preferred action.",
                100,
            );
            write_guideline(
                &store,
                secondary_id,
                "secondary",
                "needs_browser",
                "Use the fallback action.",
                50,
            );

            {
                let guard = conn.lock().unwrap();
                guard
                    .execute(
                        "INSERT INTO guideline_relationships
                            (relationship_id, scope_id, from_guideline_id, to_guideline_id, relation_type)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![
                            uuid::Uuid::new_v4().to_string(),
                            ScopeId::default().0,
                            primary_id.0.to_string(),
                            secondary_id.0.to_string(),
                            "overrides",
                        ],
                    )
                    .unwrap();
            }

            let resolver = SqlitePolicyResolver::new(conn);
            let resolution = resolver
                .resolve_policy(
                    &ScopeId::default(),
                    &CanonicalMessage::text(ScopeId::default(), "web", "open the page"),
                    &[browser_observation()],
                    None,
                    &[],
                )
                .await
                .unwrap();

            assert_eq!(resolution.active_guidelines.len(), 1);
            assert_eq!(resolution.active_guidelines[0].name, "primary");
            assert_eq!(resolution.excluded_guidelines.len(), 1);
            assert_eq!(resolution.excluded_guidelines[0].name, "secondary");
        });
    }
}
