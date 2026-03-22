use anyhow::Result;
use openparlant_types::agent::AgentId;
use openparlant_types::control::{
    CannedResponseCandidate, ControlEmbedder, GlossaryEntry, ResolvedVariable, RetrievedChunk,
    ScopeId,
};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Row, Sqlite};
use std::collections::{HashMap, HashSet};

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
    pub async fn list_retrievers(&self, scope_id: &ScopeId) -> Result<Vec<RetrieverDefinition>> {
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
    /// Dispatches on `retriever_type`:
    /// - `"static"`: searches `config_json.items` (array of `{title, content}`) for keyword matches.
    /// - `"faq_sqlite"`: searches `glossary_terms` for keyword matches and returns them as chunks.
    /// - `"embedding"`: computes a query embedding then does cosine similarity against stored chunk
    ///   vectors (`config_json.chunks[].vector`). Requires an `embedder` to be passed.
    /// - Other types: logged and skipped (Phase 2 will add embedding-based retrieval).
    pub async fn run_retrievers(
        &self,
        scope_id: &ScopeId,
        query: &str,
        active_journey_state: Option<&str>,
        active_guideline_names: &[String],
        embedder: Option<&dyn ControlEmbedder>,
    ) -> Result<Vec<RetrievedChunk>> {
        let retrievers = self.list_retrievers(scope_id).await?;
        let all_bindings = sqlx::query(
            "SELECT retriever_id, bind_type, bind_ref
             FROM retriever_bindings
             WHERE scope_id = ?",
        )
        .bind(&scope_id.0)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        let mut bindings_by_retriever: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for row in all_bindings {
            let retriever_id: String = row.try_get("retriever_id")?;
            let bind_type: String = row.try_get("bind_type")?;
            let bind_ref: String = row.try_get("bind_ref")?;
            bindings_by_retriever
                .entry(retriever_id)
                .or_default()
                .push((bind_type, bind_ref));
        }
        let active_guidelines: HashSet<&str> =
            active_guideline_names.iter().map(String::as_str).collect();
        let query_lower = query.to_lowercase();
        let mut chunks = Vec::new();

        for retriever in &retrievers {
            let is_bound = bindings_by_retriever
                .get(&retriever.retriever_id)
                .map(|bindings| !bindings.is_empty())
                .unwrap_or(false);
            if is_bound {
                let matched = bindings_by_retriever
                    .get(&retriever.retriever_id)
                    .map(|bindings| {
                        bindings
                            .iter()
                            .any(|(bind_type, bind_ref)| match bind_type.as_str() {
                                "journey_state" => active_journey_state == Some(bind_ref.as_str()),
                                "guideline" => active_guidelines.contains(bind_ref.as_str()),
                                "scope" => bind_ref == &scope_id.0,
                                "always" => true,
                                _ => false,
                            })
                    })
                    .unwrap_or(false);
                if !matched {
                    continue;
                }
            }

            match retriever.retriever_type.as_str() {
                "static" => {
                    // Expect config_json = { "items": [ { "title": "...", "content": "..." }, ... ] }
                    if let Some(items) = retriever
                        .config_json
                        .get("items")
                        .and_then(|v| v.as_array())
                    {
                        for item in items {
                            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                            let content =
                                item.get("content").and_then(|v| v.as_str()).unwrap_or("");
                            if title.to_lowercase().contains(&query_lower)
                                || content.to_lowercase().contains(&query_lower)
                            {
                                chunks.push(RetrievedChunk {
                                    source: format!("static:{}", retriever.name),
                                    content: if content.is_empty() {
                                        title.to_string()
                                    } else {
                                        format!("{title}: {content}")
                                    },
                                    score: Some(1.0),
                                    metadata: Some(serde_json::json!({ "retriever_id": retriever.retriever_id })),
                                });
                            }
                        }
                    }
                }
                "faq_sqlite" => {
                    // Search glossary_terms for keyword matches (reuse the same DB table).
                    let rows = sqlx::query(
                        "SELECT name, description, synonyms_json
                         FROM glossary_terms
                         WHERE scope_id = ? AND enabled = 1",
                    )
                    .bind(&scope_id.0)
                    .fetch_all(&self.pool)
                    .await
                    .unwrap_or_default();

                    for row in rows {
                        let name: String = row.try_get("name").unwrap_or_default();
                        let description: String = row.try_get("description").unwrap_or_default();
                        let synonyms_json: String =
                            row.try_get("synonyms_json").unwrap_or_default();
                        let synonyms: Vec<String> =
                            serde_json::from_str(&synonyms_json).unwrap_or_default();

                        let hit = name.to_lowercase().contains(&query_lower)
                            || description.to_lowercase().contains(&query_lower)
                            || synonyms
                                .iter()
                                .any(|s| s.to_lowercase().contains(&query_lower));

                        if hit {
                            chunks.push(RetrievedChunk {
                                source: format!("faq_sqlite:{}", retriever.name),
                                content: format!("{name}: {description}"),
                                score: Some(0.9),
                                metadata: Some(
                                    serde_json::json!({ "retriever_id": retriever.retriever_id }),
                                ),
                            });
                        }
                    }
                }
                "embedding" => {
                    let Some(emb) = embedder else {
                        tracing::debug!(
                            retriever = %retriever.name,
                            "embedding retriever requires embedder — skipping"
                        );
                        continue;
                    };
                    let query_vec = match emb.embed(query).await {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!(
                                retriever = %retriever.name,
                                error = %e,
                                "embedding query failed — skipping retriever"
                            );
                            continue;
                        }
                    };
                    let threshold: f32 = retriever
                        .config_json
                        .get("threshold")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.7) as f32;
                    if let Some(stored_chunks) = retriever
                        .config_json
                        .get("chunks")
                        .and_then(|v| v.as_array())
                    {
                        let mut emb_hits: Vec<RetrievedChunk> = Vec::new();
                        for chunk_val in stored_chunks {
                            let text = chunk_val.get("text").and_then(|v| v.as_str()).unwrap_or("");
                            if let Some(arr) = chunk_val.get("vector").and_then(|v| v.as_array()) {
                                let stored_vec: Vec<f32> = arr
                                    .iter()
                                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                                    .collect();
                                let score = cosine_similarity(&query_vec, &stored_vec);
                                if score >= threshold {
                                    emb_hits.push(RetrievedChunk {
                                        source: format!("embedding:{}", retriever.name),
                                        content: text.to_string(),
                                        score: Some(score),
                                        metadata: Some(serde_json::json!({
                                            "retriever_id": retriever.retriever_id
                                        })),
                                    });
                                }
                            }
                        }
                        // Sort by score descending (best first)
                        emb_hits.sort_by(|a, b| {
                            b.score
                                .unwrap_or(0.0_f32)
                                .partial_cmp(&a.score.unwrap_or(0.0_f32))
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                        chunks.extend(emb_hits);
                    }
                }

                other => {
                    tracing::debug!(
                        retriever_type = %other,
                        retriever = %retriever.name,
                        "unsupported retriever type — skipping (Phase 2 will add more types)"
                    );
                }
            }
        }

        Ok(chunks)
    }

    /// Select glossary terms for this turn: pinned (`always_include`) plus top matches vs. user text.
    ///
    /// Avoids injecting the entire scope glossary on every turn (token cost + noise). Terms with
    /// no keyword overlap and not pinned are omitted.
    pub async fn load_glossary_terms_for_turn(
        &self,
        scope_id: &ScopeId,
        message_text: &str,
    ) -> Result<Vec<GlossaryEntry>> {
        const MAX_TERMS: usize = 48;
        let rows = sqlx::query(
            "SELECT name, description, synonyms_json, COALESCE(always_include, 0) as always_include
             FROM glossary_terms
             WHERE scope_id = ? AND enabled = 1
             ORDER BY name ASC",
        )
        .bind(&scope_id.0)
        .fetch_all(&self.pool)
        .await?;

        let message_lc = message_text.to_lowercase();
        let mut pinned: Vec<GlossaryEntry> = Vec::new();
        let mut scored: Vec<(i32, GlossaryEntry)> = Vec::new();

        for row in rows {
            let name: String = row.try_get("name")?;
            let description: String = row.try_get("description")?;
            let synonyms_json: String = row.try_get("synonyms_json")?;
            let always_include: i64 = row.try_get("always_include")?;
            let synonyms: Vec<String> = serde_json::from_str(&synonyms_json).unwrap_or_default();
            let entry = GlossaryEntry {
                name: name.clone(),
                description,
                synonyms,
            };
            if always_include != 0 {
                pinned.push(entry);
                continue;
            }
            let score = glossary_relevance_score(&message_lc, &name, &entry.description, &entry.synonyms);
            if score > 0 {
                scored.push((score, entry));
            }
        }

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        let mut out: Vec<GlossaryEntry> = pinned;
        for (_, e) in scored {
            if out.len() >= MAX_TERMS {
                break;
            }
            if out.iter().any(|x| x.name == e.name) {
                continue;
            }
            out.push(e);
        }
        Ok(out)
    }

    // ── Context variables ─────────────────────────────────────────────────────

    /// Load active context variables for a given scope (visibility-filtered).
    ///
    /// Supported `value_source_type` values:
    /// - `static` / `literal` — `value_source_config` JSON `{"value":"..."}`.
    /// - `agent_kv` — `{"key":"..."}` reads `kv_store` for this agent (same SQLite DB).
    /// - `disabled` / `noop` — skipped (soft-delete / off-switch without deleting the row).
    /// - Other types — placeholder until wired (`<unresolved:...>`).
    pub async fn load_context_variables(
        &self,
        scope_id: &ScopeId,
        message_text: &str,
        active_journey_state: Option<&str>,
        agent_id: &AgentId,
    ) -> Result<Vec<ResolvedVariable>> {
        let rows = sqlx::query(
            "SELECT name, value_source_type, value_source_config, visibility_rule
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
            let visibility_rule: Option<String> = row.try_get("visibility_rule").ok();

            if !matches_text_rule(
                visibility_rule.as_deref(),
                message_text,
                active_journey_state,
            ) {
                continue;
            }

            let st = source_type.as_str();
            if matches!(st, "disabled" | "noop") {
                continue;
            }

            let value = match st {
                "static" | "literal" => match serde_json::from_str::<serde_json::Value>(&config_json) {
                    Ok(config) => config
                        .get("value")
                        .map(|v| {
                            v.as_str()
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| v.to_string())
                        })
                        .unwrap_or_default(),
                    Err(_) => config_json.clone(),
                },
                "agent_kv" => {
                    let key = serde_json::from_str::<serde_json::Value>(&config_json)
                        .ok()
                        .and_then(|c| c.get("key").and_then(|v| v.as_str()).map(|s| s.to_string()))
                        .unwrap_or_default();
                    if key.is_empty() {
                        "<agent_kv: missing key in config>".to_string()
                    } else {
                        let aid = agent_id.0.to_string();
                        match sqlx::query("SELECT value FROM kv_store WHERE agent_id = ? AND key = ?")
                            .bind(&aid)
                            .bind(&key)
                            .fetch_optional(&self.pool)
                            .await
                        {
                            Ok(Some(r)) => {
                                let blob: Vec<u8> = r.try_get("value").unwrap_or_default();
                                serde_json::from_slice::<serde_json::Value>(&blob)
                                    .map(|v| match v {
                                        serde_json::Value::String(s) => s,
                                        _ => v.to_string(),
                                    })
                                    .unwrap_or_else(|_| {
                                        String::from_utf8_lossy(&blob).into_owned()
                                    })
                            }
                            Ok(None) => format!("<kv_store:{key}: not set>"),
                            Err(e) => format!("<kv_store read error: {e}>"),
                        }
                    }
                }
                other => format!("<unresolved:{other}>"),
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
        message_text: &str,
        active_journey_state: Option<&str>,
    ) -> Result<Vec<CannedResponseCandidate>> {
        let rows = sqlx::query(
            "SELECT name, template_text, trigger_rule, COALESCE(priority, 0) AS priority
             FROM canned_responses
             WHERE scope_id = ? AND enabled = 1",
        )
        .bind(&scope_id.0)
        .fetch_all(&self.pool)
        .await?;

        let mut candidates = Vec::with_capacity(rows.len());
        for row in rows {
            let trigger_rule: Option<String> = row.try_get("trigger_rule").ok();
            if !matches_text_rule(trigger_rule.as_deref(), message_text, active_journey_state) {
                continue;
            }
            candidates.push(CannedResponseCandidate {
                name: row.try_get("name")?,
                template_text: row.try_get("template_text")?,
                priority: row.try_get("priority").unwrap_or(0),
            });
        }
        Ok(candidates)
    }
}

fn glossary_relevance_score(
    message_lc: &str,
    name: &str,
    description: &str,
    synonyms: &[String],
) -> i32 {
    let mut s = 0i32;
    let name_lc = name.to_lowercase();
    if !name_lc.is_empty() && message_lc.contains(&name_lc) {
        s += 100;
    }
    for part in name_lc.split(|c: char| !c.is_alphanumeric()).filter(|p| p.len() > 1) {
        if message_lc.contains(part) {
            s += 40;
        }
    }
    let desc_lc = description.to_lowercase();
    for tok in message_lc.split_whitespace() {
        if tok.len() < 3 {
            continue;
        }
        if desc_lc.contains(tok) {
            s += 15;
        }
    }
    for syn in synonyms {
        let sl = syn.to_lowercase();
        if sl.len() > 1 && message_lc.contains(&sl) {
            s += 35;
        }
    }
    s
}

fn matches_text_rule(
    rule: Option<&str>,
    message_text: &str,
    active_journey_state: Option<&str>,
) -> bool {
    let Some(rule) = rule.map(str::trim).filter(|rule| !rule.is_empty()) else {
        return true;
    };
    if rule.eq_ignore_ascii_case("always") {
        return true;
    }

    let text_lc = message_text.to_lowercase();

    if let Some(value) = rule.strip_prefix("contains:") {
        return text_lc.contains(&value.trim().to_lowercase());
    }
    if let Some(value) = rule.strip_prefix("journey_state:") {
        return active_journey_state
            .map(|state| state.eq_ignore_ascii_case(value.trim()))
            .unwrap_or(false);
    }
    if let Some(pattern) = rule.strip_prefix("regex:") {
        return regex_lite::Regex::new(pattern.trim())
            .map(|re| re.is_match(message_text))
            .unwrap_or(false);
    }

    text_lc.contains(&rule.to_lowercase())
}

// ─── Vector math ──────────────────────────────────────────────────────────────

/// Compute the cosine similarity between two equal-length f32 vectors.
///
/// Returns `0.0` when either vector is zero-length, empty, or the lengths differ.
/// The result is clamped to `[-1.0, 1.0]`.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }
    (dot / (mag_a * mag_b)).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::cosine_similarity;

    #[test]
    fn identical_vectors_give_score_one() {
        let v = vec![0.5, 0.5, 0.7071];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn orthogonal_vectors_give_score_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((cosine_similarity(&a, &b)).abs() < 1e-5);
    }

    #[test]
    fn different_lengths_give_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![1.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }
}
