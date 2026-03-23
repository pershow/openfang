use crate::store::{ControlStore, NotificationRecord};
use chrono::{DateTime, Utc};
use openparlant_memory::db::{block_on, SharedDb};
use openparlant_types::error::{SiliCrewError, SiliCrewResult};
use rusqlite::params;
use sqlx::Row;

fn memory_error<E: std::fmt::Display>(error: E) -> SiliCrewError {
    SiliCrewError::Memory(error.to_string())
}

fn parse_timestamp(value: &str) -> SiliCrewResult<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").map(|dt| dt.and_utc())
        })
        .map_err(memory_error)
}

fn notification_row_sqlite(
    row: &rusqlite::Row<'_>,
) -> Result<
    (
        String,
        Option<String>,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
    ),
    rusqlite::Error,
> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, Option<String>>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, String>(3)?,
        row.get::<_, String>(4)?,
        row.get::<_, String>(5)?,
        row.get::<_, Option<String>>(6)?,
        row.get::<_, Option<String>>(7)?,
        row.get::<_, Option<String>>(8)?,
        row.get::<_, Option<String>>(9)?,
        row.get::<_, String>(10)?,
        row.get::<_, Option<String>>(11)?,
    ))
}

fn notification_from_sqlite_row(
    row: (
        String,
        Option<String>,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
    ),
) -> SiliCrewResult<NotificationRecord> {
    Ok(NotificationRecord {
        id: row.0,
        tenant_id: row.1,
        user_id: row.2,
        notification_type: row.3,
        category: row.4,
        title: row.5,
        body: row.6,
        link: row.7,
        sender_id: row.8,
        sender_name: row.9,
        created_at: parse_timestamp(&row.10)?,
        read_at: row.11.as_deref().map(parse_timestamp).transpose()?,
    })
}

fn notification_from_pg_row(row: sqlx::postgres::PgRow) -> SiliCrewResult<NotificationRecord> {
    Ok(NotificationRecord {
        id: row.try_get("id").map_err(memory_error)?,
        tenant_id: row.try_get("tenant_id").map_err(memory_error)?,
        user_id: row.try_get("user_id").map_err(memory_error)?,
        notification_type: row.try_get("type").map_err(memory_error)?,
        category: row.try_get("category").map_err(memory_error)?,
        title: row.try_get("title").map_err(memory_error)?,
        body: row.try_get("body").map_err(memory_error)?,
        link: row.try_get("link").map_err(memory_error)?,
        sender_id: row.try_get("sender_id").map_err(memory_error)?,
        sender_name: row.try_get("sender_name").map_err(memory_error)?,
        created_at: parse_timestamp(
            &row.try_get::<String, _>("created_at")
                .map_err(memory_error)?,
        )?,
        read_at: row
            .try_get::<Option<String>, _>("read_at")
            .map_err(memory_error)?
            .as_deref()
            .map(parse_timestamp)
            .transpose()?,
    })
}

impl ControlStore {
    pub fn create_notification(&self, notification: &NotificationRecord) -> SiliCrewResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO notifications (id, tenant_id, user_id, type, category, title, body, link, sender_id, sender_name, created_at, read_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        notification.id,
                        notification.tenant_id,
                        notification.user_id,
                        notification.notification_type,
                        notification.category,
                        notification.title,
                        notification.body,
                        notification.link,
                        notification.sender_id,
                        notification.sender_name,
                        notification.created_at.to_rfc3339(),
                        notification.read_at.map(|dt| dt.to_rfc3339()),
                    ],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let notification = notification.clone();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO notifications (id, tenant_id, user_id, type, category, title, body, link, sender_id, sender_name, created_at, read_at)
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
                    )
                    .bind(notification.id)
                    .bind(notification.tenant_id)
                    .bind(notification.user_id)
                    .bind(notification.notification_type)
                    .bind(notification.category)
                    .bind(notification.title)
                    .bind(notification.body)
                    .bind(notification.link)
                    .bind(notification.sender_id)
                    .bind(notification.sender_name)
                    .bind(notification.created_at.to_rfc3339())
                    .bind(notification.read_at.map(|dt| dt.to_rfc3339()))
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    pub fn list_notifications(
        &self,
        user_id: &str,
        category: Option<&str>,
        limit: usize,
    ) -> SiliCrewResult<Vec<NotificationRecord>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let sql = if category.is_some() && category != Some("all") {
                    "SELECT id, tenant_id, user_id, type, category, title, body, link, sender_id, sender_name, created_at, read_at
                     FROM notifications WHERE user_id = ?1 AND category = ?2 ORDER BY created_at DESC LIMIT ?3"
                } else {
                    "SELECT id, tenant_id, user_id, type, category, title, body, link, sender_id, sender_name, created_at, read_at
                     FROM notifications WHERE user_id = ?1 ORDER BY created_at DESC LIMIT ?2"
                };
                let mut stmt = conn.prepare(sql).map_err(memory_error)?;
                let rows = if let Some(category) = category.filter(|value| *value != "all") {
                    stmt.query_map(
                        params![user_id, category, limit as i64],
                        notification_row_sqlite,
                    )
                } else {
                    stmt.query_map(params![user_id, limit as i64], notification_row_sqlite)
                }
                .map_err(memory_error)?;
                rows.map(|row| notification_from_sqlite_row(row.map_err(memory_error)?))
                    .collect()
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let user_id = user_id.to_string();
                let category = category.map(ToOwned::to_owned);
                block_on(async move {
                    let rows = if let Some(category) = category.filter(|value| value != "all") {
                        sqlx::query(
                            "SELECT id, tenant_id, user_id, type, category, title, body, link, sender_id, sender_name, created_at, read_at
                             FROM notifications WHERE user_id = $1 AND category = $2 ORDER BY created_at DESC LIMIT $3",
                        )
                        .bind(user_id)
                        .bind(category)
                        .bind(limit as i64)
                        .fetch_all(&*pool)
                        .await?
                    } else {
                        sqlx::query(
                            "SELECT id, tenant_id, user_id, type, category, title, body, link, sender_id, sender_name, created_at, read_at
                             FROM notifications WHERE user_id = $1 ORDER BY created_at DESC LIMIT $2",
                        )
                        .bind(user_id)
                        .bind(limit as i64)
                        .fetch_all(&*pool)
                        .await?
                    };
                    rows.into_iter()
                        .map(|row| {
                            notification_from_pg_row(row).map_err(|e| {
                                sqlx::Error::Decode(Box::new(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    e.to_string(),
                                )))
                            })
                        })
                        .collect::<Result<Vec<_>, sqlx::Error>>()
                })
                .map_err(memory_error)
            }
        }
    }

    pub fn unread_notification_count(&self, user_id: &str) -> SiliCrewResult<i64> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.query_row(
                    "SELECT COUNT(*) FROM notifications WHERE user_id = ?1 AND read_at IS NULL",
                    params![user_id],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(memory_error)
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let user_id = user_id.to_string();
                block_on(async move {
                    sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*) FROM notifications WHERE user_id = $1 AND read_at IS NULL",
                    )
                    .bind(user_id)
                    .fetch_one(&*pool)
                    .await
                })
                .map_err(memory_error)
            }
        }
    }

    pub fn mark_notification_read(
        &self,
        notification_id: &str,
        user_id: &str,
    ) -> SiliCrewResult<bool> {
        let now = Utc::now().to_rfc3339();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                Ok(conn
                    .execute(
                        "UPDATE notifications SET read_at = ?1 WHERE id = ?2 AND user_id = ?3 AND read_at IS NULL",
                        params![now, notification_id, user_id],
                    )
                    .map_err(memory_error)?
                    > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let notification_id = notification_id.to_string();
                let user_id = user_id.to_string();
                let affected = block_on(async move {
                    sqlx::query(
                        "UPDATE notifications SET read_at = $1 WHERE id = $2 AND user_id = $3 AND read_at IS NULL",
                    )
                    .bind(now)
                    .bind(notification_id)
                    .bind(user_id)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
                Ok(affected.rows_affected() > 0)
            }
        }
    }

    pub fn mark_all_notifications_read(&self, user_id: &str) -> SiliCrewResult<u64> {
        let now = Utc::now().to_rfc3339();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let changed = conn
                    .execute(
                        "UPDATE notifications SET read_at = ?1 WHERE user_id = ?2 AND read_at IS NULL",
                        params![now, user_id],
                    )
                    .map_err(memory_error)?;
                Ok(changed as u64)
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let user_id = user_id.to_string();
                let affected = block_on(async move {
                    sqlx::query(
                        "UPDATE notifications SET read_at = $1 WHERE user_id = $2 AND read_at IS NULL",
                    )
                    .bind(now)
                    .bind(user_id)
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
                Ok(affected.rows_affected())
            }
        }
    }
}
