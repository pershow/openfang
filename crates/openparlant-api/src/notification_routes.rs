use crate::dashboard_routes::{
    agent_tenant_id, current_dashboard_user, internal_error, require_roles, resolve_tenant_scope,
};
use crate::routes::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use silicrew_control::NotificationRecord;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct NotificationListQuery {
    pub limit: Option<usize>,
    pub category: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BroadcastRequest {
    pub title: String,
    pub body: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TenantScopedQuery {
    pub tenant_id: Option<String>,
}

pub(crate) fn notification_json(notification: &NotificationRecord) -> serde_json::Value {
    serde_json::json!({
        "id": notification.id,
        "tenant_id": notification.tenant_id,
        "type": notification.notification_type,
        "category": notification.category,
        "title": notification.title,
        "body": notification.body,
        "link": notification.link,
        "sender_id": notification.sender_id,
        "sender_name": notification.sender_name,
        "created_at": notification.created_at.to_rfc3339(),
        "read_at": notification.read_at.map(|dt| dt.to_rfc3339()),
        "is_read": notification.read_at.is_some(),
    })
}

pub(crate) fn create_notification(
    state: &AppState,
    notification: NotificationRecord,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    state
        .control_store
        .create_notification(&notification)
        .map_err(internal_error)
}

pub async fn list_notifications(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<NotificationListQuery>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    match state
        .control_store
        .list_notifications(&user.user_id, query.category.as_deref(), limit)
    {
        Ok(items) => (
            StatusCode::OK,
            Json(serde_json::json!(items
                .into_iter()
                .map(|n| notification_json(&n))
                .collect::<Vec<_>>())),
        ),
        Err(error) => internal_error(error),
    }
}

pub async fn notifications_unread_count(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    match state.control_store.unread_notification_count(&user.user_id) {
        Ok(unread_count) => (
            StatusCode::OK,
            Json(serde_json::json!({ "unread_count": unread_count })),
        ),
        Err(error) => internal_error(error),
    }
}

pub async fn notification_mark_read(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    match state
        .control_store
        .mark_notification_read(&id, &user.user_id)
    {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({ "status": "ok" }))),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "detail": "Notification not found" })),
        ),
        Err(error) => internal_error(error),
    }
}

pub async fn notifications_mark_all_read(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    match state
        .control_store
        .mark_all_notifications_read(&user.user_id)
    {
        Ok(count) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "ok", "count": count })),
        ),
        Err(error) => internal_error(error),
    }
}

pub async fn messages_inbox(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<NotificationListQuery>,
) -> impl IntoResponse {
    let user = match current_dashboard_user(&state, &headers) {
        Ok(user) => user,
        Err(error) => return error,
    };
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    match state
        .control_store
        .list_notifications(&user.user_id, None, limit)
    {
        Ok(items) => (
            StatusCode::OK,
            Json(serde_json::json!(items
                .into_iter()
                .map(|n| {
                    serde_json::json!({
                        "id": n.id,
                        "msg_type": n.notification_type,
                        "sender_name": n.sender_name.unwrap_or_else(|| "System".to_string()),
                        "receiver_name": user.display_name,
                        "content": n.body.unwrap_or_else(|| n.title.clone()),
                        "created_at": n.created_at.to_rfc3339(),
                        "read_at": n.read_at.map(|dt| dt.to_rfc3339()),
                    })
                })
                .collect::<Vec<_>>())),
        ),
        Err(error) => internal_error(error),
    }
}

pub async fn messages_unread_count(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    notifications_unread_count(State(state), headers).await
}

pub async fn messages_mark_read(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    notification_mark_read(State(state), headers, Path(id)).await
}

pub async fn messages_mark_all_read(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    notifications_mark_all_read(State(state), headers).await
}

pub async fn notifications_broadcast(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<TenantScopedQuery>,
    Json(req): Json<BroadcastRequest>,
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
    let users = match state.control_store.list_users(Some(&tenant_id)) {
        Ok(users) => users,
        Err(error) => return internal_error(error),
    };
    for recipient in &users {
        let _ = create_notification(
            &state,
            NotificationRecord {
                id: uuid::Uuid::new_v4().to_string(),
                tenant_id: Some(tenant_id.clone()),
                user_id: recipient.user_id.clone(),
                notification_type: "broadcast".to_string(),
                category: "broadcast".to_string(),
                title: req.title.clone(),
                body: req.body.clone(),
                link: None,
                sender_id: Some(user.user_id.clone()),
                sender_name: Some(user.display_name.clone()),
                created_at: chrono::Utc::now(),
                read_at: None,
            },
        );
    }
    let agents_notified = state
        .kernel
        .registry
        .list()
        .into_iter()
        .filter(|agent| agent_tenant_id(agent).as_deref() == Some(tenant_id.as_str()))
        .count();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "users_notified": users.len(),
            "agents_notified": agents_notified,
        })),
    )
}
