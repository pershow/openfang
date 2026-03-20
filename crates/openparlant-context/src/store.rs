use anyhow::Result;
use openparlant_types::control::{
    CannedResponseCandidate, GlossaryEntry, ResolvedVariable, RetrievedChunk, ScopeId,
};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Row, Sqlite};

// ─── Retriever config types ───────────────────────────────────────────────────

/// A retriever definition persisted in the `retrievers` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrieverDefinition {
    pub retriever_id: String,
    pub scope_id: String,
    pub name: String,
    pub retriever_type: String,
    pub config_json: serde_json::Value,
    pub enabled: bool,
}

/// A retriever binding from `retriever_bindings`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrieverBinding {
    pub binding_id: String,
    pub scope_id: String,
    pub retriever_id: String,
    pub bind_type: String,
    pub bind_ref: String,
}

// ─── ContextStore ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ContextStore {
    pool: Pool<Sqlite>,
}

impl ContextStore {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    // ── Retrievers ────────────────────────────────────────────────────────────

    /// Load all enabled retrievers for a scope.
    pub async fn list_retrievers(
        &self,
        scope_id: &ScopeId,
    ) -> Result<Vec<RetrieverDefinition>> {
        let rows = sqlx::query(
            "SELECT retriever_id, scope_id, name, retriever_type, config_json
             FROM retrievers
             WHERE scope_id = ? AND enabled = 1
             ORDER BY name ASC",
        )
        .bind(&scope_id.0)
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let config_str: String = row.try_get("config_json")?;
            out.push(RetrieverDefinition {
                retriever_id: row.try_get("retriever_id")?,
                scope_id: row.try_get("scope_id")?,
                name: row.try_get("name")?,
                retriever_type: row.try_get("retriever_type")?,
                config_json: serde_json::from_str(&config_str).unwrap_or_default(),
                enabled: true,
            });
        }
        Ok(out)
    }

    /// Load retriever bindings for a scope + bind_type (e.g. "guideline" or "journey_state").
    pub async fn list_retriever_bindings(
        &self,
        scope_id: &ScopeId,
        bind_type: &str,
    ) -> Result<Vec<RetrieverBinding>> {
        let rows = sqlx::query(
            "SELECT binding_id, scope_id, retriever_id, bind_type, bind_ref
             FROM retriever_bindings
             WHERE scope_id = ? AND bind_type = ?",
        )
        .bind(&scope_id.0)
        .bind(bind_type)
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(RetrieverBinding {
                binding_id: row.try_get("binding_id")?,
                scope_id: row.try_get("scope_id")?,
                retriever_id: row.try_get("retriever_id")?,
                bind_type: row.try_get("bind_type")?,
                bind_ref: row.try_get("bind_ref")?,
            });
        }
        Ok(out)
    }

    /// Placeholder: actually invoke retrievers and return chunks.
    ///
    /// In Phase 2 this will dispatch on `retriever_type` (faq_sqlite, openai_embeddings, etc.).
    /// For now returns an empty list so the pipeline compiles and runs end-to-end.
    pub async fn run_retrievers(
        &self,
        scope_id: &ScopeId,
        _query: &str,
    ) -> Result<Vec<RetrievedChunk>> {
        let _retrievers = self.list_retrievers(scope_id).await?;
        // TODO Phase 2: dispatch each retriever by type and merge results.
        Ok(Vec::new())
    }

    // ── Glossary ──────────────────────────────────────────────────────────────

    /// Load all active glossary terms for a given scope.
    pub async fn load_glossary_terms(&self, scope_id: &ScopeId) -> Result<Vec<GlossaryEntry>> {
        let rows = sqlx::query(
            "SELECT name, description, synonyms_json
             FROM glossary_terms
             WHERE scope_id = ? AND enabled = 1",
        )
        .bind(&scope_id.0)
        .fetch_all(&self.pool)
        .await?;

        let mut terms = Vec::with_capacity(rows.len());
        for row in rows {
            let synonyms_json: String = row.try_get("synonyms_json")?;
            let synonyms: Vec<String> = serde_json::from_str(&synonyms_json).unwrap_or_default();
            terms.push(GlossaryEntry {
                name: row.try_get("name")?,
                description: row.try_get("description")?,
                synonyms,
            });
        }
        Ok(terms)
    }

    // ── Context variables ─────────────────────────────────────────────────────

    /// Load all active context variables for a given scope.
    pub async fn load_context_variables(
        &self,
        scope_id: &ScopeId,
    ) -> Result<Vec<ResolvedVariable>> {
        let rows = sqlx::query(
            "SELECT name, value_source_type, value_source_config
             FROM context_variables
             WHERE scope_id = ? AND enabled = 1",
        )
        .bind(&scope_id.0)
        .fetch_all(&self.pool)
        .await?;

        let mut variables = Vec::with_capacity(rows.len());
        for row in rows {
            let name: String = row.try_get("name")?;
            let source_type: String = row.try_get("value_source_type")?;
            let config_json: String = row.try_get("value_source_config")?;

            // MVP: only "static" sources are resolved inline; others are deferred.
            let value = if source_type == "static" {
                match serde_json::from_str::<serde_json::Value>(&config_json) {
                    Ok(config) => config
                        .get("value")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    Err(_) => config_json.clone(),
                }
            } else {
                format!("<unresolved:{source_type}>")
            };

            variables.push(ResolvedVariable {
                name,
                value,
                source: source_type,
            });
        }
        Ok(variables)
    }

    // ── Canned responses ──────────────────────────────────────────────────────

    /// Load all active canned responses for a given scope.
    pub async fn load_canned_responses(
        &self,
        scope_id: &ScopeId,
    ) -> Result<Vec<CannedResponseCandidate>> {
        let rows = sqlx::query(
            "SELECT name, template_text
             FROM canned_responses
             WHERE scope_id = ? AND enabled = 1",
        )
        .bind(&scope_id.0)
        .fetch_all(&self.pool)
        .await?;

        let mut candidates = Vec::with_capacity(rows.len());
        for row in rows {
            candidates.push(CannedResponseCandidate {
                name: row.try_get("name")?,
                template_text: row.try_get("template_text")?,
                priority: 0,
            });
        }
        Ok(candidates)
    }
}
