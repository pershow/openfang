//! Journey runtime traits and baseline implementations for the control plane.

mod store;

use anyhow::Result;
use async_trait::async_trait;
use openparlant_memory::db::SharedDb;
use openparlant_types::agent::SessionId;
use openparlant_types::control::{
    CanonicalMessage, GuidelineActivation, GuidelineId, JourneyActivation, ScopeId, TurnOutcome,
};
use serde::{Deserialize, Serialize};
pub use store::JourneyStore;
pub use store::TransitionRecord;

/// Current journey resolution for a turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JourneyResolution {
    pub active_journey: Option<JourneyActivation>,
    /// Guidelines projected from the active journey state.
    ///
    /// Each active journey state can carry a list of `action_text` strings that
    /// are injected into the policy resolution as if they were always-active
    /// guidelines scoped to this journey. This mirrors Parlant's
    /// `journey_guideline_projection` behaviour.
    #[serde(default)]
    pub projected_guidelines: Vec<GuidelineActivation>,
}

/// Optional journey state update emitted after a turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JourneyUpdate {
    pub journey_instance_id: Option<String>,
    pub before_state: Option<String>,
    pub after_state: Option<String>,
    pub handoff_requested: bool,
    pub completed: bool,
}

/// Resolve journey state before the runtime loop runs and update it afterwards.
#[async_trait]
pub trait JourneyRuntime: Send + Sync {
    async fn resolve_journey(
        &self,
        scope_id: &ScopeId,
        session_id: &SessionId,
        message: &CanonicalMessage,
    ) -> Result<JourneyResolution>;

    async fn apply_outcome(
        &self,
        scope_id: &ScopeId,
        outcome: &TurnOutcome,
    ) -> Result<Option<JourneyUpdate>>;
}

// ─── Shared database-backed implementation ───────────────────────────────────

pub struct SqliteJourneyRuntime {
    store: JourneyStore,
}

impl SqliteJourneyRuntime {
    pub fn new(db: impl Into<SharedDb>) -> Self {
        Self {
            store: JourneyStore::new(db),
        }
    }
}

#[async_trait]
impl JourneyRuntime for SqliteJourneyRuntime {
    async fn resolve_journey(
        &self,
        scope_id: &ScopeId,
        session_id: &SessionId,
        message: &CanonicalMessage,
    ) -> Result<JourneyResolution> {
        let active_instances = self
            .store
            .list_active_instances_for_session(&session_id.to_string())
            .await
            .unwrap_or_default();

        if let Some(instance) = active_instances.first() {
            let journey = self
                .store
                .get_journey(&instance.journey_id)
                .await
                .unwrap_or(None);
            if let Some(j) = journey {
                let current_state = self
                    .store
                    .get_state(&instance.current_state_id)
                    .await
                    .unwrap_or(None);
                if let Some(cs) = current_state {
                    // Build projected guidelines from the active state's action texts.
                    let projected_guidelines: Vec<GuidelineActivation> = cs
                        .guideline_actions
                        .iter()
                        .enumerate()
                        .map(|(i, action_text)| GuidelineActivation {
                            // Use a deterministic UUID derived from journey+state+index
                            // so the same projection produces the same IDs across turns.
                            guideline_id: GuidelineId(uuid::Uuid::new_v5(
                                &uuid::Uuid::NAMESPACE_OID,
                                format!(
                                    "journey:{}:state:{}:{}",
                                    j.journey_id, instance.current_state_id, i
                                )
                                .as_bytes(),
                            )),
                            name: format!(
                                "[journey:{}:state:{}] {}",
                                j.name,
                                cs.name,
                                action_text.chars().take(40).collect::<String>()
                            ),
                            action_text: action_text.clone(),
                            composition_mode: None,
                            priority: 50, // mid-level priority
                            source_observations: Vec::new(),
                        })
                        .collect();

                    return Ok(JourneyResolution {
                        active_journey: Some(JourneyActivation {
                            journey_id: j.journey_id,
                            name: j.name,
                            current_state: cs.name,
                            missing_fields: compute_missing_fields(
                                &cs.required_fields,
                                &instance.state_payload,
                            ),
                            allowed_next_actions: self
                                .store
                                .list_transitions_from(&j.journey_id, &instance.current_state_id)
                                .unwrap_or_default()
                                .into_iter()
                                .map(|t| format!("{} -> {}", t.transition_type, t.to_state_id))
                                .collect(),
                        }),
                        projected_guidelines,
                    });
                }
            }
        }

        let journeys = self
            .store
            .list_journeys(scope_id, true)
            .await
            .unwrap_or_default();
        for journey in journeys {
            if !journey_triggered(&journey.trigger_config, &message.text) {
                continue;
            }
            let Some(initial_state) = self
                .store
                .get_first_state_for_journey(&journey.journey_id)
                .await
                .unwrap_or(None)
            else {
                continue;
            };

            let instance_id = uuid::Uuid::new_v4().to_string();
            self.store.upsert_journey_instance(
                &instance_id,
                scope_id,
                &session_id.to_string(),
                &journey.journey_id,
                &initial_state.state_id,
            )?;

            let projected_guidelines: Vec<GuidelineActivation> = initial_state
                .guideline_actions
                .iter()
                .enumerate()
                .map(|(i, action_text)| GuidelineActivation {
                    guideline_id: GuidelineId(uuid::Uuid::new_v5(
                        &uuid::Uuid::NAMESPACE_OID,
                        format!(
                            "journey:{}:state:{}:{}",
                            journey.journey_id, initial_state.state_id, i
                        )
                        .as_bytes(),
                    )),
                    name: format!(
                        "[journey:{}:state:{}] {}",
                        journey.name,
                        initial_state.name,
                        action_text.chars().take(40).collect::<String>()
                    ),
                    action_text: action_text.clone(),
                    composition_mode: None,
                    priority: 50,
                    source_observations: Vec::new(),
                })
                .collect();

            return Ok(JourneyResolution {
                active_journey: Some(JourneyActivation {
                    journey_id: journey.journey_id,
                    name: journey.name,
                    current_state: initial_state.name,
                    missing_fields: initial_state.required_fields.clone(),
                    allowed_next_actions: self
                        .store
                        .list_transitions_from(&journey.journey_id, &initial_state.state_id)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|t| format!("{} -> {}", t.transition_type, t.to_state_id))
                        .collect(),
                }),
                projected_guidelines,
            });
        }

        Ok(JourneyResolution::default())
    }

    async fn apply_outcome(
        &self,
        _scope_id: &ScopeId,
        outcome: &TurnOutcome,
    ) -> Result<Option<JourneyUpdate>> {
        // Find all active journey instances for this session.
        let session_id = outcome.session_id.to_string();
        let instances = self
            .store
            .list_active_instances_for_session(&session_id)
            .await
            .unwrap_or_default();

        if instances.is_empty() {
            return Ok(None);
        }

        // Process the first active instance (MVP: single active journey per session).
        let instance = &instances[0];

        // If handoff was suggested, mark the journey as awaiting_handoff.
        if outcome.handoff_suggested {
            let _ = self
                .store
                .set_instance_status(&instance.journey_instance_id, "awaiting_handoff");
            return Ok(Some(JourneyUpdate {
                journey_instance_id: Some(instance.journey_instance_id.clone()),
                before_state: Some(instance.current_state_id.clone()),
                after_state: None,
                handoff_requested: true,
                completed: false,
            }));
        }

        // Evaluate outgoing transitions from the current state.
        let transitions = self
            .store
            .list_transitions_from(&instance.journey_id, &instance.current_state_id)
            .unwrap_or_default();

        for transition in &transitions {
            let should_fire = evaluate_transition_condition(
                &transition.condition_config,
                &transition.transition_type,
                outcome,
            );
            if !should_fire {
                continue;
            }

            let is_complete = transition.transition_type == "complete";
            if is_complete {
                let _ = self
                    .store
                    .set_instance_status(&instance.journey_instance_id, "completed");
            } else {
                let _ = self
                    .store
                    .advance_instance(&instance.journey_instance_id, &transition.to_state_id);
            }
            return Ok(Some(JourneyUpdate {
                journey_instance_id: Some(instance.journey_instance_id.clone()),
                before_state: Some(instance.current_state_id.clone()),
                after_state: Some(transition.to_state_id.clone()),
                handoff_requested: false,
                completed: is_complete,
            }));
        }

        Ok(None)
    }
}

/// Backend-agnostic alias for the default store-backed journey runtime.
pub type StoreJourneyRuntime = SqliteJourneyRuntime;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn compute_missing_fields(
    required_fields: &[String],
    state_payload: &serde_json::Value,
) -> Vec<String> {
    let obj = state_payload.as_object();
    required_fields
        .iter()
        .filter(|field| {
            obj.and_then(|m| m.get(field.as_str()))
                .map(|v| v.is_null())
                .unwrap_or(true)
        })
        .cloned()
        .collect()
}

fn journey_triggered(trigger_config: &serde_json::Value, text: &str) -> bool {
    let text_lc = text.to_lowercase();
    if trigger_config
        .get("always")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return true;
    }
    if let Some(substring) = trigger_config.get("substring").and_then(|v| v.as_str()) {
        return text_lc.contains(&substring.to_lowercase());
    }
    if let Some(arr) = trigger_config.get("contains").and_then(|v| v.as_array()) {
        let terms: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
            .collect();
        if !terms.is_empty() {
            return terms.iter().any(|term| text_lc.contains(term));
        }
    }
    if let Some(arr) = trigger_config.get("all").and_then(|v| v.as_array()) {
        let terms: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
            .collect();
        if !terms.is_empty() {
            return terms.iter().all(|term| text_lc.contains(term));
        }
    }
    if let Some(pattern) = trigger_config
        .get("pattern")
        .or_else(|| trigger_config.get("regex"))
        .and_then(|v| v.as_str())
    {
        return regex_lite::Regex::new(pattern)
            .map(|re| re.is_match(text))
            .unwrap_or(false);
    }
    trigger_config
        .as_object()
        .map(|obj| obj.is_empty())
        .unwrap_or(true)
}

/// Determine whether a transition should fire given the current turn outcome.
///
/// `condition_config` is a JSON object that can carry:
/// - `"always": true` — always fires.
/// - `"tool_called": "<name>"` — fires if the outcome includes a call to that tool.
/// - `"response_contains": "<text>"` — fires if the response text contains the substring.
/// - `"type": "complete"` — treated as completion transition (no condition required).
fn evaluate_transition_condition(
    condition_config: &serde_json::Value,
    transition_type: &str,
    outcome: &TurnOutcome,
) -> bool {
    // "complete" and "handoff" transitions are handled by the caller; skip here.
    if transition_type == "complete" || transition_type == "handoff" {
        return true;
    }
    // Always-fire shorthand.
    if condition_config.get("always").and_then(|v| v.as_bool()) == Some(true) {
        return true;
    }
    // Tool-called condition.
    if let Some(tool_name) = condition_config.get("tool_called").and_then(|v| v.as_str()) {
        return outcome
            .tool_calls
            .iter()
            .any(|tc| tc.tool_name == tool_name);
    }
    // Response-contains condition.
    if let Some(text) = condition_config
        .get("response_contains")
        .and_then(|v| v.as_str())
    {
        return outcome
            .response_text
            .to_lowercase()
            .contains(&text.to_lowercase());
    }
    // Empty condition config → always fires (permissive default).
    if condition_config.as_object().map(|m| m.is_empty()) == Some(true) {
        return true;
    }
    false
}

// ─── No-op implementation (used during incremental bring-up) ─────────────────

/// Default no-op journey runtime.
#[derive(Debug, Default)]
pub struct NoopJourneyRuntime;

#[async_trait]
impl JourneyRuntime for NoopJourneyRuntime {
    async fn resolve_journey(
        &self,
        _scope_id: &ScopeId,
        _session_id: &SessionId,
        _message: &CanonicalMessage,
    ) -> Result<JourneyResolution> {
        Ok(JourneyResolution::default())
    }

    async fn apply_outcome(
        &self,
        _scope_id: &ScopeId,
        _outcome: &TurnOutcome,
    ) -> Result<Option<JourneyUpdate>> {
        Ok(None)
    }
}
