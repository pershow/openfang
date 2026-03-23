use crate::routes::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use openparlant_control::{DashboardTenant, DashboardUser, InvitationCodeRecord};
use openparlant_types::error::SiliCrewError;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

type ApiJson = (StatusCode, Json<serde_json::Value>);

const FEISHU_SYNC_KEY: &str = "feishu_org_sync";
const ORG_DIRECTORY_SNAPSHOT_KEY: &str = "org_directory_snapshot";

fn api_error(status: StatusCode, detail: impl Into<String>) -> ApiJson {
    (status, Json(serde_json::json!({ "detail": detail.into() })))
}

pub(crate) fn auth_secret(state: &AppState) -> String {
    let api_key = state.kernel.config.api_key.trim().to_string();
    if !api_key.is_empty() {
        api_key
    } else {
        state.kernel.config.auth.password_hash.clone()
    }
}

fn session_token_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|v| v.to_string())
        .or_else(|| {
            headers
                .get("cookie")
                .and_then(|v| v.to_str().ok())
                .and_then(|cookies| {
                    cookies.split(';').find_map(|c| {
                        c.trim()
                            .strip_prefix("openparlant_session=")
                            .map(|v| v.to_string())
                    })
                })
        })
}

pub(crate) fn current_dashboard_user(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<DashboardUser, ApiJson> {
    let token = session_token_from_headers(headers)
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "Missing session token"))?;

    let username = if !state.kernel.config.api_key.trim().is_empty()
        && token == state.kernel.config.api_key.trim()
    {
        state.kernel.config.auth.username.clone()
    } else {
        crate::session_auth::verify_session_token(&token, &auth_secret(state))
            .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "Invalid session token"))?
    };

    state
        .control_store
        .get_user_by_username(&username)
        .map_err(internal_error)?
        .ok_or_else(|| api_error(StatusCode::UNAUTHORIZED, "User not found"))
}

pub(crate) fn require_roles(user: &DashboardUser, allowed: &[&str]) -> Result<(), ApiJson> {
    if allowed.iter().any(|role| *role == user.role) {
        Ok(())
    } else {
        Err(api_error(StatusCode::FORBIDDEN, "Permission denied"))
    }
}

pub(crate) fn internal_error(error: SiliCrewError) -> ApiJson {
    api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn user_json(user: &DashboardUser) -> serde_json::Value {
    serde_json::json!({
        "id": user.user_id,
        "username": user.username,
        "email": user.email,
        "display_name": user.display_name,
        "role": user.role,
        "tenant_id": user.tenant_id,
        "is_active": user.is_active,
        "created_at": user.created_at.to_rfc3339(),
    })
}

fn tenant_json(tenant: &DashboardTenant) -> serde_json::Value {
    serde_json::json!({
        "id": tenant.tenant_id,
        "name": tenant.name,
        "slug": tenant.slug,
        "im_provider": tenant.im_provider,
        "timezone": tenant.timezone,
        "is_active": tenant.is_active,
        "created_at": tenant.created_at.to_rfc3339(),
    })
}

fn default_dashboard_tenant(name: &str, slug: String) -> DashboardTenant {
    DashboardTenant {
        tenant_id: uuid::Uuid::new_v4().to_string(),
        name: name.trim().to_string(),
        slug,
        im_provider: "web_only".to_string(),
        timezone: "UTC".to_string(),
        is_active: true,
        created_at: chrono::Utc::now(),
        default_message_limit: 50,
        default_message_period: "permanent".to_string(),
        default_max_agents: 2,
        default_agent_ttl_hours: 48,
        default_max_llm_calls_per_day: 100,
        min_heartbeat_interval_minutes: 120,
        default_max_triggers: 20,
        min_poll_interval_floor: 5,
        max_webhook_rate_ceiling: 5,
    }
}

pub(crate) fn resolve_tenant_scope(
    user: &DashboardUser,
    query_tenant_id: Option<&str>,
) -> Result<String, ApiJson> {
    if user.role == "platform_admin" {
        query_tenant_id
            .map(|value| value.to_string())
            .or_else(|| user.tenant_id.clone())
            .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "Tenant context is required"))
    } else {
        user.tenant_id
            .clone()
            .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "Tenant context is required"))
    }
}

pub(crate) fn agent_tenant_id(entry: &openparlant_types::agent::AgentEntry) -> Option<String> {
    entry
        .manifest
        .metadata
        .get("tenant_id")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .or_else(|| {
            entry
                .manifest
                .metadata
                .get("control_scope_id")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
        })
}

fn tenant_llm_models_key(tenant_id: &str) -> String {
    format!("tenant_llm_models_{tenant_id}")
}

fn load_tenant_llm_models(
    state: &AppState,
    tenant_id: &str,
) -> Result<Vec<serde_json::Value>, ApiJson> {
    match state
        .control_store
        .get_setting(&tenant_llm_models_key(tenant_id))
    {
        Ok(Some(setting)) => Ok(serde_json::from_str::<Vec<serde_json::Value>>(
            &setting.value_json,
        )
        .unwrap_or_default()),
        Ok(None) => Ok(Vec::new()),
        Err(error) => Err(internal_error(error)),
    }
}

fn save_tenant_llm_models(
    state: &AppState,
    tenant_id: &str,
    models: &[serde_json::Value],
) -> Result<(), ApiJson> {
    state
        .control_store
        .upsert_setting(
            &tenant_llm_models_key(tenant_id),
            &serde_json::to_string(models).unwrap_or_else(|_| "[]".to_string()),
        )
        .map_err(internal_error)?;
    Ok(())
}

fn provider_protocol(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "anthropic",
        "gemini" => "gemini",
        _ => "openai_compatible",
    }
}

fn provider_supports_tool_choice(provider: &str) -> bool {
    !matches!(provider, "anthropic" | "baidu")
}

fn tenant_setting_key(base_key: &str, tenant_id: &str) -> String {
    format!("{base_key}::{tenant_id}")
}

fn feishu_sync_setting_key(tenant_id: &str) -> String {
    tenant_setting_key(FEISHU_SYNC_KEY, tenant_id)
}

fn org_directory_snapshot_key(tenant_id: &str) -> String {
    tenant_setting_key(ORG_DIRECTORY_SNAPSHOT_KEY, tenant_id)
}

fn scoped_setting_storage_key(
    user: &DashboardUser,
    key: &str,
    query_tenant_id: Option<&str>,
) -> Result<String, ApiJson> {
    match key {
        FEISHU_SYNC_KEY => Ok(feishu_sync_setting_key(&resolve_tenant_scope(
            user,
            query_tenant_id,
        )?)),
        _ => Ok(key.to_string()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct FeishuOrgSyncConfig {
    #[serde(default)]
    app_id: String,
    #[serde(default)]
    app_secret: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    last_synced_at: Option<String>,
    #[serde(default)]
    last_error: Option<String>,
    #[serde(default)]
    last_departments: usize,
    #[serde(default)]
    last_members: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct OrgDirectorySnapshot {
    #[serde(default)]
    provider: String,
    #[serde(default)]
    tenant_id: String,
    #[serde(default)]
    synced_at: String,
    #[serde(default)]
    departments: Vec<OrgDepartment>,
    #[serde(default)]
    members: Vec<OrgMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct OrgDepartment {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    member_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct OrgMember {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    department_id: Option<String>,
    #[serde(default)]
    department_ids: Vec<String>,
    #[serde(default)]
    department_path: Option<String>,
    #[serde(default)]
    feishu_open_id: Option<String>,
}

fn feishu_api_base_url(config: &FeishuOrgSyncConfig) -> String {
    let base_url = config.base_url.trim();
    if base_url.is_empty() {
        "https://open.feishu.cn".to_string()
    } else {
        base_url.trim_end_matches('/').to_string()
    }
}

fn sanitize_feishu_sync_config(config: &FeishuOrgSyncConfig) -> serde_json::Value {
    serde_json::json!({
        "app_id": config.app_id,
        "base_url": if config.base_url.trim().is_empty() { serde_json::Value::Null } else { serde_json::json!(config.base_url) },
        "app_secret_configured": !config.app_secret.trim().is_empty(),
        "last_synced_at": config.last_synced_at,
        "last_error": config.last_error,
        "last_departments": config.last_departments,
        "last_members": config.last_members,
    })
}

fn load_feishu_sync_config(
    state: &AppState,
    tenant_id: &str,
) -> Result<Option<FeishuOrgSyncConfig>, ApiJson> {
    match state
        .control_store
        .get_setting(&feishu_sync_setting_key(tenant_id))
    {
        Ok(Some(setting)) => Ok(
            serde_json::from_str::<FeishuOrgSyncConfig>(&setting.value_json)
                .ok()
                .filter(|config| {
                    !config.app_id.trim().is_empty() || !config.app_secret.trim().is_empty()
                }),
        ),
        Ok(None) => Ok(None),
        Err(error) => Err(internal_error(error)),
    }
}

fn save_feishu_sync_config(
    state: &AppState,
    tenant_id: &str,
    config: &FeishuOrgSyncConfig,
) -> Result<(), ApiJson> {
    state
        .control_store
        .upsert_setting(
            &feishu_sync_setting_key(tenant_id),
            &serde_json::to_string(config).unwrap_or_else(|_| "{}".to_string()),
        )
        .map_err(internal_error)?;
    Ok(())
}

fn load_org_directory_snapshot(
    state: &AppState,
    tenant_id: &str,
) -> Result<Option<OrgDirectorySnapshot>, ApiJson> {
    match state
        .control_store
        .get_setting(&org_directory_snapshot_key(tenant_id))
    {
        Ok(Some(setting)) => {
            Ok(serde_json::from_str::<OrgDirectorySnapshot>(&setting.value_json).ok())
        }
        Ok(None) => Ok(None),
        Err(error) => Err(internal_error(error)),
    }
}

fn save_org_directory_snapshot(
    state: &AppState,
    tenant_id: &str,
    snapshot: &OrgDirectorySnapshot,
) -> Result<(), ApiJson> {
    state
        .control_store
        .upsert_setting(
            &org_directory_snapshot_key(tenant_id),
            &serde_json::to_string(snapshot).unwrap_or_else(|_| "{}".to_string()),
        )
        .map_err(internal_error)?;
    Ok(())
}

fn build_department_path(
    department_id: &str,
    departments_by_id: &HashMap<String, OrgDepartment>,
) -> Option<String> {
    let mut segments = Vec::new();
    let mut current = Some(department_id.to_string());
    let mut visited = HashSet::new();

    while let Some(id) = current {
        if !visited.insert(id.clone()) {
            break;
        }
        let Some(department) = departments_by_id.get(&id) else {
            break;
        };
        if !department.name.trim().is_empty() {
            segments.push(department.name.clone());
        }
        current = department.parent_id.clone();
    }

    if segments.is_empty() {
        None
    } else {
        segments.reverse();
        Some(segments.join(" / "))
    }
}

fn department_descendants(departments: &[OrgDepartment], department_id: &str) -> HashSet<String> {
    let mut children_by_parent: HashMap<Option<String>, Vec<String>> = HashMap::new();
    for department in departments {
        children_by_parent
            .entry(department.parent_id.clone())
            .or_default()
            .push(department.id.clone());
    }

    let mut descendants = HashSet::new();
    let mut queue = VecDeque::from([department_id.to_string()]);
    while let Some(id) = queue.pop_front() {
        if !descendants.insert(id.clone()) {
            continue;
        }
        if let Some(children) = children_by_parent.get(&Some(id)) {
            for child in children {
                queue.push_back(child.clone());
            }
        }
    }
    descendants
}

fn org_member_json(member: &OrgMember) -> serde_json::Value {
    serde_json::json!({
        "id": member.id,
        "name": member.name,
        "email": member.email,
        "title": member.title,
        "department_id": member.department_id,
        "department_ids": member.department_ids,
        "department_path": member.department_path,
        "feishu_open_id": member.feishu_open_id,
    })
}

async fn feishu_request_json(
    _client: &reqwest::Client,
    request: reqwest::RequestBuilder,
) -> Result<serde_json::Value, String> {
    let response = request
        .send()
        .await
        .map_err(|error| format!("Feishu request failed: {error}"))?;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .map_err(|error| format!("Failed to parse Feishu response: {error}"))?;
    if !status.is_success() {
        let message = body
            .get("msg")
            .and_then(|value| value.as_str())
            .unwrap_or("unexpected error");
        return Err(format!("Feishu API returned {status}: {message}"));
    }
    let code = body
        .get("code")
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    if code != 0 {
        let message = body
            .get("msg")
            .and_then(|value| value.as_str())
            .unwrap_or("unexpected error");
        return Err(format!("Feishu API error {code}: {message}"));
    }
    Ok(body)
}

async fn feishu_tenant_access_token(config: &FeishuOrgSyncConfig) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("Failed to create Feishu client: {error}"))?;
    let body = feishu_request_json(
        &client,
        client
            .post(format!(
                "{}/open-apis/auth/v3/tenant_access_token/internal",
                feishu_api_base_url(config)
            ))
            .json(&serde_json::json!({
                "app_id": config.app_id,
                "app_secret": config.app_secret,
            })),
    )
    .await?;

    body.get("tenant_access_token")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Feishu auth succeeded but tenant_access_token is missing".to_string())
}

async fn feishu_list_department_children(
    client: &reqwest::Client,
    base_url: &str,
    tenant_access_token: &str,
    department_id: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let mut items = Vec::new();
    let mut page_token: Option<String> = None;

    loop {
        let mut query = vec![
            (
                "department_id_type".to_string(),
                "open_department_id".to_string(),
            ),
            ("user_id_type".to_string(), "open_id".to_string()),
            ("page_size".to_string(), "50".to_string()),
            ("fetch_child".to_string(), "false".to_string()),
        ];
        if let Some(token) = page_token.clone() {
            query.push(("page_token".to_string(), token));
        }

        let body = feishu_request_json(
            client,
            client
                .get(format!(
                    "{base_url}/open-apis/contact/v3/departments/{department_id}/children"
                ))
                .bearer_auth(tenant_access_token)
                .query(&query),
        )
        .await?;

        let data = body
            .get("data")
            .and_then(|value| value.as_object())
            .ok_or_else(|| "Feishu departments response is missing data".to_string())?;

        if let Some(page_items) = data.get("items").and_then(|value| value.as_array()) {
            items.extend(page_items.iter().cloned());
        }

        if !data
            .get("has_more")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            break;
        }

        page_token = data
            .get("page_token")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        if page_token.is_none() {
            break;
        }
    }

    Ok(items)
}

async fn feishu_list_users_by_department(
    client: &reqwest::Client,
    base_url: &str,
    tenant_access_token: &str,
    department_id: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let mut items = Vec::new();
    let mut page_token: Option<String> = None;

    loop {
        let mut query = vec![
            ("department_id".to_string(), department_id.to_string()),
            (
                "department_id_type".to_string(),
                "open_department_id".to_string(),
            ),
            ("user_id_type".to_string(), "open_id".to_string()),
            ("page_size".to_string(), "50".to_string()),
        ];
        if let Some(token) = page_token.clone() {
            query.push(("page_token".to_string(), token));
        }

        let body = feishu_request_json(
            client,
            client
                .get(format!(
                    "{base_url}/open-apis/contact/v3/users/find_by_department"
                ))
                .bearer_auth(tenant_access_token)
                .query(&query),
        )
        .await?;

        let data = body
            .get("data")
            .and_then(|value| value.as_object())
            .ok_or_else(|| "Feishu users response is missing data".to_string())?;

        if let Some(page_items) = data.get("items").and_then(|value| value.as_array()) {
            items.extend(page_items.iter().cloned());
        }

        if !data
            .get("has_more")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            break;
        }

        page_token = data
            .get("page_token")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        if page_token.is_none() {
            break;
        }
    }

    Ok(items)
}

async fn sync_feishu_org_snapshot(
    tenant_id: &str,
    config: &FeishuOrgSyncConfig,
) -> Result<OrgDirectorySnapshot, String> {
    if config.app_id.trim().is_empty() || config.app_secret.trim().is_empty() {
        return Err("Feishu sync requires both app_id and app_secret".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("Failed to create Feishu client: {error}"))?;
    let base_url = feishu_api_base_url(config);
    let tenant_access_token = feishu_tenant_access_token(config).await?;

    let mut departments = Vec::<OrgDepartment>::new();
    let mut seen_departments = HashSet::new();
    let mut queue = VecDeque::from(["0".to_string()]);

    while let Some(parent_department_id) = queue.pop_front() {
        let children = feishu_list_department_children(
            &client,
            &base_url,
            &tenant_access_token,
            &parent_department_id,
        )
        .await?;

        for child in children {
            let item = child.get("department").unwrap_or(&child);
            let department_id = item
                .get("open_department_id")
                .or_else(|| item.get("department_id"))
                .or_else(|| item.get("id"))
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if department_id.is_empty() || !seen_departments.insert(department_id.clone()) {
                continue;
            }

            let name = item
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let raw_parent_id = item
                .get("parent_department_id")
                .or_else(|| item.get("parent_id"))
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
                .unwrap_or_else(|| parent_department_id.clone());
            let parent_id = if raw_parent_id == "0" || raw_parent_id.is_empty() {
                None
            } else {
                Some(raw_parent_id)
            };

            departments.push(OrgDepartment {
                id: department_id.clone(),
                name,
                parent_id,
                member_count: 0,
            });
            queue.push_back(department_id);
        }
    }

    let departments_by_id = departments
        .iter()
        .cloned()
        .map(|department| (department.id.clone(), department))
        .collect::<HashMap<_, _>>();
    let mut members_by_id = HashMap::<String, OrgMember>::new();
    let mut members_by_department = HashMap::<String, HashSet<String>>::new();
    let mut department_ids_to_scan = vec!["0".to_string()];
    department_ids_to_scan.extend(departments_by_id.keys().cloned());

    for department_id in department_ids_to_scan {
        let users = feishu_list_users_by_department(
            &client,
            &base_url,
            &tenant_access_token,
            &department_id,
        )
        .await?;

        for user in users {
            let item = user.get("user").unwrap_or(&user);
            let member_id = item
                .get("open_id")
                .or_else(|| item.get("user_id"))
                .or_else(|| item.get("union_id"))
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if member_id.is_empty() {
                continue;
            }

            let mut department_ids = item
                .get("department_ids")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|value| value.as_str())
                        .map(|value| value.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if department_ids.is_empty() && department_id != "0" {
                department_ids.push(department_id.clone());
            }
            department_ids.retain(|value| departments_by_id.contains_key(value));
            department_ids.sort();
            department_ids.dedup();

            let name = item
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let email = item
                .get("enterprise_email")
                .or_else(|| item.get("email"))
                .and_then(|value| value.as_str())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let title = item
                .get("job_title")
                .or_else(|| item.get("title"))
                .and_then(|value| value.as_str())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let feishu_open_id = item
                .get("open_id")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
                .filter(|value| !value.is_empty());

            let entry = members_by_id
                .entry(member_id.clone())
                .or_insert_with(|| OrgMember {
                    id: member_id.clone(),
                    name: name.clone(),
                    email: email.clone(),
                    title: title.clone(),
                    department_id: department_ids.first().cloned(),
                    department_ids: Vec::new(),
                    department_path: None,
                    feishu_open_id: feishu_open_id.clone(),
                });

            if entry.name.trim().is_empty() && !name.trim().is_empty() {
                entry.name = name;
            }
            if entry.email.is_none() {
                entry.email = email;
            }
            if entry.title.is_none() {
                entry.title = title;
            }
            if entry.feishu_open_id.is_none() {
                entry.feishu_open_id = feishu_open_id;
            }
            for id in department_ids {
                if !entry.department_ids.contains(&id) {
                    entry.department_ids.push(id.clone());
                }
                members_by_department
                    .entry(id)
                    .or_default()
                    .insert(member_id.clone());
            }
            entry.department_ids.sort();
            entry.department_ids.dedup();
            if entry.department_id.is_none() {
                entry.department_id = entry.department_ids.first().cloned();
            }
        }
    }

    for department in &mut departments {
        department.member_count = members_by_department
            .get(&department.id)
            .map(|items| items.len())
            .unwrap_or_default();
    }

    let mut members = members_by_id.into_values().collect::<Vec<_>>();
    for member in &mut members {
        member.department_path = member
            .department_ids
            .iter()
            .find_map(|department_id| build_department_path(department_id, &departments_by_id));
    }

    departments.sort_by(|left, right| {
        left.parent_id
            .cmp(&right.parent_id)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.id.cmp(&right.id))
    });
    members.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
    });

    Ok(OrgDirectorySnapshot {
        provider: "feishu".to_string(),
        tenant_id: tenant_id.to_string(),
        synced_at: chrono::Utc::now().to_rfc3339(),
        departments,
        members,
    })
}

fn extract_mentions(content: &str) -> Vec<String> {
    content
        .split_whitespace()
        .filter_map(|token| token.strip_prefix('@'))
        .map(|value| {
            value
                .trim_matches(|ch: char| {
                    !ch.is_alphanumeric()
                        && ch != '_'
                        && ch != '-'
                        && !('\u{4e00}'..='\u{9fff}').contains(&ch)
                })
                .to_string()
        })
        .filter(|value| !value.is_empty())
        .collect()
}

fn slugify_company_name(name: &str) -> String {
    let base = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>();
    let base = base
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = if base.is_empty() { "company" } else { &base };
    format!("{base}-{}", &uuid::Uuid::new_v4().simple().to_string()[..6])
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateMeRequest {
    pub username: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
pub struct TenantCreateRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct TenantUpdateRequest {
    pub name: Option<String>,
    pub im_provider: Option<String>,
    pub timezone: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct TenantJoinRequest {
    pub invitation_code: String,
}

pub async fn auth_register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterRequest>,
) -> impl IntoResponse {
    if !state.kernel.config.auth.enabled {
        return api_error(StatusCode::NOT_FOUND, "Auth not enabled");
    }
    if req.username.trim().is_empty() || req.email.trim().is_empty() || req.password.len() < 6 {
        return api_error(StatusCode::BAD_REQUEST, "Invalid registration payload");
    }
    let existing_user = match state
        .control_store
        .get_user_by_username(req.username.trim())
    {
        Ok(user) => user,
        Err(error) => return internal_error(error),
    };
    if existing_user.is_some() {
        return api_error(StatusCode::CONFLICT, "Username already exists");
    }

    let user = DashboardUser {
        user_id: uuid::Uuid::new_v4().to_string(),
        username: req.username.trim().to_string(),
        email: req.email.trim().to_string(),
        password_hash: crate::session_auth::hash_password(&req.password),
        display_name: if req.display_name.trim().is_empty() {
            req.username.trim().to_string()
        } else {
            req.display_name.trim().to_string()
        },
        role: "member".to_string(),
        tenant_id: None,
        is_active: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        quota_message_limit: 50,
        quota_message_period: "permanent".to_string(),
        quota_messages_used: 0,
        quota_max_agents: 2,
        quota_agent_ttl_hours: 48,
        source: "registration".to_string(),
    };
    if let Err(error) = state.control_store.upsert_user(&user) {
        return internal_error(error);
    }

    let token = crate::session_auth::create_session_token(
        &user.username,
        &auth_secret(&state),
        state.kernel.config.auth.session_ttl_hours,
    );
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "access_token": token,
            "token_type": "bearer",
            "user": user_json(&user),
            "needs_company_setup": true,
        })),
    )
}

pub async fn auth_login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    if !state.kernel.config.auth.enabled {
        return api_error(StatusCode::NOT_FOUND, "Auth not enabled");
    }

    let Some(user) = (match state
        .control_store
        .get_user_by_username(req.username.trim())
    {
        Ok(user) => user,
        Err(error) => return internal_error(error),
    }) else {
        return api_error(StatusCode::UNAUTHORIZED, "Invalid credentials");
    };

    if !user.is_active {
        return api_error(StatusCode::FORBIDDEN, "Account is disabled");
    }
    if !crate::session_auth::verify_password(&req.password, &user.password_hash) {
        return api_error(StatusCode::UNAUTHORIZED, "Invalid credentials");
    }
    if let Some(ref tenant_id) = user.tenant_id {
        let tenant = match state.control_store.get_tenant(tenant_id) {
            Ok(tenant) => tenant,
            Err(error) => return internal_error(error),
        };
        if let Some(tenant) = tenant {
            if !tenant.is_active {
                return api_error(
                    StatusCode::FORBIDDEN,
                    "Your company has been disabled. Please contact the platform administrator.",
                );
            }
        }
    }

    let token = crate::session_auth::create_session_token(
        &user.username,
        &auth_secret(&state),
        state.kernel.config.auth.session_ttl_hours,
    );
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "access_token": token,
            "token_type": "bearer",
            "user": user_json(&user),
            "needs_company_setup": user.tenant_id.is_none(),
        })),
    )
}

pub async fn auth_me(State(state): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    match current_dashboard_user(&state, &headers) {
        Ok(user) => (StatusCode::OK, Json(user_json(&user))),
        Err(error) => error,
    }
}

pub async fn auth_update_me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<UpdateMeRequest>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    match state.control_store.update_user_profile(
        &user.user_id,
        req.username
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty()),
        req.display_name
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty()),
    ) {
        Ok(Some(user)) => (StatusCode::OK, Json(user_json(&user))),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "User not found"),
        Err(error) => internal_error(error),
    }
}

pub async fn auth_update_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<UpdatePasswordRequest>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if !crate::session_auth::verify_password(&req.old_password, &user.password_hash) {
        return api_error(StatusCode::UNAUTHORIZED, "Invalid current password");
    }
    if req.new_password.len() < 6 {
        return api_error(
            StatusCode::BAD_REQUEST,
            "New password must be at least 6 characters",
        );
    }
    match state.control_store.update_user_password(
        &user.user_id,
        &crate::session_auth::hash_password(&req.new_password),
    ) {
        Ok(Some(_)) => (StatusCode::OK, Json(serde_json::json!({"status": "ok"}))),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "User not found"),
        Err(error) => internal_error(error),
    }
}

pub async fn registration_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state
        .control_store
        .get_bool_setting("allow_self_create_company", true)
    {
        Ok(enabled) => (
            StatusCode::OK,
            Json(serde_json::json!({ "allow_self_create_company": enabled })),
        ),
        Err(error) => internal_error(error),
    }
}

pub async fn self_create_company(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<TenantCreateRequest>,
) -> impl IntoResponse {
    let mut user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if user.tenant_id.is_some() {
        return api_error(StatusCode::BAD_REQUEST, "You already belong to a company");
    }
    match state
        .control_store
        .get_bool_setting("allow_self_create_company", true)
    {
        Ok(false) if user.role != "platform_admin" => {
            return api_error(
                StatusCode::FORBIDDEN,
                "Company self-creation is currently disabled",
            );
        }
        Ok(_) => {}
        Err(error) => return internal_error(error),
    }
    let tenant = default_dashboard_tenant(&req.name, slugify_company_name(&req.name));
    if let Err(error) = state.control_store.upsert_tenant(&tenant) {
        return internal_error(error);
    }

    user.tenant_id = Some(tenant.tenant_id.clone());
    if user.role == "member" {
        user.role = "org_admin".to_string();
    }
    user.quota_message_limit = tenant.default_message_limit;
    user.quota_message_period = tenant.default_message_period.clone();
    user.quota_max_agents = tenant.default_max_agents;
    user.quota_agent_ttl_hours = tenant.default_agent_ttl_hours;
    user.updated_at = chrono::Utc::now();
    if let Err(error) = state.control_store.upsert_user(&user) {
        return internal_error(error);
    }

    (StatusCode::CREATED, Json(tenant_json(&tenant)))
}

pub async fn join_company(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<TenantJoinRequest>,
) -> impl IntoResponse {
    let mut user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if user.tenant_id.is_some() {
        return api_error(StatusCode::BAD_REQUEST, "You already belong to a company");
    }
    let Some(code) = (match state
        .control_store
        .get_invitation_code_by_code(req.invitation_code.trim())
    {
        Ok(code) => code,
        Err(error) => return internal_error(error),
    }) else {
        return api_error(StatusCode::BAD_REQUEST, "Invalid invitation code");
    };
    if !code.is_active || code.used_count >= code.max_uses {
        return api_error(
            StatusCode::BAD_REQUEST,
            "Invitation code has reached its usage limit",
        );
    }
    let Some(tenant_id) = code.tenant_id.clone() else {
        return api_error(
            StatusCode::BAD_REQUEST,
            "Invitation code is not tenant-scoped",
        );
    };
    let Some(tenant) = (match state.control_store.get_tenant(&tenant_id) {
        Ok(tenant) => tenant,
        Err(error) => return internal_error(error),
    }) else {
        return api_error(StatusCode::BAD_REQUEST, "Company not found");
    };
    if !tenant.is_active {
        return api_error(StatusCode::BAD_REQUEST, "Company not found or is disabled");
    }
    let has_admin = match state.control_store.list_users(Some(&tenant_id)) {
        Ok(users) => users,
        Err(error) => return internal_error(error),
    }
    .into_iter()
    .any(|existing| existing.role == "org_admin" || existing.role == "platform_admin");

    user.tenant_id = Some(tenant_id.clone());
    if user.role == "member" && !has_admin {
        user.role = "org_admin".to_string();
    }
    user.quota_message_limit = tenant.default_message_limit;
    user.quota_message_period = tenant.default_message_period.clone();
    user.quota_max_agents = tenant.default_max_agents;
    user.quota_agent_ttl_hours = tenant.default_agent_ttl_hours;
    user.updated_at = chrono::Utc::now();

    if let Err(error) = state.control_store.upsert_user(&user) {
        return internal_error(error);
    }
    if let Err(error) = state
        .control_store
        .increment_invitation_code_usage(&code.invitation_id)
    {
        return internal_error(error);
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "tenant": tenant_json(&tenant),
            "role": user.role,
        })),
    )
}

pub async fn list_tenants(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin"]) {
        return error;
    }
    match state.control_store.list_tenants() {
        Ok(items) => (
            StatusCode::OK,
            Json(serde_json::json!(items
                .iter()
                .map(tenant_json)
                .collect::<Vec<_>>())),
        ),
        Err(error) => internal_error(error),
    }
}

pub async fn get_tenant(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(tenant_id): Path<String>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if user.role == "org_admin" && user.tenant_id.as_deref() != Some(tenant_id.as_str()) {
        return api_error(StatusCode::FORBIDDEN, "Access denied");
    }
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    match state.control_store.get_tenant(&tenant_id) {
        Ok(Some(tenant)) => (StatusCode::OK, Json(tenant_json(&tenant))),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "Tenant not found"),
        Err(error) => internal_error(error),
    }
}

pub async fn update_tenant(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(tenant_id): Path<String>,
    Json(req): Json<TenantUpdateRequest>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if user.role == "org_admin" && user.tenant_id.as_deref() != Some(tenant_id.as_str()) {
        return api_error(StatusCode::FORBIDDEN, "Can only update your own company");
    }
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let Some(mut tenant) = (match state.control_store.get_tenant(&tenant_id) {
        Ok(tenant) => tenant,
        Err(error) => return internal_error(error),
    }) else {
        return api_error(StatusCode::NOT_FOUND, "Tenant not found");
    };
    let mut updated = false;
    if let Some(name) = req
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        tenant.name = name.to_string();
        updated = true;
    }
    if let Some(im_provider) = req
        .im_provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        tenant.im_provider = im_provider.to_string();
        updated = true;
    }
    if let Some(timezone) = req
        .timezone
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        tenant.timezone = timezone.to_string();
        updated = true;
    }
    if let Some(is_active) = req.is_active {
        tenant.is_active = is_active;
        updated = true;
    }
    if !updated {
        return api_error(StatusCode::BAD_REQUEST, "No tenant fields were provided");
    }
    if let Err(error) = state.control_store.upsert_tenant(&tenant) {
        return internal_error(error);
    }
    (StatusCode::OK, Json(tenant_json(&tenant)))
}

pub async fn delete_tenant(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(tenant_id): Path<String>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if user.role == "org_admin" && user.tenant_id.as_deref() != Some(tenant_id.as_str()) {
        return api_error(StatusCode::FORBIDDEN, "Can only delete your own company");
    }
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    match state.control_store.delete_tenant(&tenant_id) {
        Ok(Some(fallback_tenant_id)) => (
            StatusCode::OK,
            Json(serde_json::json!({ "fallback_tenant_id": fallback_tenant_id })),
        ),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "Tenant not found"),
        Err(SiliCrewError::InvalidInput(message)) => api_error(StatusCode::BAD_REQUEST, message),
        Err(error) => internal_error(error),
    }
}

#[derive(Debug, Deserialize)]
pub struct CompanyCreateRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct PlatformSettingsUpdate {
    pub allow_self_create_company: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UsersQuery {
    pub tenant_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateQuotaRequest {
    pub quota_message_limit: i32,
    pub quota_message_period: String,
    pub quota_max_agents: i32,
    pub quota_agent_ttl_hours: i32,
}

#[derive(Debug, Deserialize)]
pub struct InvitationCodesQuery {
    pub page: Option<usize>,
    pub page_size: Option<usize>,
    pub search: Option<String>,
    pub tenant_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct InvitationCodesCreateRequest {
    pub count: usize,
    pub max_uses: i32,
    pub tenant_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OrgMembersQuery {
    pub tenant_id: Option<String>,
    pub department_id: Option<String>,
    pub search: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TenantScopedQuery {
    pub tenant_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AuditLogsQuery {
    pub tenant_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct ForceQuery {
    pub force: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct PlazaPostsQuery {
    pub tenant_id: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct PlazaPostCreateRequest {
    pub content: String,
    pub author_id: Option<String>,
    pub author_type: Option<String>,
    pub author_name: Option<String>,
    pub tenant_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PlazaCommentCreateRequest {
    pub content: String,
    pub author_id: Option<String>,
    pub author_type: Option<String>,
    pub author_name: Option<String>,
}

pub async fn admin_list_companies(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin"]) {
        return error;
    }
    match state.control_store.list_company_stats() {
        Ok(items) => (
            StatusCode::OK,
            Json(serde_json::json!(items
                .into_iter()
                .map(|item| serde_json::json!({
                    "id": item.tenant.tenant_id,
                    "name": item.tenant.name,
                    "slug": item.tenant.slug,
                    "is_active": item.tenant.is_active,
                    "created_at": item.tenant.created_at.to_rfc3339(),
                    "user_count": item.user_count,
                    "agent_count": item.agent_count,
                    "agent_running_count": item.agent_running_count,
                    "total_tokens": item.total_tokens,
                }))
                .collect::<Vec<_>>())),
        ),
        Err(error) => internal_error(error),
    }
}

pub async fn admin_create_company(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CompanyCreateRequest>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin"]) {
        return error;
    }
    let tenant = default_dashboard_tenant(&req.name, slugify_company_name(&req.name));
    if let Err(error) = state.control_store.upsert_tenant(&tenant) {
        return internal_error(error);
    }
    let code = InvitationCodeRecord {
        invitation_id: uuid::Uuid::new_v4().to_string(),
        code: uuid::Uuid::new_v4().simple().to_string()[..16].to_uppercase(),
        tenant_id: Some(tenant.tenant_id.clone()),
        max_uses: 1,
        used_count: 0,
        is_active: true,
        created_by: Some(user.user_id),
        created_at: chrono::Utc::now(),
    };
    if let Err(error) = state.control_store.create_invitation_code(&code) {
        return internal_error(error);
    }
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "company": {
                "id": tenant.tenant_id,
                "name": tenant.name,
                "slug": tenant.slug,
                "is_active": tenant.is_active,
                "created_at": tenant.created_at.to_rfc3339(),
            },
            "admin_invitation_code": code.code,
        })),
    )
}

pub async fn admin_toggle_company(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(company_id): Path<String>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin"]) {
        return error;
    }
    let Some(mut tenant) = (match state.control_store.get_tenant(&company_id) {
        Ok(tenant) => tenant,
        Err(error) => return internal_error(error),
    }) else {
        return api_error(StatusCode::NOT_FOUND, "Company not found");
    };
    tenant.is_active = !tenant.is_active;
    if let Err(error) = state.control_store.upsert_tenant(&tenant) {
        return internal_error(error);
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({ "ok": true, "is_active": tenant.is_active })),
    )
}

pub async fn admin_get_platform_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin"]) {
        return error;
    }
    match state
        .control_store
        .get_bool_setting("allow_self_create_company", true)
    {
        Ok(allow_self_create_company) => (
            StatusCode::OK,
            Json(serde_json::json!({ "allow_self_create_company": allow_self_create_company })),
        ),
        Err(error) => internal_error(error),
    }
}

pub async fn admin_update_platform_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<PlatformSettingsUpdate>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin"]) {
        return error;
    }
    if let Some(value) = req.allow_self_create_company {
        if let Err(error) = state
            .control_store
            .set_bool_setting("allow_self_create_company", value)
        {
            return internal_error(error);
        }
    }
    match state
        .control_store
        .get_bool_setting("allow_self_create_company", true)
    {
        Ok(allow_self_create_company) => (
            StatusCode::OK,
            Json(serde_json::json!({ "allow_self_create_company": allow_self_create_company })),
        ),
        Err(error) => internal_error(error),
    }
}

pub async fn list_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UsersQuery>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let tenant_filter = if user.role == "platform_admin" {
        query.tenant_id.as_deref()
    } else {
        user.tenant_id.as_deref()
    };
    match state
        .control_store
        .list_users_with_agent_counts(tenant_filter)
    {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!(items))),
        Err(error) => internal_error(error),
    }
}

pub async fn update_user_quota(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(req): Json<UpdateQuotaRequest>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    match state.control_store.update_user_quota(
        &user_id,
        req.quota_message_limit,
        &req.quota_message_period,
        req.quota_max_agents,
        req.quota_agent_ttl_hours,
    ) {
        Ok(Some(user)) => (StatusCode::OK, Json(user_json(&user))),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "User not found"),
        Err(error) => internal_error(error),
    }
}

pub async fn list_invitation_codes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<InvitationCodesQuery>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let tenant_filter = if user.role == "platform_admin" {
        query.tenant_id.as_deref()
    } else {
        user.tenant_id.as_deref()
    };
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;
    match state.control_store.list_invitation_codes(
        tenant_filter,
        query.search.as_deref().filter(|value| !value.is_empty()),
        page_size,
        offset,
    ) {
        Ok((items, total)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "items": items.into_iter().map(|item| serde_json::json!({
                    "id": item.invitation_id,
                    "code": item.code,
                    "tenant_id": item.tenant_id,
                    "max_uses": item.max_uses,
                    "used_count": item.used_count,
                    "is_active": item.is_active,
                    "created_at": item.created_at.to_rfc3339(),
                })).collect::<Vec<_>>(),
                "total": total,
            })),
        ),
        Err(error) => internal_error(error),
    }
}

pub async fn create_invitation_codes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<InvitationCodesCreateRequest>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let tenant_id = if user.role == "platform_admin" {
        req.tenant_id.clone().or(user.tenant_id.clone())
    } else {
        user.tenant_id.clone()
    };
    let Some(tenant_id) = tenant_id else {
        return api_error(StatusCode::BAD_REQUEST, "Tenant context is required");
    };
    let count = req.count.clamp(1, 100);
    for _ in 0..count {
        let code = InvitationCodeRecord {
            invitation_id: uuid::Uuid::new_v4().to_string(),
            code: uuid::Uuid::new_v4().simple().to_string()[..16].to_uppercase(),
            tenant_id: Some(tenant_id.clone()),
            max_uses: req.max_uses.max(1),
            used_count: 0,
            is_active: true,
            created_by: Some(user.user_id.clone()),
            created_at: chrono::Utc::now(),
        };
        if let Err(error) = state.control_store.create_invitation_code(&code) {
            return internal_error(error);
        }
    }
    (
        StatusCode::CREATED,
        Json(serde_json::json!({"status": "ok"})),
    )
}

pub async fn delete_invitation_code(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(invitation_id): Path<String>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    match state
        .control_store
        .deactivate_invitation_code(&invitation_id)
    {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({"status": "ok"}))),
        Ok(false) => api_error(StatusCode::NOT_FOUND, "Invitation code not found"),
        Err(error) => internal_error(error),
    }
}

pub async fn export_invitation_codes_csv(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error.into_response();
    }
    let tenant_filter = if user.role == "platform_admin" {
        None
    } else {
        user.tenant_id.as_deref()
    };
    let (items, _) = match state
        .control_store
        .list_invitation_codes(tenant_filter, None, 10_000, 0)
    {
        Ok(result) => result,
        Err(error) => return internal_error(error).into_response(),
    };
    let mut csv = "code,max_uses,used_count,is_active,created_at\n".to_string();
    for item in items {
        csv.push_str(&format!(
            "{},{},{},{},{}\n",
            item.code, item.max_uses, item.used_count, item.is_active, item.created_at
        ));
    }
    (
        StatusCode::OK,
        [
            ("content-type", "text/csv; charset=utf-8"),
            (
                "content-disposition",
                "attachment; filename=\"invitation_codes.csv\"",
            ),
        ],
        csv,
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
pub struct SystemSettingUpsertRequest {
    pub value: serde_json::Value,
}

pub async fn get_system_setting(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<TenantScopedQuery>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let storage_key = match scoped_setting_storage_key(&user, &key, query.tenant_id.as_deref()) {
        Ok(key) => key,
        Err(error) => return error,
    };
    match state.control_store.get_setting(&storage_key) {
        Ok(Some(setting)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "key": setting.key,
                "value": if key == FEISHU_SYNC_KEY {
                    serde_json::from_str::<FeishuOrgSyncConfig>(&setting.value_json)
                        .map(|config| sanitize_feishu_sync_config(&config))
                        .unwrap_or_else(|_| serde_json::json!({}))
                } else {
                    serde_json::from_str::<serde_json::Value>(&setting.value_json).unwrap_or_else(|_| serde_json::json!({}))
                },
                "updated_at": setting.updated_at.to_rfc3339(),
            })),
        ),
        Ok(None) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "key": key,
                "value": serde_json::json!({}),
            })),
        ),
        Err(error) => internal_error(error),
    }
}

pub async fn get_system_setting_public(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    match state.control_store.get_setting(&key) {
        Ok(Some(setting)) => (
            StatusCode::OK,
            Json(
                serde_json::from_str::<serde_json::Value>(&setting.value_json)
                    .unwrap_or_else(|_| serde_json::json!({})),
            ),
        ),
        Ok(None) => (StatusCode::OK, Json(serde_json::json!({}))),
        Err(error) => internal_error(error),
    }
}

pub async fn upsert_system_setting(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<TenantScopedQuery>,
    Path(key): Path<String>,
    Json(req): Json<SystemSettingUpsertRequest>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let storage_key = match scoped_setting_storage_key(&user, &key, query.tenant_id.as_deref()) {
        Ok(key) => key,
        Err(error) => return error,
    };
    match state
        .control_store
        .upsert_setting(&storage_key, &req.value.to_string())
    {
        Ok(setting) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "key": setting.key,
                "value": if key == FEISHU_SYNC_KEY {
                    serde_json::from_value::<FeishuOrgSyncConfig>(req.value.clone())
                        .map(|config| sanitize_feishu_sync_config(&config))
                        .unwrap_or_else(|_| serde_json::json!({}))
                } else {
                    req.value
                },
                "updated_at": setting.updated_at.to_rfc3339(),
            })),
        ),
        Err(error) => internal_error(error),
    }
}

pub async fn enterprise_org_departments(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<OrgMembersQuery>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let tenant_id = match resolve_tenant_scope(&user, query.tenant_id.as_deref()) {
        Ok(tenant_id) => tenant_id,
        Err(error) => return error,
    };

    let snapshot = match load_org_directory_snapshot(&state, &tenant_id) {
        Ok(snapshot) => snapshot,
        Err(error) => return error,
    };
    let departments = snapshot
        .map(|snapshot| {
            snapshot
                .departments
                .into_iter()
                .map(|department| {
                    serde_json::json!({
                        "id": department.id,
                        "name": department.name,
                        "parent_id": department.parent_id,
                        "member_count": department.member_count,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    (StatusCode::OK, Json(serde_json::json!(departments)))
}

pub async fn enterprise_org_members(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<OrgMembersQuery>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let tenant_id = match resolve_tenant_scope(&user, query.tenant_id.as_deref()) {
        Ok(tenant_id) => tenant_id,
        Err(error) => return error,
    };
    let search = query.search.as_deref().map(|value| value.to_lowercase());
    let snapshot = match load_org_directory_snapshot(&state, &tenant_id) {
        Ok(snapshot) => snapshot,
        Err(error) => return error,
    };

    if let Some(snapshot) = snapshot {
        let allowed_departments = query
            .department_id
            .as_deref()
            .map(|department_id| department_descendants(&snapshot.departments, department_id));
        let items = snapshot
            .members
            .into_iter()
            .filter(|member| {
                allowed_departments.as_ref().map_or(true, |allowed| {
                    member
                        .department_ids
                        .iter()
                        .any(|department_id| allowed.contains(department_id))
                })
            })
            .filter(|member| {
                search.as_ref().map_or(true, |search| {
                    member.name.to_lowercase().contains(search)
                        || member
                            .email
                            .as_deref()
                            .unwrap_or_default()
                            .to_lowercase()
                            .contains(search)
                        || member
                            .title
                            .as_deref()
                            .unwrap_or_default()
                            .to_lowercase()
                            .contains(search)
                        || member
                            .department_path
                            .as_deref()
                            .unwrap_or_default()
                            .to_lowercase()
                            .contains(search)
                })
            })
            .map(|member| org_member_json(&member))
            .collect::<Vec<_>>();
        return (StatusCode::OK, Json(serde_json::json!(items)));
    }

    let users = match state.control_store.list_users(Some(&tenant_id)) {
        Ok(users) => users,
        Err(error) => return internal_error(error),
    };
    let items = users
        .into_iter()
        .filter(|entry| {
            search.as_ref().map_or(true, |search| {
                entry.username.to_lowercase().contains(search)
                    || entry.display_name.to_lowercase().contains(search)
                    || entry.email.to_lowercase().contains(search)
            })
        })
        .map(|entry| {
            serde_json::json!({
                "id": entry.user_id,
                "name": entry.display_name,
                "email": entry.email,
                "title": serde_json::Value::Null,
                "department_id": serde_json::Value::Null,
                "department_ids": [],
                "department_path": serde_json::Value::Null,
                "feishu_open_id": serde_json::Value::Null,
            })
        })
        .collect::<Vec<_>>();
    (StatusCode::OK, Json(serde_json::json!(items)))
}

pub async fn enterprise_org_sync(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<TenantScopedQuery>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let tenant_id = match resolve_tenant_scope(&user, query.tenant_id.as_deref()) {
        Ok(tenant_id) => tenant_id,
        Err(error) => return error,
    };

    let Some(mut config) = (match load_feishu_sync_config(&state, &tenant_id) {
        Ok(config) => config,
        Err(error) => return error,
    }) else {
        return api_error(
            StatusCode::BAD_REQUEST,
            "Feishu org sync is not configured for this tenant",
        );
    };

    let snapshot = match sync_feishu_org_snapshot(&tenant_id, &config).await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            config.last_error = Some(error.clone());
            let _ = save_feishu_sync_config(&state, &tenant_id, &config);
            return api_error(StatusCode::BAD_REQUEST, error);
        }
    };

    config.last_synced_at = Some(snapshot.synced_at.clone());
    config.last_error = None;
    config.last_departments = snapshot.departments.len();
    config.last_members = snapshot.members.len();

    if let Err(error) = save_org_directory_snapshot(&state, &tenant_id, &snapshot) {
        return error;
    }
    if let Err(error) = save_feishu_sync_config(&state, &tenant_id, &config) {
        return error;
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "departments": snapshot.departments.len(),
            "members": snapshot.members.len(),
            "synced_at": snapshot.synced_at,
            "provider": snapshot.provider,
        })),
    )
}

pub async fn plaza_org_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<OrgMembersQuery>,
) -> impl IntoResponse {
    enterprise_org_members(State(state), headers, Query(query)).await
}

pub async fn enterprise_tenant_quotas(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<TenantScopedQuery>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let tenant_id = match resolve_tenant_scope(&user, query.tenant_id.as_deref()) {
        Ok(tenant_id) => tenant_id,
        Err(error) => return error,
    };
    let Some(tenant) = (match state.control_store.get_tenant(&tenant_id) {
        Ok(tenant) => tenant,
        Err(error) => return internal_error(error),
    }) else {
        return api_error(StatusCode::NOT_FOUND, "Tenant not found");
    };
    let extra = match state
        .control_store
        .get_setting(&format!("tenant_quotas_{tenant_id}"))
    {
        Ok(Some(setting)) => serde_json::from_str::<serde_json::Value>(&setting.value_json)
            .unwrap_or_else(|_| serde_json::json!({})),
        Ok(None) => serde_json::json!({}),
        Err(error) => return internal_error(error),
    };
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "default_message_limit": tenant.default_message_limit,
            "default_message_period": tenant.default_message_period,
            "default_max_agents": tenant.default_max_agents,
            "default_agent_ttl_hours": tenant.default_agent_ttl_hours,
            "default_max_llm_calls_per_day": extra.get("default_max_llm_calls_per_day").and_then(|v| v.as_i64()).unwrap_or(tenant.default_max_llm_calls_per_day as i64),
            "min_heartbeat_interval_minutes": extra.get("min_heartbeat_interval_minutes").and_then(|v| v.as_i64()).unwrap_or(tenant.min_heartbeat_interval_minutes as i64),
            "default_max_triggers": extra.get("default_max_triggers").and_then(|v| v.as_i64()).unwrap_or(tenant.default_max_triggers as i64),
            "min_poll_interval_floor": extra.get("min_poll_interval_floor").and_then(|v| v.as_i64()).unwrap_or(tenant.min_poll_interval_floor as i64),
            "max_webhook_rate_ceiling": extra.get("max_webhook_rate_ceiling").and_then(|v| v.as_i64()).unwrap_or(tenant.max_webhook_rate_ceiling as i64),
        })),
    )
}

pub async fn update_enterprise_tenant_quotas(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<TenantScopedQuery>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let tenant_id = match resolve_tenant_scope(&user, query.tenant_id.as_deref()) {
        Ok(tenant_id) => tenant_id,
        Err(error) => return error,
    };
    let Some(mut tenant) = (match state.control_store.get_tenant(&tenant_id) {
        Ok(tenant) => tenant,
        Err(error) => return internal_error(error),
    }) else {
        return api_error(StatusCode::NOT_FOUND, "Tenant not found");
    };
    tenant.default_message_limit = req
        .get("default_message_limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(tenant.default_message_limit as i64) as i32;
    tenant.default_message_period = req
        .get("default_message_period")
        .and_then(|v| v.as_str())
        .unwrap_or(&tenant.default_message_period)
        .to_string();
    tenant.default_max_agents = req
        .get("default_max_agents")
        .and_then(|v| v.as_i64())
        .unwrap_or(tenant.default_max_agents as i64) as i32;
    tenant.default_agent_ttl_hours =
        req.get("default_agent_ttl_hours")
            .and_then(|v| v.as_i64())
            .unwrap_or(tenant.default_agent_ttl_hours as i64) as i32;
    tenant.default_max_llm_calls_per_day =
        req.get("default_max_llm_calls_per_day")
            .and_then(|v| v.as_i64())
            .unwrap_or(tenant.default_max_llm_calls_per_day as i64) as i32;
    tenant.min_heartbeat_interval_minutes =
        req.get("min_heartbeat_interval_minutes")
            .and_then(|v| v.as_i64())
            .unwrap_or(tenant.min_heartbeat_interval_minutes as i64) as i32;
    tenant.default_max_triggers = req
        .get("default_max_triggers")
        .and_then(|v| v.as_i64())
        .unwrap_or(tenant.default_max_triggers as i64) as i32;
    tenant.min_poll_interval_floor =
        req.get("min_poll_interval_floor")
            .and_then(|v| v.as_i64())
            .unwrap_or(tenant.min_poll_interval_floor as i64) as i32;
    tenant.max_webhook_rate_ceiling =
        req.get("max_webhook_rate_ceiling")
            .and_then(|v| v.as_i64())
            .unwrap_or(tenant.max_webhook_rate_ceiling as i64) as i32;
    if let Err(error) = state.control_store.upsert_tenant(&tenant) {
        return internal_error(error);
    }
    let extra = serde_json::json!({
        "default_max_llm_calls_per_day": req.get("default_max_llm_calls_per_day").and_then(|v| v.as_i64()).unwrap_or(100),
        "min_heartbeat_interval_minutes": req.get("min_heartbeat_interval_minutes").and_then(|v| v.as_i64()).unwrap_or(120),
        "default_max_triggers": req.get("default_max_triggers").and_then(|v| v.as_i64()).unwrap_or(20),
        "min_poll_interval_floor": req.get("min_poll_interval_floor").and_then(|v| v.as_i64()).unwrap_or(5),
        "max_webhook_rate_ceiling": req.get("max_webhook_rate_ceiling").and_then(|v| v.as_i64()).unwrap_or(5),
    });
    if let Err(error) = state
        .control_store
        .upsert_setting(&format!("tenant_quotas_{tenant_id}"), &extra.to_string())
    {
        return internal_error(error);
    }
    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}

pub async fn enterprise_stats(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<TenantScopedQuery>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let tenant_id = match resolve_tenant_scope(&user, query.tenant_id.as_deref()) {
        Ok(tenant_id) => tenant_id,
        Err(error) => return error,
    };
    let total_users = match state.control_store.list_users(Some(&tenant_id)) {
        Ok(users) => users.len(),
        Err(error) => return internal_error(error),
    };
    let registry_agents = state.kernel.registry.list();
    let tenant_agents = registry_agents
        .iter()
        .filter(|entry| agent_tenant_id(entry).as_deref() == Some(tenant_id.as_str()))
        .collect::<Vec<_>>();
    let total_agents = tenant_agents.len();
    let running_agents = tenant_agents
        .iter()
        .filter(|entry| format!("{:?}", entry.state).eq_ignore_ascii_case("running"))
        .count();
    let pending_approvals = state
        .kernel
        .approval_manager
        .list_pending()
        .into_iter()
        .filter(|approval| {
            tenant_agents.iter().any(|entry| {
                entry.id.to_string() == approval.agent_id || entry.name == approval.agent_id
            })
        })
        .count();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "total_users": total_users,
            "total_agents": total_agents,
            "running_agents": running_agents,
            "pending_approvals": pending_approvals,
        })),
    )
}

pub async fn enterprise_approvals(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<TenantScopedQuery>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let tenant_id = match resolve_tenant_scope(&user, query.tenant_id.as_deref()) {
        Ok(tenant_id) => tenant_id,
        Err(error) => return error,
    };
    let registry_agents = state.kernel.registry.list();
    let agent_name_for = |agent_id: &str| {
        registry_agents
            .iter()
            .find(|entry| entry.id.to_string() == agent_id || entry.name == agent_id)
            .and_then(|entry| {
                if agent_tenant_id(entry).as_deref() == Some(tenant_id.as_str()) {
                    Some(entry.name.clone())
                } else {
                    None
                }
            })
    };
    let mut items = Vec::new();
    for request in state.kernel.approval_manager.list_pending() {
        let Some(agent_name) = agent_name_for(&request.agent_id) else {
            continue;
        };
        items.push(serde_json::json!({
            "id": request.id,
            "agent_id": request.agent_id,
            "agent_name": agent_name,
            "action_type": request.action_summary,
            "created_at": request.requested_at,
            "status": "pending",
        }));
    }
    for record in state.kernel.approval_manager.list_recent(200) {
        let Some(agent_name) = agent_name_for(&record.request.agent_id) else {
            continue;
        };
        let status = match record.decision {
            openparlant_types::approval::ApprovalDecision::Approved => "approved",
            openparlant_types::approval::ApprovalDecision::Denied => "rejected",
            openparlant_types::approval::ApprovalDecision::TimedOut => "expired",
        };
        items.push(serde_json::json!({
            "id": record.request.id,
            "agent_id": record.request.agent_id,
            "agent_name": agent_name,
            "action_type": record.request.action_summary,
            "created_at": record.request.requested_at,
            "status": status,
        }));
    }
    items.sort_by(|a, b| b["created_at"].as_str().cmp(&a["created_at"].as_str()));
    (StatusCode::OK, Json(serde_json::json!(items)))
}

#[derive(Debug, Deserialize)]
pub struct ResolveApprovalRequest {
    pub action: String,
}

pub async fn enterprise_resolve_approval(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<ResolveApprovalRequest>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let uuid = match uuid::Uuid::parse_str(&id) {
        Ok(uuid) => uuid,
        Err(_) => return api_error(StatusCode::BAD_REQUEST, "Invalid approval ID"),
    };
    let decision = match req.action.as_str() {
        "approve" => openparlant_types::approval::ApprovalDecision::Approved,
        "reject" => openparlant_types::approval::ApprovalDecision::Denied,
        _ => return api_error(StatusCode::BAD_REQUEST, "Invalid action"),
    };
    match state
        .kernel
        .approval_manager
        .resolve(uuid, decision, Some(user.username))
    {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"status": "ok"}))),
        Err(error) => api_error(StatusCode::NOT_FOUND, error),
    }
}

pub async fn enterprise_audit_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<AuditLogsQuery>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let tenant_id = match resolve_tenant_scope(&user, query.tenant_id.as_deref()) {
        Ok(tenant_id) => tenant_id,
        Err(error) => return error,
    };
    let limit = query.limit.unwrap_or(200).clamp(1, 1000);
    let registry_agents = state.kernel.registry.list();
    let items = state
        .kernel
        .audit_log
        .recent(limit)
        .into_iter()
        .filter(|entry| {
            registry_agents.iter().any(|agent| {
                (agent.id.to_string() == entry.agent_id || agent.name == entry.agent_id)
                    && agent_tenant_id(agent).as_deref() == Some(tenant_id.as_str())
            })
        })
        .map(|entry| {
            serde_json::json!({
                "id": entry.seq,
                "created_at": entry.timestamp,
                "agent_id": entry.agent_id,
                "action": format!("{:?}", entry.action),
                "details": {
                    "detail": entry.detail,
                    "outcome": entry.outcome,
                },
            })
        })
        .collect::<Vec<_>>();
    (StatusCode::OK, Json(serde_json::json!(items)))
}

pub async fn enterprise_llm_providers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let catalog = match state.kernel.model_catalog.read() {
        Ok(catalog) => catalog,
        Err(_) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Model catalog unavailable",
            )
        }
    };
    let items = catalog
        .list_providers()
        .iter()
        .map(|provider| {
            let default_max_tokens = catalog
                .default_model_for_provider(&provider.id)
                .and_then(|model_id| catalog.find_model(&model_id).map(|model| model.max_output_tokens))
                .unwrap_or(4096);
            serde_json::json!({
                "provider": provider.id,
                "display_name": provider.display_name,
                "protocol": provider_protocol(&provider.id),
                "default_base_url": if provider.base_url.is_empty() { serde_json::Value::Null } else { serde_json::json!(provider.base_url) },
                "supports_tool_choice": provider_supports_tool_choice(&provider.id),
                "default_max_tokens": default_max_tokens,
            })
        })
        .collect::<Vec<_>>();
    (StatusCode::OK, Json(serde_json::json!(items)))
}

pub async fn enterprise_llm_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<TenantScopedQuery>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let tenant_id = match resolve_tenant_scope(&user, query.tenant_id.as_deref()) {
        Ok(tenant_id) => tenant_id,
        Err(error) => return error,
    };
    let models = match load_tenant_llm_models(&state, &tenant_id) {
        Ok(models) => models,
        Err(error) => return error,
    };
    let masked = models
        .into_iter()
        .map(|mut model| {
            if let Some(api_key) = model.get("api_key").and_then(|value| value.as_str()) {
                let masked = if api_key.is_empty() {
                    String::new()
                } else {
                    format!("{}••••••", &api_key.chars().take(6).collect::<String>())
                };
                if let Some(object) = model.as_object_mut() {
                    object.remove("api_key");
                    object.insert("api_key_masked".to_string(), serde_json::json!(masked));
                }
            }
            model
        })
        .collect::<Vec<_>>();
    (StatusCode::OK, Json(serde_json::json!(masked)))
}

pub async fn enterprise_add_llm_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<TenantScopedQuery>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let tenant_id = match resolve_tenant_scope(&user, query.tenant_id.as_deref()) {
        Ok(tenant_id) => tenant_id,
        Err(error) => return error,
    };
    let mut models = match load_tenant_llm_models(&state, &tenant_id) {
        Ok(models) => models,
        Err(error) => return error,
    };
    let model = serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "provider": req.get("provider").and_then(|v| v.as_str()).unwrap_or("custom"),
        "model": req.get("model").and_then(|v| v.as_str()).unwrap_or(""),
        "label": req.get("label").and_then(|v| v.as_str()).unwrap_or(""),
        "base_url": req.get("base_url").and_then(|v| v.as_str()).unwrap_or(""),
        "api_key": req.get("api_key").and_then(|v| v.as_str()).unwrap_or(""),
        "supports_vision": req.get("supports_vision").and_then(|v| v.as_bool()).unwrap_or(false),
        "max_output_tokens": req.get("max_output_tokens").and_then(|v| v.as_u64()).unwrap_or(4096),
        "enabled": true,
        "created_at": chrono::Utc::now().to_rfc3339(),
    });
    models.push(model.clone());
    if let Err(error) = save_tenant_llm_models(&state, &tenant_id, &models) {
        return error;
    }
    (StatusCode::CREATED, Json(model))
}

pub async fn enterprise_update_llm_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let tenant_id = match resolve_tenant_scope(&user, None) {
        Ok(tenant_id) => tenant_id,
        Err(error) => return error,
    };
    let mut models = match load_tenant_llm_models(&state, &tenant_id) {
        Ok(models) => models,
        Err(error) => return error,
    };
    let Some(index) = models
        .iter()
        .position(|model| model.get("id").and_then(|v| v.as_str()) == Some(id.as_str()))
    else {
        return api_error(StatusCode::NOT_FOUND, "Model not found");
    };
    if let Some(object) = models[index].as_object_mut() {
        for key in [
            "provider",
            "model",
            "label",
            "base_url",
            "supports_vision",
            "max_output_tokens",
            "enabled",
        ] {
            if let Some(value) = req.get(key) {
                object.insert(key.to_string(), value.clone());
            }
        }
        if let Some(api_key) = req.get("api_key").and_then(|v| v.as_str()) {
            if !api_key.is_empty() {
                object.insert("api_key".to_string(), serde_json::json!(api_key));
            }
        }
    }
    if let Err(error) = save_tenant_llm_models(&state, &tenant_id, &models) {
        return error;
    }
    (StatusCode::OK, Json(models[index].clone()))
}

pub async fn enterprise_delete_llm_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(_query): Query<ForceQuery>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let tenant_id = match resolve_tenant_scope(&user, None) {
        Ok(tenant_id) => tenant_id,
        Err(error) => return error,
    };
    let mut models = match load_tenant_llm_models(&state, &tenant_id) {
        Ok(models) => models,
        Err(error) => return error,
    };
    let before = models.len();
    models.retain(|model| model.get("id").and_then(|v| v.as_str()) != Some(id.as_str()));
    if before == models.len() {
        return api_error(StatusCode::NOT_FOUND, "Model not found");
    }
    if let Err(error) = save_tenant_llm_models(&state, &tenant_id, &models) {
        return error;
    }
    (StatusCode::NO_CONTENT, Json(serde_json::json!(null)))
}

pub async fn enterprise_llm_test(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if let Err(error) = require_roles(&user, &["platform_admin", "org_admin"]) {
        return error;
    }
    let provider = req.get("provider").and_then(|v| v.as_str()).unwrap_or("");
    let model = req.get("model").and_then(|v| v.as_str()).unwrap_or("");
    if provider.is_empty() || model.is_empty() {
        return api_error(StatusCode::BAD_REQUEST, "provider and model are required");
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "latency_ms": 0,
            "message": "Validation-only test passed",
        })),
    )
}

pub async fn plaza_list_posts(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PlazaPostsQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(20).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    match state
        .control_store
        .list_plaza_posts(query.tenant_id.as_deref(), limit, offset)
    {
        Ok(posts) => (
            StatusCode::OK,
            Json(serde_json::json!(posts
                .into_iter()
                .map(|post| serde_json::json!({
                    "id": post.id,
                    "author_id": post.author_id,
                    "author_type": post.author_type,
                    "author_name": post.author_name,
                    "content": post.content,
                    "likes_count": post.likes_count,
                    "comments_count": post.comments_count,
                    "created_at": post.created_at.to_rfc3339(),
                }))
                .collect::<Vec<_>>())),
        ),
        Err(error) => internal_error(error),
    }
}

fn send_social_notification(
    state: &AppState,
    tenant_id: Option<String>,
    user_id: &str,
    notification_type: &str,
    title: String,
    body: Option<String>,
    link: Option<String>,
    sender_id: Option<String>,
    sender_name: Option<String>,
) {
    let _ = crate::notification_routes::create_notification(
        state,
        openparlant_control::NotificationRecord {
            id: uuid::Uuid::new_v4().to_string(),
            tenant_id,
            user_id: user_id.to_string(),
            notification_type: notification_type.to_string(),
            category: "social".to_string(),
            title,
            body,
            link,
            sender_id,
            sender_name,
            created_at: chrono::Utc::now(),
            read_at: None,
        },
    );
}

fn notify_mentions_for_content(
    state: &AppState,
    tenant_id: &str,
    content: &str,
    sender_user_id: &str,
    sender_name: &str,
    link: &str,
) {
    let mentions = extract_mentions(content);
    if mentions.is_empty() {
        return;
    }
    let mut notified = HashSet::new();

    if let Ok(users) = state.control_store.list_users(Some(tenant_id)) {
        for mention in &mentions {
            if let Some(user) = users.iter().find(|user| {
                user.user_id != sender_user_id
                    && (user.display_name.eq_ignore_ascii_case(mention)
                        || user.username.eq_ignore_ascii_case(mention))
            }) {
                if notified.insert(user.user_id.clone()) {
                    send_social_notification(
                        state,
                        Some(tenant_id.to_string()),
                        &user.user_id,
                        "mention",
                        format!("{sender_name} mentioned you"),
                        Some(content.chars().take(160).collect()),
                        Some(link.to_string()),
                        Some(sender_user_id.to_string()),
                        Some(sender_name.to_string()),
                    );
                }
            }
        }
    }

    for agent in state.kernel.registry.list() {
        if agent_tenant_id(&agent).as_deref() != Some(tenant_id) {
            continue;
        }
        let agent_name = agent.name.clone();
        let creator_id = agent
            .manifest
            .metadata
            .get("creator_user_id")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        let Some(creator_id) = creator_id else {
            continue;
        };
        if creator_id == sender_user_id {
            continue;
        }
        if mentions
            .iter()
            .any(|mention| mention.eq_ignore_ascii_case(&agent_name))
            && notified.insert(creator_id.clone())
        {
            send_social_notification(
                state,
                Some(tenant_id.to_string()),
                &creator_id,
                "mention",
                format!("{sender_name} mentioned {agent_name}"),
                Some(content.chars().take(160).collect()),
                Some(link.to_string()),
                Some(sender_user_id.to_string()),
                Some(sender_name.to_string()),
            );
        }
    }
}

pub async fn plaza_stats(
    State(state): State<Arc<AppState>>,
    Query(query): Query<TenantScopedQuery>,
) -> impl IntoResponse {
    match state.control_store.plaza_stats(query.tenant_id.as_deref()) {
        Ok(stats) => (StatusCode::OK, Json(stats)),
        Err(error) => internal_error(error),
    }
}

pub async fn plaza_get_post(
    State(state): State<Arc<AppState>>,
    Path(post_id): Path<String>,
) -> impl IntoResponse {
    let post = match state.control_store.get_plaza_post(&post_id) {
        Ok(Some(post)) => post,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "Post not found"),
        Err(error) => return internal_error(error),
    };
    let comments = match state.control_store.list_plaza_comments(&post_id) {
        Ok(comments) => comments,
        Err(error) => return internal_error(error),
    };
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "id": post.id,
            "author_id": post.author_id,
            "author_type": post.author_type,
            "author_name": post.author_name,
            "content": post.content,
            "likes_count": post.likes_count,
            "comments_count": post.comments_count,
            "created_at": post.created_at.to_rfc3339(),
            "comments": comments.into_iter().map(|comment| serde_json::json!({
                "id": comment.id,
                "post_id": comment.post_id,
                "author_id": comment.author_id,
                "author_type": comment.author_type,
                "author_name": comment.author_name,
                "content": comment.content,
                "created_at": comment.created_at.to_rfc3339(),
            })).collect::<Vec<_>>(),
        })),
    )
}

pub async fn plaza_create_post(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<PlazaPostCreateRequest>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if req.content.trim().is_empty() {
        return api_error(StatusCode::BAD_REQUEST, "Content cannot be empty");
    }
    let tenant_id = match resolve_tenant_scope(&user, req.tenant_id.as_deref()) {
        Ok(tenant_id) => tenant_id,
        Err(error) => return error,
    };
    let post = openparlant_control::PlazaPostRecord {
        id: uuid::Uuid::new_v4().to_string(),
        author_id: user.user_id.clone(),
        author_type: req.author_type.unwrap_or_else(|| "human".to_string()),
        author_name: if user.display_name.is_empty() {
            user.username.clone()
        } else {
            user.display_name.clone()
        },
        content: req.content.trim().chars().take(500).collect(),
        tenant_id: Some(tenant_id),
        likes_count: 0,
        comments_count: 0,
        created_at: chrono::Utc::now(),
    };
    match state.control_store.create_plaza_post(&post) {
        Ok(()) => {
            if let Some(ref tenant_id) = post.tenant_id {
                notify_mentions_for_content(
                    &state,
                    tenant_id,
                    &post.content,
                    &post.author_id,
                    &post.author_name,
                    &format!("/plaza?post={}", post.id),
                );
            }
            (
                StatusCode::CREATED,
                Json(serde_json::json!({
                    "id": post.id,
                    "author_id": post.author_id,
                    "author_type": post.author_type,
                    "author_name": post.author_name,
                    "content": post.content,
                    "likes_count": post.likes_count,
                    "comments_count": post.comments_count,
                    "created_at": post.created_at.to_rfc3339(),
                })),
            )
        }
        Err(error) => internal_error(error),
    }
}

pub async fn plaza_add_comment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(post_id): Path<String>,
    Json(req): Json<PlazaCommentCreateRequest>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    if req.content.trim().is_empty() {
        return api_error(StatusCode::BAD_REQUEST, "Content cannot be empty");
    }
    match state.control_store.get_plaza_post(&post_id) {
        Ok(Some(_)) => {}
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "Post not found"),
        Err(error) => return internal_error(error),
    }
    let comment = openparlant_control::PlazaCommentRecord {
        id: uuid::Uuid::new_v4().to_string(),
        post_id: post_id.clone(),
        author_id: user.user_id.clone(),
        author_type: req.author_type.unwrap_or_else(|| "human".to_string()),
        author_name: if user.display_name.is_empty() {
            user.username.clone()
        } else {
            user.display_name.clone()
        },
        content: req.content.trim().chars().take(300).collect(),
        created_at: chrono::Utc::now(),
    };
    match state.control_store.create_plaza_comment(&comment) {
        Ok(()) => {
            if let Ok(Some(post)) = state.control_store.get_plaza_post(&post_id) {
                let link = format!("/plaza?post={}", post.id);
                if post.author_id != user.user_id {
                    if post.author_type == "human" {
                        send_social_notification(
                            &state,
                            post.tenant_id.clone(),
                            &post.author_id,
                            "plaza_comment",
                            format!("{} commented on your post", comment.author_name),
                            Some(comment.content.chars().take(160).collect()),
                            Some(link.clone()),
                            Some(user.user_id.clone()),
                            Some(comment.author_name.clone()),
                        );
                    } else {
                        for agent in state.kernel.registry.list() {
                            if agent.id.to_string() == post.author_id {
                                if let Some(creator_id) = agent
                                    .manifest
                                    .metadata
                                    .get("creator_user_id")
                                    .and_then(|value| value.as_str())
                                {
                                    if creator_id != user.user_id {
                                        send_social_notification(
                                            &state,
                                            post.tenant_id.clone(),
                                            creator_id,
                                            "plaza_comment",
                                            format!(
                                                "{} commented on {}'s post",
                                                comment.author_name, post.author_name
                                            ),
                                            Some(comment.content.chars().take(160).collect()),
                                            Some(link.clone()),
                                            Some(user.user_id.clone()),
                                            Some(comment.author_name.clone()),
                                        );
                                    }
                                }
                                break;
                            }
                        }
                    }
                }
                if let Some(ref tenant_id) = post.tenant_id {
                    notify_mentions_for_content(
                        &state,
                        tenant_id,
                        &comment.content,
                        &comment.author_id,
                        &comment.author_name,
                        &link,
                    );
                }
            }
            (
                StatusCode::CREATED,
                Json(serde_json::json!({
                    "id": comment.id,
                    "post_id": comment.post_id,
                    "author_id": comment.author_id,
                    "author_type": comment.author_type,
                    "author_name": comment.author_name,
                    "content": comment.content,
                    "created_at": comment.created_at.to_rfc3339(),
                })),
            )
        }
        Err(error) => internal_error(error),
    }
}

pub async fn plaza_toggle_like(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(post_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    let author_type = query
        .get("author_type")
        .cloned()
        .unwrap_or_else(|| "human".to_string());
    match state
        .control_store
        .toggle_plaza_like(&post_id, &user.user_id, &author_type)
    {
        Ok(liked) => {
            if liked {
                if let Ok(Some(post)) = state.control_store.get_plaza_post(&post_id) {
                    if post.author_id != user.user_id {
                        let target_user_id = if post.author_type == "human" {
                            Some(post.author_id.clone())
                        } else {
                            state
                                .kernel
                                .registry
                                .list()
                                .into_iter()
                                .find(|agent| agent.id.to_string() == post.author_id)
                                .and_then(|agent| {
                                    agent
                                        .manifest
                                        .metadata
                                        .get("creator_user_id")
                                        .and_then(|value| value.as_str())
                                        .map(|value| value.to_string())
                                })
                        };
                        if let Some(target_user_id) = target_user_id {
                            send_social_notification(
                                &state,
                                post.tenant_id.clone(),
                                &target_user_id,
                                "plaza_like",
                                format!("{} liked your post", user.display_name),
                                Some(post.content.chars().take(120).collect()),
                                Some(format!("/plaza?post={}", post.id)),
                                Some(user.user_id.clone()),
                                Some(user.display_name.clone()),
                            );
                        }
                    }
                }
            }
            (StatusCode::OK, Json(serde_json::json!({ "liked": liked })))
        }
        Err(error) => internal_error(error),
    }
}

pub async fn plaza_delete_post(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(post_id): Path<String>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    let post = match state.control_store.get_plaza_post(&post_id) {
        Ok(Some(post)) => post,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "Post not found"),
        Err(error) => return internal_error(error),
    };
    let is_admin = matches!(user.role.as_str(), "platform_admin" | "org_admin");
    let is_author = post.author_id == user.user_id;
    if !is_admin && !is_author {
        return api_error(StatusCode::FORBIDDEN, "Not allowed to delete this post");
    }
    match state.control_store.delete_plaza_post(&post_id) {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({ "deleted": true }))),
        Ok(false) => api_error(StatusCode::NOT_FOUND, "Post not found"),
        Err(error) => internal_error(error),
    }
}
