//! Journey runtime traits and baseline implementations for the control plane.

mod store;

use anyhow::Result;
use async_trait::async_trait;
use openparlant_types::control::{CanonicalMessage, JourneyActivation, ScopeId, TurnOutcome};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
pub use store::JourneyStore;
pub use store::TransitionRecord;

/// Current journey resolution for a turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JourneyResolution {
    pub active_journey: Option<JourneyActivation>,
}

/// Optional journey state update emitted after a turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JourneyUpdate {
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
        message: &CanonicalMessage,
    ) -> Result<JourneyResolution>;

    async fn apply_outcome(
        &self,
        scope_id: &ScopeId,
        outcome: &TurnOutcome,
    ) -> Result<Option<JourneyUpdate>>;
}

// ─── SQLite-backed implementation ────────────────────────────────────────────

pub struct SqliteJourneyRuntime {
    store: JourneyStore,
}

impl SqliteJourneyRuntime {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            store: JourneyStore::new(conn),
        }
    }
}

#[async_trait]
impl JourneyRuntime for SqliteJourneyRuntime {
    async fn resolve_journey(
        &self,
        scope_id: &ScopeId,
        _message: &CanonicalMessage,
    ) -> Result<JourneyResolution> {
        let active_instances = self
            .store
            .list_active_instances(scope_id)
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
                    return Ok(JourneyResolution {
                        active_journey: Some(JourneyActivation {
                            journey_id: j.journey_id,
                            name: j.name,
                            current_state: cs.name,
                            missing_fields: Vec::new(),
                            allowed_next_actions: Vec::new(),
                        }),
                    });
                }
            }
        }

        Ok(JourneyResolution::default())
    }

    async fn apply_outcome(
        &self,
        _scope_id: &ScopeId,
        outcome: &TurnOutcome,
    ) -> Result<Option<JourneyUpdate>> {
        // Find all active journey instances for this session.
        let session_id = outcome.scope_id.0.as_str();
        let instances = self
            .store
            .list_active_instances_for_session(session_id)
            .await
            .unwrap_or_default();

        if instances.is_empty() {
            return Ok(None);
        }

        // Process the first active instance (MVP: single active journey per session).
        let (instance_id, _scope_id_str, journey_id, current_state_id) = &instances[0];

        // If handoff was suggested, mark the journey as awaiting_handoff.
        if outcome.handoff_suggested {
            let _ = self.store.set_instance_status(instance_id, "awaiting_handoff");
            return Ok(Some(JourneyUpdate {
                before_state: Some(current_state_id.clone()),
                after_state: None,
                handoff_requested: true,
                completed: false,
            }));
        }

        // Evaluate outgoing transitions from the current state.
        let transitions = self
            .store
            .list_transitions_from(journey_id, current_state_id)
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
                let _ = self.store.set_instance_status(instance_id, "completed");
            } else {
                let _ = self
                    .store
                    .advance_instance(instance_id, &transition.to_state_id);
            }
            return Ok(Some(JourneyUpdate {
                before_state: Some(current_state_id.clone()),
                after_state: Some(transition.to_state_id.clone()),
                handoff_requested: false,
                completed: is_complete,
            }));
        }

        Ok(None)
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

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
        return outcome.tool_calls.iter().any(|tc| tc.tool_name == tool_name);
    }
    // Response-contains condition.
    if let Some(text) = condition_config
        .get("response_contains")
        .and_then(|v| v.as_str())
    {
        return outcome.response_text.to_lowercase().contains(&text.to_lowercase());
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
