//! Knowledge and response context compilation traits for the control plane.

mod store;

use anyhow::Result;
use async_trait::async_trait;
use openparlant_types::control::{
    CannedResponseCandidate, CanonicalMessage, GlossaryEntry, JourneyActivation, ResolvedVariable,
    RetrievedChunk, ScopeId,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
pub use store::ContextStore;

/// Compiled knowledge bundle for a turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KnowledgeBundle {
    pub retrieved_chunks: Vec<RetrievedChunk>,
    pub glossary_terms: Vec<GlossaryEntry>,
    pub context_variables: Vec<ResolvedVariable>,
    pub canned_response_candidates: Vec<CannedResponseCandidate>,
}

/// Compile retrievers, glossary, variables, and canned responses for a turn.
#[async_trait]
pub trait KnowledgeCompiler: Send + Sync {
    async fn compile_knowledge(
        &self,
        scope_id: &ScopeId,
        message: &CanonicalMessage,
        active_journey: Option<&JourneyActivation>,
    ) -> Result<KnowledgeBundle>;
}

// ─── SQLite-backed implementation ────────────────────────────────────────────

pub struct SqliteKnowledgeCompiler {
    store: ContextStore,
}

impl SqliteKnowledgeCompiler {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            store: ContextStore::new(pool),
        }
    }
}

#[async_trait]
impl KnowledgeCompiler for SqliteKnowledgeCompiler {
    async fn compile_knowledge(
        &self,
        scope_id: &ScopeId,
        message: &CanonicalMessage,
        _active_journey: Option<&JourneyActivation>,
    ) -> Result<KnowledgeBundle> {
        let retrieved_chunks = self.store.run_retrievers(scope_id, &message.text).await?;
        let glossary_terms = self.store.load_glossary_terms(scope_id).await?;
        let context_variables = self.store.load_context_variables(scope_id).await?;
        let canned_response_candidates = self.store.load_canned_responses(scope_id).await?;

        Ok(KnowledgeBundle {
            retrieved_chunks,
            glossary_terms,
            context_variables,
            canned_response_candidates,
        })
    }
}

// ─── No-op implementation (used during incremental bring-up) ─────────────────

/// Default no-op knowledge compiler.
#[derive(Debug, Default)]
pub struct NoopKnowledgeCompiler;

#[async_trait]
impl KnowledgeCompiler for NoopKnowledgeCompiler {
    async fn compile_knowledge(
        &self,
        _scope_id: &ScopeId,
        _message: &CanonicalMessage,
        _active_journey: Option<&JourneyActivation>,
    ) -> Result<KnowledgeBundle> {
        Ok(KnowledgeBundle::default())
    }
}
