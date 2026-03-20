//! Journey runtime traits and baseline implementations for the control plane.

mod store;

use anyhow::Result;
use async_trait::async_trait;
use openparlant_types::control::{CanonicalMessage, JourneyActivation, ScopeId, TurnOutcome};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
pub use store::JourneyStore;

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
        _outcome: &TurnOutcome,
    ) -> Result<Option<JourneyUpdate>> {
        Ok(None)
    }
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
