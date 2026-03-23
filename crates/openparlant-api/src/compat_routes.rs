use crate::routes::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use silicrew_skills::clawhub::ClawHubClient;
use silicrew_types::agent::AgentId;
use silicrew_types::approval::ApprovalDecision;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path as StdPath, PathBuf};
use std::sync::Arc;

fn get_agent_workspace(
    state: &AppState,
    id: &str,
) -> Result<(AgentId, PathBuf), (StatusCode, Json<serde_json::Value>)> {
    let agent_id: AgentId = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid agent ID"})),
            ));
        }
    };

    let entry = match state.kernel.registry.get(agent_id) {
        Some(entry) => entry,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent not found"})),
            ));
        }
    };

    let workspace = match entry.manifest.workspace {
        Some(ref workspace) => workspace.clone(),
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Agent has no workspace"})),
            ));
        }
    };

    Ok((agent_id, workspace))
}

fn list_directory_entries(
    root: &StdPath,
    relative_path: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let target = if relative_path.trim().is_empty() || relative_path == "." {
        root.to_path_buf()
    } else {
        silicrew_runtime::workspace_sandbox::resolve_sandbox_path(relative_path, root)?
    };

    if !target.exists() {
        return Ok(Vec::new());
    }

    if !target.is_dir() {
        return Err("Requested path is not a directory".to_string());
    }

    let mut items = Vec::new();
    for entry in std::fs::read_dir(&target).map_err(|e| format!("Failed to read directory: {e}"))? {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {e}"))?;
        let path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|e| format!("Failed to stat entry: {e}"))?;
        let relative = path
            .strip_prefix(root)
            .map_err(|e| format!("Failed to resolve relative path: {e}"))?
            .to_string_lossy()
            .replace('\\', "/");

        items.push(serde_json::json!({
            "name": entry.file_name().to_string_lossy().to_string(),
            "path": relative,
            "is_dir": metadata.is_dir(),
            "size": if metadata.is_file() { metadata.len() } else { 0 },
        }));
    }

    items.sort_by(|a, b| {
        let a_dir = a["is_dir"].as_bool().unwrap_or(false);
        let b_dir = b["is_dir"].as_bool().unwrap_or(false);
        b_dir
            .cmp(&a_dir)
            .then_with(|| a["name"].as_str().cmp(&b["name"].as_str()))
    });

    Ok(items)
}

fn mask_secret(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let tail: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("***{tail}")
}

fn copy_dir_recursive(src: &StdPath, dst: &StdPath) -> Result<u64, String> {
    if !src.exists() {
        return Err("Source path does not exist".to_string());
    }
    std::fs::create_dir_all(dst).map_err(|e| format!("Failed to create destination: {e}"))?;
    let mut copied = 0;
    for entry in
        std::fs::read_dir(src).map_err(|e| format!("Failed to read source directory: {e}"))?
    {
        let entry = entry.map_err(|e| format!("Failed to read source entry: {e}"))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copied += copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create parent: {e}"))?;
            }
            std::fs::copy(&src_path, &dst_path).map_err(|e| format!("Failed to copy file: {e}"))?;
            copied += 1;
        }
    }
    Ok(copied)
}

/// GET /api/skills/browse/list?path=... — compatibility filesystem browser for global skills.
pub async fn skills_browse_list(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let root = state.kernel.config.home_dir.join("skills");
    let _ = std::fs::create_dir_all(&root);
    let path = params.get("path").cloned().unwrap_or_default();

    match list_directory_entries(&root, &path) {
        Ok(items) => Json(serde_json::Value::Array(items)).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": error})),
        )
            .into_response(),
    }
}

/// GET /api/skills/browse/read?path=... — compatibility reader for global skills files.
pub async fn skills_browse_read(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let root = state.kernel.config.home_dir.join("skills");
    let _ = std::fs::create_dir_all(&root);
    let Some(path) = params.get("path") else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Missing 'path' query parameter"})),
        )
            .into_response();
    };

    let resolved = match silicrew_runtime::workspace_sandbox::resolve_sandbox_path(path, &root) {
        Ok(path) => path,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": error})),
            )
                .into_response();
        }
    };

    match std::fs::read_to_string(&resolved) {
        Ok(content) => Json(serde_json::json!({ "content": content })).into_response(),
        Err(error) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Failed to read file: {error}")})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct BrowseWriteRequest {
    pub path: String,
    pub content: String,
}

/// PUT /api/skills/browse/write — compatibility writer for global skills files.
pub async fn skills_browse_write(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BrowseWriteRequest>,
) -> impl IntoResponse {
    let root = state.kernel.config.home_dir.join("skills");
    let _ = std::fs::create_dir_all(&root);

    let resolved =
        match silicrew_runtime::workspace_sandbox::resolve_sandbox_path(&body.path, &root) {
            Ok(path) => path,
            Err(error) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": error})),
                )
                    .into_response();
            }
        };

    if let Some(parent) = resolved.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Failed to create directory: {error}")})),
            )
                .into_response();
        }
    }

    match std::fs::write(&resolved, body.content) {
        Ok(()) => {
            state.kernel.reload_skills();
            Json(serde_json::json!({"status": "ok", "path": body.path})).into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to write file: {error}")})),
        )
            .into_response(),
    }
}

/// DELETE /api/skills/browse/delete?path=... — compatibility delete for global skills files.
pub async fn skills_browse_delete(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let root = state.kernel.config.home_dir.join("skills");
    let _ = std::fs::create_dir_all(&root);
    let Some(path) = params.get("path") else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Missing 'path' query parameter"})),
        )
            .into_response();
    };

    let resolved = match silicrew_runtime::workspace_sandbox::resolve_sandbox_path(path, &root) {
        Ok(path) => path,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": error})),
            )
                .into_response();
        }
    };

    let result = if resolved.is_dir() {
        std::fs::remove_dir_all(&resolved)
    } else {
        std::fs::remove_file(&resolved)
    };

    match result {
        Ok(()) => {
            state.kernel.reload_skills();
            Json(serde_json::json!({"status": "deleted", "path": path})).into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to delete path: {error}")})),
        )
            .into_response(),
    }
}

/// GET /api/skills/settings/token — compatibility endpoint for skill source tokens.
pub async fn skills_settings_token_get(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let github_token = state
        .control_store
        .get_setting("skills_github_token")
        .ok()
        .flatten()
        .and_then(|record| serde_json::from_str::<serde_json::Value>(&record.value_json).ok())
        .and_then(|value| {
            value
                .get("token")
                .and_then(|token| token.as_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_default();
    let clawhub_key = state
        .control_store
        .get_setting("skills_clawhub_key")
        .ok()
        .flatten()
        .and_then(|record| serde_json::from_str::<serde_json::Value>(&record.value_json).ok())
        .and_then(|value| {
            value
                .get("token")
                .and_then(|token| token.as_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_default();

    Json(serde_json::json!({
        "configured": !github_token.is_empty(),
        "source": if github_token.is_empty() { "none" } else { "system_settings" },
        "masked": mask_secret(&github_token),
        "clawhub_configured": !clawhub_key.is_empty(),
        "clawhub_masked": mask_secret(&clawhub_key),
    }))
}

#[derive(Deserialize)]
pub struct SkillsSettingsTokenUpdate {
    pub github_token: Option<String>,
    pub clawhub_key: Option<String>,
}

/// PUT /api/skills/settings/token — compatibility setter for skill source tokens.
pub async fn skills_settings_token_put(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SkillsSettingsTokenUpdate>,
) -> impl IntoResponse {
    if let Some(token) = body.github_token {
        let payload = serde_json::json!({ "token": token }).to_string();
        if let Err(error) = state
            .control_store
            .upsert_setting("skills_github_token", &payload)
        {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": error.to_string()})),
            )
                .into_response();
        }
    }

    if let Some(token) = body.clawhub_key {
        let payload = serde_json::json!({ "token": token }).to_string();
        if let Err(error) = state
            .control_store
            .upsert_setting("skills_clawhub_key", &payload)
        {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": error.to_string()})),
            )
                .into_response();
        }
    }

    Json(serde_json::json!({"status": "ok"})).into_response()
}

#[derive(Deserialize)]
pub struct ImportSkillRequest {
    pub skill_id: String,
}

/// POST /api/agents/{id}/files/import-skill — copy a global skill into an agent workspace.
pub async fn import_skill_to_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<ImportSkillRequest>,
) -> impl IntoResponse {
    let (_, workspace) = match get_agent_workspace(&state, &id) {
        Ok(result) => result,
        Err(error) => return error.into_response(),
    };

    let source = state
        .kernel
        .config
        .home_dir
        .join("skills")
        .join(&body.skill_id);
    if !source.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Skill not found"})),
        )
            .into_response();
    }

    let target = workspace.join("skills").join(&body.skill_id);
    match copy_dir_recursive(&source, &target) {
        Ok(files_written) => Json(serde_json::json!({
            "status": "ok",
            "skill_id": body.skill_id,
            "files_written": files_written,
        }))
        .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": error})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct ImportClawHubRequest {
    pub slug: String,
}

/// POST /api/agents/{id}/files/import-from-clawhub — install a ClawHub skill directly into an agent workspace.
pub async fn import_clawhub_skill_to_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<ImportClawHubRequest>,
) -> impl IntoResponse {
    let (_, workspace) = match get_agent_workspace(&state, &id) {
        Ok(result) => result,
        Err(error) => return error.into_response(),
    };

    let cache_dir = state.kernel.config.home_dir.join(".cache").join("clawhub");
    let client = ClawHubClient::new(cache_dir);
    let target_dir = workspace.join("skills");
    if let Err(error) = std::fs::create_dir_all(&target_dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                serde_json::json!({"error": format!("Failed to create skills directory: {error}")}),
            ),
        )
            .into_response();
    }

    match client.install(&body.slug, &target_dir).await {
        Ok(result) => Json(serde_json::json!({
            "status": "ok",
            "slug": body.slug,
            "name": result.skill_name,
            "version": result.version,
            "warning_count": result.warnings.len(),
            "is_prompt_only": result.is_prompt_only,
        }))
        .into_response(),
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": error.to_string()})),
        )
            .into_response(),
    }
}

/// POST /api/skills/import-from-url/preview — explicit not-yet-supported compatibility endpoint.
pub async fn import_skill_from_url_preview() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(
            serde_json::json!({"detail": "Import from arbitrary URL is not supported by this SiliCrew backend yet."}),
        ),
    )
}

/// POST /api/skills/import-from-url — explicit not-yet-supported compatibility endpoint.
pub async fn import_skill_from_url() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(
            serde_json::json!({"detail": "Import from arbitrary URL is not supported by this SiliCrew backend yet."}),
        ),
    )
}

/// POST /api/agents/{id}/files/import-from-url — explicit not-yet-supported compatibility endpoint.
pub async fn import_skill_from_url_to_agent() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(
            serde_json::json!({"detail": "Import from arbitrary URL is not supported by this SiliCrew backend yet."}),
        ),
    )
}

/// GET /api/agents/{id}/activity — lightweight compatibility projection from audit log.
pub async fn agent_activity(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let limit = params
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(50)
        .min(200);
    let items: Vec<_> = state
        .kernel
        .audit_log
        .recent(500)
        .into_iter()
        .filter(|entry| entry.agent_id == id)
        .take(limit)
        .map(|entry| {
            serde_json::json!({
                "id": entry.seq.to_string(),
                "action_type": format!("{:?}", entry.action).to_lowercase(),
                "summary": entry.detail,
                "detail": { "outcome": entry.outcome },
                "created_at": entry.timestamp,
            })
        })
        .collect();

    Json(items)
}

/// GET /api/agents/{id}/metrics — lightweight compatibility metrics for legacy agent detail cards.
pub async fn agent_metrics(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let pending_approvals = state
        .kernel
        .approval_manager
        .list_pending()
        .into_iter()
        .filter(|approval| approval.agent_id == id)
        .count();
    let actions_last_24h = state
        .kernel
        .audit_log
        .recent(1000)
        .into_iter()
        .filter(|entry| entry.agent_id == id)
        .filter(|entry| {
            chrono::DateTime::parse_from_rfc3339(&entry.timestamp)
                .map(|timestamp| {
                    chrono::Utc::now()
                        .signed_duration_since(timestamp.with_timezone(&chrono::Utc))
                        .num_hours()
                        < 24
                })
                .unwrap_or(false)
        })
        .count();

    Json(serde_json::json!({
        "tasks": { "done": 0, "total": 0, "completion_rate": 0 },
        "approvals": { "pending": pending_approvals },
        "activity": { "actions_last_24h": actions_last_24h },
    }))
}

/// GET /api/agents/{id}/approvals — filter global approvals by agent id.
pub async fn agent_approvals(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let pending = state.kernel.approval_manager.list_pending();
    let recent = state.kernel.approval_manager.list_recent(50);

    let mut approvals: Vec<serde_json::Value> = pending
        .into_iter()
        .filter(|approval| approval.agent_id == id)
        .map(|approval| {
            serde_json::json!({
                "id": approval.id,
                "tool_name": approval.tool_name,
                "description": approval.description,
                "action_summary": approval.action_summary,
                "status": "pending",
                "created_at": approval.requested_at,
            })
        })
        .collect();

    approvals.extend(
        recent
            .into_iter()
            .filter(|record| record.request.agent_id == id)
            .map(|record| {
                let status = match record.decision {
                    ApprovalDecision::Approved => "approved",
                    ApprovalDecision::Denied => "rejected",
                    ApprovalDecision::TimedOut => "expired",
                };
                serde_json::json!({
                    "id": record.request.id,
                    "tool_name": record.request.tool_name,
                    "description": record.request.description,
                    "action_summary": record.request.action_summary,
                    "status": status,
                    "created_at": record.request.requested_at,
                    "decided_at": record.decided_at,
                })
            }),
    );

    Json(approvals)
}

#[derive(Deserialize)]
pub struct ResolveAgentApprovalRequest {
    pub action: String,
}

/// POST /api/agents/{id}/approvals/{approval_id}/resolve — compatibility resolver for legacy agent detail page.
pub async fn resolve_agent_approval(
    State(state): State<Arc<AppState>>,
    Path((_agent_id, approval_id)): Path<(String, String)>,
    Json(body): Json<ResolveAgentApprovalRequest>,
) -> impl IntoResponse {
    let uuid = match uuid::Uuid::parse_str(&approval_id) {
        Ok(uuid) => uuid,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid approval ID"})),
            )
                .into_response();
        }
    };
    let decision = if body.action.eq_ignore_ascii_case("approve") {
        ApprovalDecision::Approved
    } else {
        ApprovalDecision::Denied
    };

    match state
        .kernel
        .approval_manager
        .resolve(uuid, decision, Some("api".to_string()))
    {
        Ok(result) => Json(serde_json::json!({
            "status": body.action,
            "decided_at": result.decided_at.to_rfc3339(),
        }))
        .into_response(),
        Err(error) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": error})),
        )
            .into_response(),
    }
}
