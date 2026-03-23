//! Knowledge and response context compilation traits for the control plane.

mod store;

use anyhow::Result;
use async_trait::async_trait;
use silicrew_memory::db::SharedDb;
use silicrew_types::control::{
    CannedResponseCandidate, CanonicalMessage, ControlEmbedder, GlossaryEntry, JourneyActivation,
    KnowledgeCompileContext, ResolvedVariable, RetrievedChunk, ScopeId,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
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
        active_guideline_names: &[String],
        compile_ctx: &KnowledgeCompileContext,
    ) -> Result<KnowledgeBundle>;
}

// ─── Shared database-backed implementation ───────────────────────────────────

pub struct SqliteKnowledgeCompiler {
    store: ContextStore,
    /// Optional embedder for `"embedding"` retriever type.
    embedder: Option<Arc<dyn ControlEmbedder>>,
}

impl SqliteKnowledgeCompiler {
    pub fn new(db: impl Into<SharedDb>) -> Self {
        Self {
            store: ContextStore::new(db),
            embedder: None,
        }
    }

    /// Attach an embedder to enable vector-based (`"embedding"`) retrieval.
    pub fn with_embedder(mut self, embedder: Arc<dyn ControlEmbedder>) -> Self {
        self.embedder = Some(embedder);
        self
    }
}

#[async_trait]
impl KnowledgeCompiler for SqliteKnowledgeCompiler {
    async fn compile_knowledge(
        &self,
        scope_id: &ScopeId,
        message: &CanonicalMessage,
        active_journey: Option<&JourneyActivation>,
        active_guideline_names: &[String],
        compile_ctx: &KnowledgeCompileContext,
    ) -> Result<KnowledgeBundle> {
        let active_journey_state = active_journey.map(|journey| journey.current_state.as_str());
        let embedder_ref = self.embedder.as_deref();
        let retrieved_chunks = self
            .store
            .run_retrievers(
                scope_id,
                &message.text,
                active_journey_state,
                active_guideline_names,
                embedder_ref,
            )
            .await?;
        let glossary_terms = self
            .store
            .load_glossary_terms_for_turn(scope_id, &message.text)
            .await?;
        let context_variables = self
            .store
            .load_context_variables(
                scope_id,
                &message.text,
                active_journey_state,
                &compile_ctx.agent_id,
                &compile_ctx.session_id,
            )
            .await?;
        let canned_response_candidates = self
            .store
            .load_canned_responses(scope_id, &message.text, active_journey_state)
            .await?;

        Ok(KnowledgeBundle {
            retrieved_chunks,
            glossary_terms,
            context_variables,
            canned_response_candidates,
        })
    }
}

/// Backend-agnostic alias for the default store-backed knowledge compiler.
pub type StoreKnowledgeCompiler = SqliteKnowledgeCompiler;

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
        _active_guideline_names: &[String],
        _compile_ctx: &KnowledgeCompileContext,
    ) -> Result<KnowledgeBundle> {
        Ok(KnowledgeBundle::default())
    }
}
