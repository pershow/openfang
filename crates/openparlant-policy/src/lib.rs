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
