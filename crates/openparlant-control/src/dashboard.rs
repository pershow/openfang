use crate::store::{
    ControlStore, DashboardCompanyStats, DashboardTenant, DashboardUser, InvitationCodeRecord,
    SystemSettingRecord,
};
use chrono::{DateTime, Utc};
use openparlant_memory::db::{block_on, SharedDb};
use openparlant_types::control::{ControlScope, ScopeId};
use openparlant_types::error::{SiliCrewError, SiliCrewResult};
use rusqlite::params;
use sqlx::Row;
use std::collections::HashMap;

fn parse_timestamp(value: &str) -> SiliCrewResult<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").map(|dt| dt.and_utc())
        })
        .map_err(memory_error)
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn memory_error<E: std::fmt::Display>(error: E) -> SiliCrewError {
    SiliCrewError::Memory(error.to_string())
}

fn tenant_from_sqlite_row(
    row: (
        String,
        String,
        String,
        String,
        String,
        i64,
        String,
        i32,
        String,
        i32,
        i32,
        i32,
        i32,
        i32,
        i32,
        i32,
    ),
) -> SiliCrewResult<DashboardTenant> {
    Ok(DashboardTenant {
        tenant_id: row.0,
        name: row.1,
        slug: row.2,
        im_provider: row.3,
        timezone: row.4,
        is_active: row.5 != 0,
        created_at: parse_timestamp(&row.6)?,
        default_message_limit: row.7,
        default_message_period: row.8,
        default_max_agents: row.9,
        default_agent_ttl_hours: row.10,
        default_max_llm_calls_per_day: row.11,
        min_heartbeat_interval_minutes: row.12,
        default_max_triggers: row.13,
        min_poll_interval_floor: row.14,
        max_webhook_rate_ceiling: row.15,
    })
}

fn tenant_from_postgres_row(row: sqlx::postgres::PgRow) -> SiliCrewResult<DashboardTenant> {
    Ok(DashboardTenant {
        tenant_id: row.try_get("id").map_err(memory_error)?,
        name: row.try_get("name").map_err(memory_error)?,
        slug: row.try_get("slug").map_err(memory_error)?,
        im_provider: row.try_get("im_provider").map_err(memory_error)?,
        timezone: row.try_get("timezone").map_err(memory_error)?,
        is_active: row.try_get("is_active").map_err(memory_error)?,
        created_at: parse_timestamp(
            &row.try_get::<String, _>("created_at")
                .map_err(memory_error)?,
        )?,
        default_message_limit: row.try_get("default_message_limit").map_err(memory_error)?,
        default_message_period: row
            .try_get("default_message_period")
            .map_err(memory_error)?,
        default_max_agents: row.try_get("default_max_agents").map_err(memory_error)?,
        default_agent_ttl_hours: row
            .try_get("default_agent_ttl_hours")
            .map_err(memory_error)?,
        default_max_llm_calls_per_day: row
            .try_get("default_max_llm_calls_per_day")
            .map_err(memory_error)?,
        min_heartbeat_interval_minutes: row
            .try_get("min_heartbeat_interval_minutes")
            .map_err(memory_error)?,
        default_max_triggers: row.try_get("default_max_triggers").map_err(memory_error)?,
        min_poll_interval_floor: row
            .try_get("min_poll_interval_floor")
            .map_err(memory_error)?,
        max_webhook_rate_ceiling: row
            .try_get("max_webhook_rate_ceiling")
            .map_err(memory_error)?,
    })
}

impl ControlStore {
    pub fn ensure_default_tenant_and_admin(
        &self,
        username: &str,
        password_hash: &str,
    ) -> SiliCrewResult<(DashboardTenant, DashboardUser)> {
        let tenant = match self.get_tenant_by_slug("default")? {
            Some(existing) => existing,
            None => {
                let tenant = DashboardTenant {
                    tenant_id: uuid::Uuid::new_v4().to_string(),
                    name: "Default Company".to_string(),
                    slug: "default".to_string(),
                    im_provider: "web_only".to_string(),
                    timezone: "UTC".to_string(),
                    is_active: true,
                    created_at: Utc::now(),
                    default_message_limit: 50,
                    default_message_period: "permanent".to_string(),
                    default_max_agents: 2,
                    default_agent_ttl_hours: 48,
                    default_max_llm_calls_per_day: 100,
                    min_heartbeat_interval_minutes: 120,
                    default_max_triggers: 20,
                    min_poll_interval_floor: 5,
                    max_webhook_rate_ceiling: 5,
                };
                self.upsert_tenant(&tenant)?;
                tenant
            }
        };

        let user = match self.get_user_by_username(username)? {
            Some(mut existing) => {
                existing.password_hash = password_hash.to_string();
                existing.role = "platform_admin".to_string();
                existing.is_active = true;
                existing.tenant_id = Some(tenant.tenant_id.clone());
                existing.updated_at = Utc::now();
                self.upsert_user(&existing)?;
                existing
            }
            None => {
                let user = DashboardUser {
                    user_id: uuid::Uuid::new_v4().to_string(),
                    username: username.to_string(),
                    email: format!("{username}@local.openparlant"),
                    password_hash: password_hash.to_string(),
                    display_name: username.to_string(),
                    role: "platform_admin".to_string(),
                    tenant_id: Some(tenant.tenant_id.clone()),
                    is_active: true,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    quota_message_limit: tenant.default_message_limit,
                    quota_message_period: tenant.default_message_period.clone(),
                    quota_messages_used: 0,
                    quota_max_agents: tenant.default_max_agents,
                    quota_agent_ttl_hours: tenant.default_agent_ttl_hours,
                    source: "bootstrap".to_string(),
                };
                self.upsert_user(&user)?;
                user
            }
        };

        Ok((tenant, user))
    }

    pub fn upsert_tenant(&self, tenant: &DashboardTenant) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO tenants (
                        id, name, slug, im_provider, timezone, is_active, created_at,
                        default_message_limit, default_message_period, default_max_agents, default_agent_ttl_hours,
                        default_max_llm_calls_per_day, min_heartbeat_interval_minutes, default_max_triggers,
                        min_poll_interval_floor, max_webhook_rate_ceiling
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                     ON CONFLICT(id) DO UPDATE SET
                        name = excluded.name,
                        slug = excluded.slug,
                        im_provider = excluded.im_provider,
                        timezone = excluded.timezone,
                        is_active = excluded.is_active,
                        default_message_limit = excluded.default_message_limit,
                        default_message_period = excluded.default_message_period,
                        default_max_agents = excluded.default_max_agents,
                        default_agent_ttl_hours = excluded.default_agent_ttl_hours,
                        default_max_llm_calls_per_day = excluded.default_max_llm_calls_per_day,
                        min_heartbeat_interval_minutes = excluded.min_heartbeat_interval_minutes,
                        default_max_triggers = excluded.default_max_triggers,
                        min_poll_interval_floor = excluded.min_poll_interval_floor,
                        max_webhook_rate_ceiling = excluded.max_webhook_rate_ceiling",
                    params![
                        tenant.tenant_id,
                        tenant.name,
                        tenant.slug,
                        tenant.im_provider,
                        tenant.timezone,
                        tenant.is_active as i64,
                        tenant.created_at.to_rfc3339(),
                        tenant.default_message_limit,
                        tenant.default_message_period,
                        tenant.default_max_agents,
                        tenant.default_agent_ttl_hours,
                        tenant.default_max_llm_calls_per_day,
                        tenant.min_heartbeat_interval_minutes,
                        tenant.default_max_triggers,
                        tenant.min_poll_interval_floor,
                        tenant.max_webhook_rate_ceiling,
                    ],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let tenant = tenant.clone();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO tenants (
                            id, name, slug, im_provider, timezone, is_active, created_at,
                            default_message_limit, default_message_period, default_max_agents, default_agent_ttl_hours,
                            default_max_llm_calls_per_day, min_heartbeat_interval_minutes, default_max_triggers,
                            min_poll_interval_floor, max_webhook_rate_ceiling
                        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
                        ON CONFLICT(id) DO UPDATE SET
                            name = EXCLUDED.name,
                            slug = EXCLUDED.slug,
                            im_provider = EXCLUDED.im_provider,
                            timezone = EXCLUDED.timezone,
                            is_active = EXCLUDED.is_active,
                            default_message_limit = EXCLUDED.default_message_limit,
                            default_message_period = EXCLUDED.default_message_period,
                            default_max_agents = EXCLUDED.default_max_agents,
                            default_agent_ttl_hours = EXCLUDED.default_agent_ttl_hours,
                            default_max_llm_calls_per_day = EXCLUDED.default_max_llm_calls_per_day,
                            min_heartbeat_interval_minutes = EXCLUDED.min_heartbeat_interval_minutes,
                            default_max_triggers = EXCLUDED.default_max_triggers,
                            min_poll_interval_floor = EXCLUDED.min_poll_interval_floor,
                            max_webhook_rate_ceiling = EXCLUDED.max_webhook_rate_ceiling",
                    )
                    .bind(tenant.tenant_id)
                    .bind(tenant.name)
                    .bind(tenant.slug)
                    .bind(tenant.im_provider)
                    .bind(tenant.timezone)
                    .bind(tenant.is_active)
                    .bind(tenant.created_at.to_rfc3339())
                    .bind(tenant.default_message_limit)
                    .bind(tenant.default_message_period)
                    .bind(tenant.default_max_agents)
                    .bind(tenant.default_agent_ttl_hours)
                    .bind(tenant.default_max_llm_calls_per_day)
                    .bind(tenant.min_heartbeat_interval_minutes)
                    .bind(tenant.default_max_triggers)
                    .bind(tenant.min_poll_interval_floor)
                    .bind(tenant.max_webhook_rate_ceiling)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }

        self.upsert_scope(&ControlScope {
            scope_id: ScopeId::new(tenant.tenant_id.clone()),
            name: tenant.name.clone(),
            scope_type: "tenant".to_string(),
            status: if tenant.is_active {
                "active".to_string()
            } else {
                "disabled".to_string()
            },
            created_at: tenant.created_at,
            updated_at: Utc::now(),
        })?;

        Ok(())
    }

    pub fn get_tenant(&self, tenant_id: &str) -> SiliCrewResult<Option<DashboardTenant>> {
        self.get_tenant_by("id", tenant_id)
    }

    pub fn get_tenant_by_slug(&self, slug: &str) -> SiliCrewResult<Option<DashboardTenant>> {
        self.get_tenant_by("slug", slug)
    }

    fn get_tenant_by(&self, field: &str, value: &str) -> SiliCrewResult<Option<DashboardTenant>> {
        let sqlite_sql = format!(
            "SELECT id, name, slug, im_provider, timezone, is_active, created_at, default_message_limit, \
             default_message_period, default_max_agents, default_agent_ttl_hours, \
             default_max_llm_calls_per_day, min_heartbeat_interval_minutes, default_max_triggers, \
             min_poll_interval_floor, max_webhook_rate_ceiling \
             FROM tenants WHERE {field} = ?1"
        );
        let pg_sql = format!(
            "SELECT id, name, slug, im_provider, timezone, is_active, created_at, default_message_limit, \
             default_message_period, default_max_agents, default_agent_ttl_hours, \
             default_max_llm_calls_per_day, min_heartbeat_interval_minutes, default_max_triggers, \
             min_poll_interval_floor, max_webhook_rate_ceiling \
             FROM tenants WHERE {field} = $1"
        );
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let row = conn.query_row(&sqlite_sql, params![value], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, i32>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, i32>(9)?,
                        row.get::<_, i32>(10)?,
                        row.get::<_, i32>(11)?,
                        row.get::<_, i32>(12)?,
                        row.get::<_, i32>(13)?,
                        row.get::<_, i32>(14)?,
                        row.get::<_, i32>(15)?,
                    ))
                });
                match row {
                    Ok(row) => Ok(Some(tenant_from_sqlite_row(row)?)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let value = value.to_string();
                let row = block_on(async move {
                    sqlx::query(&pg_sql)
                        .bind(value)
                        .fetch_optional(&*pool)
                        .await
                })
                .map_err(memory_error)?;
                match row {
                    Some(row) => Ok(Some(tenant_from_postgres_row(row)?)),
                    None => Ok(None),
                }
            }
        }
    }

    pub fn list_tenants(&self) -> SiliCrewResult<Vec<DashboardTenant>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare(
                        "SELECT id, name, slug, im_provider, timezone, is_active, created_at, default_message_limit,
                                default_message_period, default_max_agents, default_agent_ttl_hours,
                                default_max_llm_calls_per_day, min_heartbeat_interval_minutes, default_max_triggers,
                                min_poll_interval_floor, max_webhook_rate_ceiling
                         FROM tenants ORDER BY created_at DESC",
                    )
                    .map_err(memory_error)?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, i64>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, i32>(7)?,
                            row.get::<_, String>(8)?,
                            row.get::<_, i32>(9)?,
                            row.get::<_, i32>(10)?,
                            row.get::<_, i32>(11)?,
                            row.get::<_, i32>(12)?,
                            row.get::<_, i32>(13)?,
                            row.get::<_, i32>(14)?,
                            row.get::<_, i32>(15)?,
                        ))
                    })
                    .map_err(memory_error)?;
                rows.map(|row| tenant_from_sqlite_row(row.map_err(memory_error)?))
                    .collect()
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT id, name, slug, im_provider, timezone, is_active, created_at, default_message_limit,
                                default_message_period, default_max_agents, default_agent_ttl_hours,
                                default_max_llm_calls_per_day, min_heartbeat_interval_minutes, default_max_triggers,
                                min_poll_interval_floor, max_webhook_rate_ceiling
                         FROM tenants ORDER BY created_at DESC",
                    )
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(memory_error)?;
                rows.into_iter().map(tenant_from_postgres_row).collect()
            }
        }
    }

    pub fn delete_tenant(&self, tenant_id: &str) -> SiliCrewResult<Option<String>> {
        let Some(tenant) = self.get_tenant(tenant_id)? else {
            return Ok(None);
        };
        if tenant.slug == "default" {
            return Err(SiliCrewError::InvalidInput(
                "default tenant cannot be deleted".to_string(),
            ));
        }

        let fallback = self
            .get_tenant_by_slug("default")?
            .ok_or_else(|| SiliCrewError::Memory("default tenant missing".to_string()))?;

        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "UPDATE users SET tenant_id = ?1, updated_at = ?2 WHERE tenant_id = ?3",
                    params![fallback.tenant_id, now_rfc3339(), tenant_id],
                )
                .map_err(memory_error)?;
                conn.execute(
                    "DELETE FROM invitation_codes WHERE tenant_id = ?1",
                    params![tenant_id],
                )
                .map_err(memory_error)?;
                conn.execute("DELETE FROM tenants WHERE id = ?1", params![tenant_id])
                    .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let tenant_id = tenant_id.to_string();
                let fallback_id = fallback.tenant_id.clone();
                let updated_at = now_rfc3339();
                block_on(async move {
                    sqlx::query(
                        "UPDATE users SET tenant_id = $1, updated_at = $2 WHERE tenant_id = $3",
                    )
                    .bind(fallback_id)
                    .bind(updated_at)
                    .bind(&tenant_id)
                    .execute(&*pool)
                    .await?;
                    sqlx::query("DELETE FROM invitation_codes WHERE tenant_id = $1")
                        .bind(&tenant_id)
                        .execute(&*pool)
                        .await?;
                    sqlx::query("DELETE FROM tenants WHERE id = $1")
                        .bind(&tenant_id)
                        .execute(&*pool)
                        .await
                })
                .map_err(memory_error)?;
            }
        }

        Ok(Some(fallback.tenant_id))
    }

    pub fn upsert_user(&self, user: &DashboardUser) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO users (
                        user_id, username, email, password_hash, display_name, role, tenant_id,
                        is_active, created_at, updated_at, quota_message_limit, quota_message_period,
                        quota_messages_used, quota_max_agents, quota_agent_ttl_hours, source
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                    ON CONFLICT(user_id) DO UPDATE SET
                        username = excluded.username,
                        email = excluded.email,
                        password_hash = excluded.password_hash,
                        display_name = excluded.display_name,
                        role = excluded.role,
                        tenant_id = excluded.tenant_id,
                        is_active = excluded.is_active,
                        updated_at = excluded.updated_at,
                        quota_message_limit = excluded.quota_message_limit,
                        quota_message_period = excluded.quota_message_period,
                        quota_messages_used = excluded.quota_messages_used,
                        quota_max_agents = excluded.quota_max_agents,
                        quota_agent_ttl_hours = excluded.quota_agent_ttl_hours,
                        source = excluded.source",
                    params![
                        user.user_id,
                        user.username,
                        user.email,
                        user.password_hash,
                        user.display_name,
                        user.role,
                        user.tenant_id,
                        user.is_active as i64,
                        user.created_at.to_rfc3339(),
                        user.updated_at.to_rfc3339(),
                        user.quota_message_limit,
                        user.quota_message_period,
                        user.quota_messages_used,
                        user.quota_max_agents,
                        user.quota_agent_ttl_hours,
                        user.source,
                    ],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let user = user.clone();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO users (
                            user_id, username, email, password_hash, display_name, role, tenant_id,
                            is_active, created_at, updated_at, quota_message_limit, quota_message_period,
                            quota_messages_used, quota_max_agents, quota_agent_ttl_hours, source
                        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
                        ON CONFLICT(user_id) DO UPDATE SET
                            username = EXCLUDED.username,
                            email = EXCLUDED.email,
                            password_hash = EXCLUDED.password_hash,
                            display_name = EXCLUDED.display_name,
                            role = EXCLUDED.role,
                            tenant_id = EXCLUDED.tenant_id,
                            is_active = EXCLUDED.is_active,
                            updated_at = EXCLUDED.updated_at,
                            quota_message_limit = EXCLUDED.quota_message_limit,
                            quota_message_period = EXCLUDED.quota_message_period,
                            quota_messages_used = EXCLUDED.quota_messages_used,
                            quota_max_agents = EXCLUDED.quota_max_agents,
                            quota_agent_ttl_hours = EXCLUDED.quota_agent_ttl_hours,
                            source = EXCLUDED.source",
                    )
                    .bind(user.user_id)
                    .bind(user.username)
                    .bind(user.email)
                    .bind(user.password_hash)
                    .bind(user.display_name)
                    .bind(user.role)
                    .bind(user.tenant_id)
                    .bind(user.is_active)
                    .bind(user.created_at.to_rfc3339())
                    .bind(user.updated_at.to_rfc3339())
                    .bind(user.quota_message_limit)
                    .bind(user.quota_message_period)
                    .bind(user.quota_messages_used)
                    .bind(user.quota_max_agents)
                    .bind(user.quota_agent_ttl_hours)
                    .bind(user.source)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    pub fn get_user_by_username(&self, username: &str) -> SiliCrewResult<Option<DashboardUser>> {
        self.get_user_by("username", username)
    }

    pub fn get_user_by_id(&self, user_id: &str) -> SiliCrewResult<Option<DashboardUser>> {
        self.get_user_by("user_id", user_id)
    }

    fn get_user_by(&self, field: &str, value: &str) -> SiliCrewResult<Option<DashboardUser>> {
        let sqlite_sql = format!(
            "SELECT user_id, username, email, password_hash, display_name, role, tenant_id,
                    is_active, created_at, updated_at, quota_message_limit, quota_message_period,
                    quota_messages_used, quota_max_agents, quota_agent_ttl_hours, source
             FROM users WHERE {field} = ?1"
        );
        let pg_sql = format!(
            "SELECT user_id, username, email, password_hash, display_name, role, tenant_id,
                    is_active, created_at, updated_at, quota_message_limit, quota_message_period,
                    quota_messages_used, quota_max_agents, quota_agent_ttl_hours, source
             FROM users WHERE {field} = $1"
        );
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let row = conn.query_row(&sqlite_sql, params![value], user_row_sqlite);
                match row {
                    Ok(row) => Ok(Some(user_from_sqlite_row(row)?)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let value = value.to_string();
                let row = block_on(async move {
                    sqlx::query(&pg_sql)
                        .bind(value)
                        .fetch_optional(&*pool)
                        .await
                })
                .map_err(memory_error)?;
                match row {
                    Some(row) => Ok(Some(user_from_postgres_row(row)?)),
                    None => Ok(None),
                }
            }
        }
    }

    pub fn list_users(&self, tenant_id: Option<&str>) -> SiliCrewResult<Vec<DashboardUser>> {
        let sqlite_sql = if tenant_id.is_some() {
            "SELECT user_id, username, email, password_hash, display_name, role, tenant_id,
                    is_active, created_at, updated_at, quota_message_limit, quota_message_period,
                    quota_messages_used, quota_max_agents, quota_agent_ttl_hours, source
             FROM users WHERE tenant_id = ?1 ORDER BY created_at DESC"
        } else {
            "SELECT user_id, username, email, password_hash, display_name, role, tenant_id,
                    is_active, created_at, updated_at, quota_message_limit, quota_message_period,
                    quota_messages_used, quota_max_agents, quota_agent_ttl_hours, source
             FROM users ORDER BY created_at DESC"
        };
        let pg_sql = if tenant_id.is_some() {
            "SELECT user_id, username, email, password_hash, display_name, role, tenant_id,
                    is_active, created_at, updated_at, quota_message_limit, quota_message_period,
                    quota_messages_used, quota_max_agents, quota_agent_ttl_hours, source
             FROM users WHERE tenant_id = $1 ORDER BY created_at DESC"
        } else {
            "SELECT user_id, username, email, password_hash, display_name, role, tenant_id,
                    is_active, created_at, updated_at, quota_message_limit, quota_message_period,
                    quota_messages_used, quota_max_agents, quota_agent_ttl_hours, source
             FROM users ORDER BY created_at DESC"
        };
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn.prepare(sqlite_sql).map_err(memory_error)?;
                let rows = if let Some(tenant_id) = tenant_id {
                    stmt.query_map(params![tenant_id], user_row_sqlite)
                } else {
                    stmt.query_map([], user_row_sqlite)
                }
                .map_err(memory_error)?;
                rows.map(|row| user_from_sqlite_row(row.map_err(memory_error)?))
                    .collect()
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let tenant_id = tenant_id.map(ToOwned::to_owned);
                let rows = block_on(async move {
                    if let Some(tenant_id) = tenant_id {
                        sqlx::query(pg_sql).bind(tenant_id).fetch_all(&*pool).await
                    } else {
                        sqlx::query(pg_sql).fetch_all(&*pool).await
                    }
                })
                .map_err(memory_error)?;
                rows.into_iter().map(user_from_postgres_row).collect()
            }
        }
    }

    pub fn update_user_profile(
        &self,
        user_id: &str,
        username: Option<&str>,
        display_name: Option<&str>,
    ) -> SiliCrewResult<Option<DashboardUser>> {
        let Some(mut user) = self.get_user_by_id(user_id)? else {
            return Ok(None);
        };
        if let Some(username) = username {
            user.username = username.to_string();
        }
        if let Some(display_name) = display_name {
            user.display_name = display_name.to_string();
        }
        user.updated_at = Utc::now();
        self.upsert_user(&user)?;
        Ok(Some(user))
    }

    pub fn update_user_password(
        &self,
        user_id: &str,
        password_hash: &str,
    ) -> SiliCrewResult<Option<DashboardUser>> {
        let Some(mut user) = self.get_user_by_id(user_id)? else {
            return Ok(None);
        };
        user.password_hash = password_hash.to_string();
        user.updated_at = Utc::now();
        self.upsert_user(&user)?;
        Ok(Some(user))
    }

    pub fn update_user_quota(
        &self,
        user_id: &str,
        quota_message_limit: i32,
        quota_message_period: &str,
        quota_max_agents: i32,
        quota_agent_ttl_hours: i32,
    ) -> SiliCrewResult<Option<DashboardUser>> {
        let Some(mut user) = self.get_user_by_id(user_id)? else {
            return Ok(None);
        };
        user.quota_message_limit = quota_message_limit;
        user.quota_message_period = quota_message_period.to_string();
        user.quota_max_agents = quota_max_agents;
        user.quota_agent_ttl_hours = quota_agent_ttl_hours;
        user.updated_at = Utc::now();
        self.upsert_user(&user)?;
        Ok(Some(user))
    }
}

fn user_row_sqlite(
    row: &rusqlite::Row<'_>,
) -> Result<
    (
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        i64,
        String,
        String,
        i32,
        String,
        i32,
        i32,
        i32,
        String,
    ),
    rusqlite::Error,
> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, String>(3)?,
        row.get::<_, String>(4)?,
        row.get::<_, String>(5)?,
        row.get::<_, Option<String>>(6)?,
        row.get::<_, i64>(7)?,
        row.get::<_, String>(8)?,
        row.get::<_, String>(9)?,
        row.get::<_, i32>(10)?,
        row.get::<_, String>(11)?,
        row.get::<_, i32>(12)?,
        row.get::<_, i32>(13)?,
        row.get::<_, i32>(14)?,
        row.get::<_, String>(15)?,
    ))
}

fn user_from_sqlite_row(
    row: (
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        i64,
        String,
        String,
        i32,
        String,
        i32,
        i32,
        i32,
        String,
    ),
) -> SiliCrewResult<DashboardUser> {
    Ok(DashboardUser {
        user_id: row.0,
        username: row.1,
        email: row.2,
        password_hash: row.3,
        display_name: row.4,
        role: row.5,
        tenant_id: row.6,
        is_active: row.7 != 0,
        created_at: parse_timestamp(&row.8)?,
        updated_at: parse_timestamp(&row.9)?,
        quota_message_limit: row.10,
        quota_message_period: row.11,
        quota_messages_used: row.12,
        quota_max_agents: row.13,
        quota_agent_ttl_hours: row.14,
        source: row.15,
    })
}

fn user_from_postgres_row(row: sqlx::postgres::PgRow) -> SiliCrewResult<DashboardUser> {
    Ok(DashboardUser {
        user_id: row.try_get("user_id").map_err(memory_error)?,
        username: row.try_get("username").map_err(memory_error)?,
        email: row.try_get("email").map_err(memory_error)?,
        password_hash: row.try_get("password_hash").map_err(memory_error)?,
        display_name: row.try_get("display_name").map_err(memory_error)?,
        role: row.try_get("role").map_err(memory_error)?,
        tenant_id: row.try_get("tenant_id").map_err(memory_error)?,
        is_active: row.try_get("is_active").map_err(memory_error)?,
        created_at: parse_timestamp(
            &row.try_get::<String, _>("created_at")
                .map_err(memory_error)?,
        )?,
        updated_at: parse_timestamp(
            &row.try_get::<String, _>("updated_at")
                .map_err(memory_error)?,
        )?,
        quota_message_limit: row.try_get("quota_message_limit").map_err(memory_error)?,
        quota_message_period: row.try_get("quota_message_period").map_err(memory_error)?,
        quota_messages_used: row.try_get("quota_messages_used").map_err(memory_error)?,
        quota_max_agents: row.try_get("quota_max_agents").map_err(memory_error)?,
        quota_agent_ttl_hours: row.try_get("quota_agent_ttl_hours").map_err(memory_error)?,
        source: row.try_get("source").map_err(memory_error)?,
    })
}

fn invitation_from_sqlite_row(
    row: (
        String,
        String,
        Option<String>,
        i32,
        i32,
        i64,
        Option<String>,
        String,
    ),
) -> SiliCrewResult<InvitationCodeRecord> {
    Ok(InvitationCodeRecord {
        invitation_id: row.0,
        code: row.1,
        tenant_id: row.2,
        max_uses: row.3,
        used_count: row.4,
        is_active: row.5 != 0,
        created_by: row.6,
        created_at: parse_timestamp(&row.7)?,
    })
}

fn invitation_row_sqlite(
    row: &rusqlite::Row<'_>,
) -> Result<
    (
        String,
        String,
        Option<String>,
        i32,
        i32,
        i64,
        Option<String>,
        String,
    ),
    rusqlite::Error,
> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, Option<String>>(2)?,
        row.get::<_, i32>(3)?,
        row.get::<_, i32>(4)?,
        row.get::<_, i64>(5)?,
        row.get::<_, Option<String>>(6)?,
        row.get::<_, String>(7)?,
    ))
}

fn invitation_from_postgres_row(
    row: sqlx::postgres::PgRow,
) -> SiliCrewResult<InvitationCodeRecord> {
    Ok(InvitationCodeRecord {
        invitation_id: row.try_get("id").map_err(memory_error)?,
        code: row.try_get("code").map_err(memory_error)?,
        tenant_id: row.try_get("tenant_id").map_err(memory_error)?,
        max_uses: row.try_get("max_uses").map_err(memory_error)?,
        used_count: row.try_get("used_count").map_err(memory_error)?,
        is_active: row.try_get("is_active").map_err(memory_error)?,
        created_by: row.try_get("created_by").map_err(memory_error)?,
        created_at: parse_timestamp(
            &row.try_get::<String, _>("created_at")
                .map_err(memory_error)?,
        )?,
    })
}

fn setting_from_sqlite_row(row: (String, String, String)) -> SiliCrewResult<SystemSettingRecord> {
    Ok(SystemSettingRecord {
        key: row.0,
        value_json: row.1,
        updated_at: parse_timestamp(&row.2)?,
    })
}

fn setting_from_postgres_row(row: sqlx::postgres::PgRow) -> SiliCrewResult<SystemSettingRecord> {
    Ok(SystemSettingRecord {
        key: row.try_get("key").map_err(memory_error)?,
        value_json: row.try_get("value_json").map_err(memory_error)?,
        updated_at: parse_timestamp(
            &row.try_get::<String, _>("updated_at")
                .map_err(memory_error)?,
        )?,
    })
}

fn manifest_metadata_string(
    manifest: &openparlant_types::agent::AgentManifest,
    key: &str,
) -> Option<String> {
    manifest
        .metadata
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

fn manifest_tenant_id(manifest: &openparlant_types::agent::AgentManifest) -> Option<String> {
    manifest_metadata_string(manifest, "tenant_id")
        .or_else(|| manifest_metadata_string(manifest, "control_scope_id"))
}

impl ControlStore {
    pub fn create_invitation_code(&self, code: &InvitationCodeRecord) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO invitation_codes (id, code, tenant_id, max_uses, used_count, is_active, created_by, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        code.invitation_id,
                        code.code,
                        code.tenant_id,
                        code.max_uses,
                        code.used_count,
                        code.is_active as i64,
                        code.created_by,
                        code.created_at.to_rfc3339(),
                    ],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let code = code.clone();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO invitation_codes (id, code, tenant_id, max_uses, used_count, is_active, created_by, created_at)
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                    )
                    .bind(code.invitation_id)
                    .bind(code.code)
                    .bind(code.tenant_id)
                    .bind(code.max_uses)
                    .bind(code.used_count)
                    .bind(code.is_active)
                    .bind(code.created_by)
                    .bind(code.created_at.to_rfc3339())
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    pub fn get_invitation_code_by_code(
        &self,
        code: &str,
    ) -> SiliCrewResult<Option<InvitationCodeRecord>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let row = conn.query_row(
                    "SELECT id, code, tenant_id, max_uses, used_count, is_active, created_by, created_at
                     FROM invitation_codes WHERE code = ?1",
                    params![code],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, i32>(3)?,
                            row.get::<_, i32>(4)?,
                            row.get::<_, i64>(5)?,
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, String>(7)?,
                        ))
                    },
                );
                match row {
                    Ok(row) => Ok(Some(invitation_from_sqlite_row(row)?)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let code = code.to_string();
                let row = block_on(async move {
                    sqlx::query(
                        "SELECT id, code, tenant_id, max_uses, used_count, is_active, created_by, created_at
                         FROM invitation_codes WHERE code = $1",
                    )
                    .bind(code)
                    .fetch_optional(&*pool)
                    .await
                })
                .map_err(memory_error)?;
                match row {
                    Some(row) => Ok(Some(invitation_from_postgres_row(row)?)),
                    None => Ok(None),
                }
            }
        }
    }

    pub fn increment_invitation_code_usage(&self, invitation_id: &str) -> SiliCrewResult<bool> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                Ok(conn
                    .execute(
                        "UPDATE invitation_codes SET used_count = used_count + 1 WHERE id = ?1",
                        params![invitation_id],
                    )
                    .map_err(memory_error)?
                    > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let invitation_id = invitation_id.to_string();
                let result = block_on(async move {
                    sqlx::query(
                        "UPDATE invitation_codes SET used_count = used_count + 1 WHERE id = $1",
                    )
                    .bind(invitation_id)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
                Ok(result.rows_affected() > 0)
            }
        }
    }

    pub fn deactivate_invitation_code(&self, invitation_id: &str) -> SiliCrewResult<bool> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                Ok(conn
                    .execute(
                        "UPDATE invitation_codes SET is_active = 0 WHERE id = ?1",
                        params![invitation_id],
                    )
                    .map_err(memory_error)?
                    > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let invitation_id = invitation_id.to_string();
                let result = block_on(async move {
                    sqlx::query("UPDATE invitation_codes SET is_active = FALSE WHERE id = $1")
                        .bind(invitation_id)
                        .execute(&*pool)
                        .await
                })
                .map_err(memory_error)?;
                Ok(result.rows_affected() > 0)
            }
        }
    }

    pub fn list_invitation_codes(
        &self,
        tenant_id: Option<&str>,
        search: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> SiliCrewResult<(Vec<InvitationCodeRecord>, i64)> {
        let items = match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let sql = match (tenant_id.is_some(), search.is_some()) {
                    (true, true) => "SELECT id, code, tenant_id, max_uses, used_count, is_active, created_by, created_at
                                     FROM invitation_codes WHERE tenant_id = ?1 AND LOWER(code) LIKE ?2
                                     ORDER BY created_at DESC LIMIT ?3 OFFSET ?4",
                    (true, false) => "SELECT id, code, tenant_id, max_uses, used_count, is_active, created_by, created_at
                                      FROM invitation_codes WHERE tenant_id = ?1
                                      ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
                    (false, true) => "SELECT id, code, tenant_id, max_uses, used_count, is_active, created_by, created_at
                                      FROM invitation_codes WHERE LOWER(code) LIKE ?1
                                      ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
                    (false, false) => "SELECT id, code, tenant_id, max_uses, used_count, is_active, created_by, created_at
                                       FROM invitation_codes ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
                };
                let search_value = search.map(|value| format!("%{}%", value.to_lowercase()));
                let mut stmt = conn.prepare(sql).map_err(memory_error)?;
                let rows = match (tenant_id, search_value.as_deref()) {
                    (Some(tenant_id), Some(search_value)) => stmt.query_map(
                        params![tenant_id, search_value, limit as i64, offset as i64],
                        invitation_row_sqlite,
                    ),
                    (Some(tenant_id), None) => stmt.query_map(
                        params![tenant_id, limit as i64, offset as i64],
                        invitation_row_sqlite,
                    ),
                    (None, Some(search_value)) => stmt.query_map(
                        params![search_value, limit as i64, offset as i64],
                        invitation_row_sqlite,
                    ),
                    (None, None) => {
                        stmt.query_map(params![limit as i64, offset as i64], invitation_row_sqlite)
                    }
                }
                .map_err(memory_error)?;
                rows.map(|row| invitation_from_sqlite_row(row.map_err(memory_error)?))
                    .collect::<SiliCrewResult<Vec<_>>>()?
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let tenant_id = tenant_id.map(ToOwned::to_owned);
                let search = search.map(|value| format!("%{}%", value.to_lowercase()));
                block_on(async move {
                    let rows = match (tenant_id.as_deref(), search.as_deref()) {
                        (Some(tenant_id), Some(search)) => sqlx::query(
                            "SELECT id, code, tenant_id, max_uses, used_count, is_active, created_by, created_at
                             FROM invitation_codes WHERE tenant_id = $1 AND LOWER(code) LIKE $2
                             ORDER BY created_at DESC LIMIT $3 OFFSET $4",
                        )
                        .bind(tenant_id)
                        .bind(search)
                        .bind(limit as i64)
                        .bind(offset as i64)
                        .fetch_all(&*pool)
                        .await?,
                        (Some(tenant_id), None) => sqlx::query(
                            "SELECT id, code, tenant_id, max_uses, used_count, is_active, created_by, created_at
                             FROM invitation_codes WHERE tenant_id = $1
                             ORDER BY created_at DESC LIMIT $2 OFFSET $3",
                        )
                        .bind(tenant_id)
                        .bind(limit as i64)
                        .bind(offset as i64)
                        .fetch_all(&*pool)
                        .await?,
                        (None, Some(search)) => sqlx::query(
                            "SELECT id, code, tenant_id, max_uses, used_count, is_active, created_by, created_at
                             FROM invitation_codes WHERE LOWER(code) LIKE $1
                             ORDER BY created_at DESC LIMIT $2 OFFSET $3",
                        )
                        .bind(search)
                        .bind(limit as i64)
                        .bind(offset as i64)
                        .fetch_all(&*pool)
                        .await?,
                        (None, None) => sqlx::query(
                            "SELECT id, code, tenant_id, max_uses, used_count, is_active, created_by, created_at
                             FROM invitation_codes ORDER BY created_at DESC LIMIT $1 OFFSET $2",
                        )
                        .bind(limit as i64)
                        .bind(offset as i64)
                        .fetch_all(&*pool)
                        .await?,
                    };
                    Ok::<Vec<InvitationCodeRecord>, sqlx::Error>(
                        rows.into_iter()
                            .map(|row| invitation_from_postgres_row(row).map_err(|e| sqlx::Error::Decode(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())))))
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                })
                .map_err(memory_error)?
            }
        };

        let total = match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                match (tenant_id, search) {
                    (Some(tenant_id), Some(search)) => conn.query_row(
                        "SELECT COUNT(*) FROM invitation_codes WHERE tenant_id = ?1 AND LOWER(code) LIKE ?2",
                        params![tenant_id, format!("%{}%", search.to_lowercase())],
                        |row| row.get::<_, i64>(0),
                    ),
                    (Some(tenant_id), None) => conn.query_row(
                        "SELECT COUNT(*) FROM invitation_codes WHERE tenant_id = ?1",
                        params![tenant_id],
                        |row| row.get::<_, i64>(0),
                    ),
                    (None, Some(search)) => conn.query_row(
                        "SELECT COUNT(*) FROM invitation_codes WHERE LOWER(code) LIKE ?1",
                        params![format!("%{}%", search.to_lowercase())],
                        |row| row.get::<_, i64>(0),
                    ),
                    (None, None) => conn.query_row(
                        "SELECT COUNT(*) FROM invitation_codes",
                        [],
                        |row| row.get::<_, i64>(0),
                    ),
                }
                .map_err(memory_error)?
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let tenant_id = tenant_id.map(ToOwned::to_owned);
                let search = search.map(|value| format!("%{}%", value.to_lowercase()));
                block_on(async move {
                    let count = match (tenant_id.as_deref(), search.as_deref()) {
                        (Some(tenant_id), Some(search)) => sqlx::query_scalar(
                            "SELECT COUNT(*) FROM invitation_codes WHERE tenant_id = $1 AND LOWER(code) LIKE $2",
                        )
                        .bind(tenant_id)
                        .bind(search)
                        .fetch_one(&*pool)
                        .await?,
                        (Some(tenant_id), None) => sqlx::query_scalar(
                            "SELECT COUNT(*) FROM invitation_codes WHERE tenant_id = $1",
                        )
                        .bind(tenant_id)
                        .fetch_one(&*pool)
                        .await?,
                        (None, Some(search)) => sqlx::query_scalar(
                            "SELECT COUNT(*) FROM invitation_codes WHERE LOWER(code) LIKE $1",
                        )
                        .bind(search)
                        .fetch_one(&*pool)
                        .await?,
                        (None, None) => sqlx::query_scalar("SELECT COUNT(*) FROM invitation_codes")
                            .fetch_one(&*pool)
                            .await?,
                    };
                    Ok::<i64, sqlx::Error>(count)
                })
                .map_err(memory_error)?
            }
        };

        Ok((items, total))
    }

    pub fn get_setting(&self, key: &str) -> SiliCrewResult<Option<SystemSettingRecord>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let row = conn.query_row(
                    "SELECT key, value_json, updated_at FROM system_settings WHERE key = ?1",
                    params![key],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                );
                match row {
                    Ok(row) => Ok(Some(setting_from_sqlite_row(row)?)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let key = key.to_string();
                let row = block_on(async move {
                    sqlx::query(
                        "SELECT key, value_json, updated_at FROM system_settings WHERE key = $1",
                    )
                    .bind(key)
                    .fetch_optional(&*pool)
                    .await
                })
                .map_err(memory_error)?;
                match row {
                    Some(row) => Ok(Some(setting_from_postgres_row(row)?)),
                    None => Ok(None),
                }
            }
        }
    }

    pub fn upsert_setting(
        &self,
        key: &str,
        value_json: &str,
    ) -> SiliCrewResult<SystemSettingRecord> {
        let updated_at = Utc::now();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO system_settings (key, value_json, updated_at)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
                    params![key, value_json, updated_at.to_rfc3339()],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let key = key.to_string();
                let value_json = value_json.to_string();
                let updated_at_str = updated_at.to_rfc3339();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO system_settings (key, value_json, updated_at)
                         VALUES ($1, $2, $3)
                         ON CONFLICT(key) DO UPDATE SET value_json = EXCLUDED.value_json, updated_at = EXCLUDED.updated_at",
                    )
                    .bind(key)
                    .bind(value_json)
                    .bind(updated_at_str)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(SystemSettingRecord {
            key: key.to_string(),
            value_json: value_json.to_string(),
            updated_at,
        })
    }

    pub fn get_bool_setting(&self, key: &str, default: bool) -> SiliCrewResult<bool> {
        Ok(self
            .get_setting(key)?
            .and_then(|setting| serde_json::from_str::<serde_json::Value>(&setting.value_json).ok())
            .and_then(|value| value.get("enabled").and_then(|enabled| enabled.as_bool()))
            .unwrap_or(default))
    }

    pub fn set_bool_setting(&self, key: &str, enabled: bool) -> SiliCrewResult<()> {
        self.upsert_setting(key, &serde_json::json!({ "enabled": enabled }).to_string())?;
        Ok(())
    }

    pub fn list_users_with_agent_counts(
        &self,
        tenant_id: Option<&str>,
    ) -> SiliCrewResult<Vec<serde_json::Value>> {
        let users = self.list_users(tenant_id)?;
        let creator_counts = self.agent_creator_counts(tenant_id)?;
        Ok(users
            .into_iter()
            .map(|user| {
                let agents_count = creator_counts
                    .get(&user.user_id)
                    .copied()
                    .unwrap_or_default();
                serde_json::json!({
                    "id": user.user_id,
                    "username": user.username,
                    "email": user.email,
                    "display_name": user.display_name,
                    "role": user.role,
                    "tenant_id": user.tenant_id,
                    "is_active": user.is_active,
                    "quota_message_limit": user.quota_message_limit,
                    "quota_message_period": user.quota_message_period,
                    "quota_messages_used": user.quota_messages_used,
                    "quota_max_agents": user.quota_max_agents,
                    "quota_agent_ttl_hours": user.quota_agent_ttl_hours,
                    "agents_count": agents_count,
                    "created_at": user.created_at.to_rfc3339(),
                    "source": user.source,
                })
            })
            .collect())
    }

    pub fn list_company_stats(&self) -> SiliCrewResult<Vec<DashboardCompanyStats>> {
        let tenants = self.list_tenants()?;
        let users = self.list_users(None)?;
        let agent_stats = self.company_agent_stats()?;
        Ok(tenants
            .into_iter()
            .map(|tenant| {
                let user_count = users
                    .iter()
                    .filter(|user| user.tenant_id.as_deref() == Some(tenant.tenant_id.as_str()))
                    .count() as i64;
                let (agent_count, agent_running_count, total_tokens) = agent_stats
                    .get(&tenant.tenant_id)
                    .copied()
                    .unwrap_or((0, 0, 0));
                DashboardCompanyStats {
                    tenant,
                    user_count,
                    agent_count,
                    agent_running_count,
                    total_tokens,
                }
            })
            .collect())
    }

    fn load_agent_manifest_rows(
        &self,
    ) -> SiliCrewResult<Vec<(String, String, openparlant_types::agent::AgentManifest)>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare("SELECT id, state, manifest FROM agents")
                    .map_err(memory_error)?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Vec<u8>>(2)?,
                        ))
                    })
                    .map_err(memory_error)?;
                rows.map(|row| {
                    let (agent_id, state, manifest_blob) = row.map_err(memory_error)?;
                    let manifest =
                        rmp_serde::from_slice::<openparlant_types::agent::AgentManifest>(
                            &manifest_blob,
                        )
                        .map_err(memory_error)?;
                    Ok((agent_id, state, manifest))
                })
                .collect()
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let rows = block_on(async move {
                    sqlx::query("SELECT id, state, manifest FROM agents")
                        .fetch_all(&*pool)
                        .await
                })
                .map_err(memory_error)?;
                rows.into_iter()
                    .map(|row| {
                        let manifest_blob: Vec<u8> =
                            row.try_get("manifest").map_err(memory_error)?;
                        let manifest = rmp_serde::from_slice::<
                            openparlant_types::agent::AgentManifest,
                        >(&manifest_blob)
                        .map_err(memory_error)?;
                        Ok((
                            row.try_get("id").map_err(memory_error)?,
                            row.try_get("state").map_err(memory_error)?,
                            manifest,
                        ))
                    })
                    .collect()
            }
        }
    }

    fn load_agent_usage_totals(&self) -> SiliCrewResult<HashMap<String, i64>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut stmt = conn
                    .prepare("SELECT agent_id, COALESCE(SUM(input_tokens + output_tokens), 0) FROM usage_events GROUP BY agent_id")
                    .map_err(memory_error)?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                    })
                    .map_err(memory_error)?;
                rows.map(|row| row.map_err(memory_error)).collect()
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let rows = block_on(async move {
                    sqlx::query(
                        "SELECT agent_id, COALESCE(SUM(input_tokens + output_tokens), 0) AS total_tokens
                         FROM usage_events GROUP BY agent_id",
                    )
                    .fetch_all(&*pool)
                    .await
                })
                .map_err(memory_error)?;
                rows.into_iter()
                    .map(|row| {
                        Ok((
                            row.try_get("agent_id").map_err(memory_error)?,
                            row.try_get("total_tokens").map_err(memory_error)?,
                        ))
                    })
                    .collect()
            }
        }
    }

    fn company_agent_stats(&self) -> SiliCrewResult<HashMap<String, (i64, i64, i64)>> {
        let manifests = self.load_agent_manifest_rows()?;
        let usage_totals = self.load_agent_usage_totals()?;
        let mut stats = HashMap::new();
        for (agent_id, state, manifest) in manifests {
            let Some(tenant_id) = manifest_tenant_id(&manifest) else {
                continue;
            };
            let entry = stats.entry(tenant_id).or_insert((0, 0, 0));
            entry.0 += 1;
            if state.contains("running") {
                entry.1 += 1;
            }
            entry.2 += usage_totals.get(&agent_id).copied().unwrap_or_default();
        }
        Ok(stats)
    }

    fn agent_creator_counts(
        &self,
        tenant_id: Option<&str>,
    ) -> SiliCrewResult<HashMap<String, i64>> {
        let manifests = self.load_agent_manifest_rows()?;
        let mut counts = HashMap::new();
        for (_agent_id, _state, manifest) in manifests {
            if let Some(filter_tenant) = tenant_id {
                if manifest_tenant_id(&manifest).as_deref() != Some(filter_tenant) {
                    continue;
                }
            }
            let Some(creator_user_id) = manifest_metadata_string(&manifest, "creator_user_id")
            else {
                continue;
            };
            *counts.entry(creator_user_id).or_insert(0) += 1;
        }
        Ok(counts)
    }
}
