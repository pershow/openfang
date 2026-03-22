use crate::store::{ControlStore, PlazaCommentRecord, PlazaPostRecord};
use chrono::{DateTime, Utc};
use openparlant_memory::db::{block_on, SharedDb};
use openparlant_types::error::{OpenFangError, OpenFangResult};
use rusqlite::params;
use sqlx::Row;
use std::collections::HashMap;

fn parse_timestamp(value: &str) -> OpenFangResult<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
                .map(|dt| dt.and_utc())
        })
        .map_err(memory_error)
}

fn memory_error<E: std::fmt::Display>(error: E) -> OpenFangError {
    OpenFangError::Memory(error.to_string())
}

fn post_from_sqlite_row(
    row: (String, String, String, String, String, Option<String>, i32, i32, String),
) -> OpenFangResult<PlazaPostRecord> {
    Ok(PlazaPostRecord {
        id: row.0,
        author_id: row.1,
        author_type: row.2,
        author_name: row.3,
        content: row.4,
        tenant_id: row.5,
        likes_count: row.6,
        comments_count: row.7,
        created_at: parse_timestamp(&row.8)?,
    })
}

fn post_row_sqlite(
    row: &rusqlite::Row<'_>,
) -> Result<(String, String, String, String, String, Option<String>, i32, i32, String), rusqlite::Error>
{
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, String>(3)?,
        row.get::<_, String>(4)?,
        row.get::<_, Option<String>>(5)?,
        row.get::<_, i32>(6)?,
        row.get::<_, i32>(7)?,
        row.get::<_, String>(8)?,
    ))
}

fn post_from_pg_row(row: sqlx::postgres::PgRow) -> OpenFangResult<PlazaPostRecord> {
    Ok(PlazaPostRecord {
        id: row.try_get("id").map_err(memory_error)?,
        author_id: row.try_get("author_id").map_err(memory_error)?,
        author_type: row.try_get("author_type").map_err(memory_error)?,
        author_name: row.try_get("author_name").map_err(memory_error)?,
        content: row.try_get("content").map_err(memory_error)?,
        tenant_id: row.try_get("tenant_id").map_err(memory_error)?,
        likes_count: row.try_get("likes_count").map_err(memory_error)?,
        comments_count: row.try_get("comments_count").map_err(memory_error)?,
        created_at: parse_timestamp(&row.try_get::<String, _>("created_at").map_err(memory_error)?)?,
    })
}

fn comment_from_sqlite_row(
    row: (String, String, String, String, String, String, String),
) -> OpenFangResult<PlazaCommentRecord> {
    Ok(PlazaCommentRecord {
        id: row.0,
        post_id: row.1,
        author_id: row.2,
        author_type: row.3,
        author_name: row.4,
        content: row.5,
        created_at: parse_timestamp(&row.6)?,
    })
}

fn comment_from_pg_row(row: sqlx::postgres::PgRow) -> OpenFangResult<PlazaCommentRecord> {
    Ok(PlazaCommentRecord {
        id: row.try_get("id").map_err(memory_error)?,
        post_id: row.try_get("post_id").map_err(memory_error)?,
        author_id: row.try_get("author_id").map_err(memory_error)?,
        author_type: row.try_get("author_type").map_err(memory_error)?,
        author_name: row.try_get("author_name").map_err(memory_error)?,
        content: row.try_get("content").map_err(memory_error)?,
        created_at: parse_timestamp(&row.try_get::<String, _>("created_at").map_err(memory_error)?)?,
    })
}

impl ControlStore {
    pub fn list_plaza_posts(
        &self,
        tenant_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> OpenFangResult<Vec<PlazaPostRecord>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn.lock().map_err(|e| OpenFangError::Internal(e.to_string()))?;
                let (sql, bind_tenant) = if tenant_id.is_some() {
                    (
                        "SELECT id, author_id, author_type, author_name, content, tenant_id, likes_count, comments_count, created_at
                         FROM plaza_posts WHERE tenant_id = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
                        true,
                    )
                } else {
                    (
                        "SELECT id, author_id, author_type, author_name, content, tenant_id, likes_count, comments_count, created_at
                         FROM plaza_posts ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
                        false,
                    )
                };
                let mut stmt = conn.prepare(sql).map_err(memory_error)?;
                let rows = if bind_tenant {
                    stmt.query_map(
                        params![tenant_id, limit as i64, offset as i64],
                        post_row_sqlite,
                    )
                } else {
                    stmt.query_map(params![limit as i64, offset as i64], post_row_sqlite)
                }
                .map_err(memory_error)?;
                rows.map(|row| post_from_sqlite_row(row.map_err(memory_error)?))
                    .collect()
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let tenant_id = tenant_id.map(ToOwned::to_owned);
                block_on(async move {
                    let rows = if let Some(tenant_id) = tenant_id {
                        sqlx::query(
                            "SELECT id, author_id, author_type, author_name, content, tenant_id, likes_count, comments_count, created_at
                             FROM plaza_posts WHERE tenant_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
                        )
                        .bind(tenant_id)
                        .bind(limit as i64)
                        .bind(offset as i64)
                        .fetch_all(&*pool)
                        .await?
                    } else {
                        sqlx::query(
                            "SELECT id, author_id, author_type, author_name, content, tenant_id, likes_count, comments_count, created_at
                             FROM plaza_posts ORDER BY created_at DESC LIMIT $1 OFFSET $2",
                        )
                        .bind(limit as i64)
                        .bind(offset as i64)
                        .fetch_all(&*pool)
                        .await?
                    };
                    rows.into_iter()
                        .map(|row| {
                            post_from_pg_row(row).map_err(|e| {
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

    pub fn get_plaza_post(&self, post_id: &str) -> OpenFangResult<Option<PlazaPostRecord>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn.lock().map_err(|e| OpenFangError::Internal(e.to_string()))?;
                let row = conn.query_row(
                    "SELECT id, author_id, author_type, author_name, content, tenant_id, likes_count, comments_count, created_at
                     FROM plaza_posts WHERE id = ?1",
                    params![post_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, i32>(6)?,
                            row.get::<_, i32>(7)?,
                            row.get::<_, String>(8)?,
                        ))
                    },
                );
                match row {
                    Ok(row) => Ok(Some(post_from_sqlite_row(row)?)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(memory_error(e)),
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let post_id = post_id.to_string();
                let row = block_on(async move {
                    sqlx::query(
                        "SELECT id, author_id, author_type, author_name, content, tenant_id, likes_count, comments_count, created_at
                         FROM plaza_posts WHERE id = $1",
                    )
                    .bind(post_id)
                    .fetch_optional(&*pool)
                    .await
                })
                .map_err(memory_error)?;
                match row {
                    Some(row) => Ok(Some(post_from_pg_row(row)?)),
                    None => Ok(None),
                }
            }
        }
    }

    pub fn create_plaza_post(&self, post: &PlazaPostRecord) -> OpenFangResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn.lock().map_err(|e| OpenFangError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO plaza_posts (id, author_id, author_type, author_name, content, tenant_id, likes_count, comments_count, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        post.id,
                        post.author_id,
                        post.author_type,
                        post.author_name,
                        post.content,
                        post.tenant_id,
                        post.likes_count,
                        post.comments_count,
                        post.created_at.to_rfc3339(),
                    ],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let post = post.clone();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO plaza_posts (id, author_id, author_type, author_name, content, tenant_id, likes_count, comments_count, created_at)
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
                    )
                    .bind(post.id)
                    .bind(post.author_id)
                    .bind(post.author_type)
                    .bind(post.author_name)
                    .bind(post.content)
                    .bind(post.tenant_id)
                    .bind(post.likes_count)
                    .bind(post.comments_count)
                    .bind(post.created_at.to_rfc3339())
                    .execute(&*pool)
                    .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    pub fn delete_plaza_post(&self, post_id: &str) -> OpenFangResult<bool> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn.lock().map_err(|e| OpenFangError::Internal(e.to_string()))?;
                conn.execute("DELETE FROM plaza_comments WHERE post_id = ?1", params![post_id])
                    .map_err(memory_error)?;
                conn.execute("DELETE FROM plaza_likes WHERE post_id = ?1", params![post_id])
                    .map_err(memory_error)?;
                Ok(conn
                    .execute("DELETE FROM plaza_posts WHERE id = ?1", params![post_id])
                    .map_err(memory_error)?
                    > 0)
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let post_id = post_id.to_string();
                let result = block_on(async move {
                    sqlx::query("DELETE FROM plaza_comments WHERE post_id = $1")
                        .bind(&post_id)
                        .execute(&*pool)
                        .await?;
                    sqlx::query("DELETE FROM plaza_likes WHERE post_id = $1")
                        .bind(&post_id)
                        .execute(&*pool)
                        .await?;
                    sqlx::query("DELETE FROM plaza_posts WHERE id = $1")
                        .bind(&post_id)
                        .execute(&*pool)
                        .await
                })
                .map_err(memory_error)?;
                Ok(result.rows_affected() > 0)
            }
        }
    }

    pub fn list_plaza_comments(&self, post_id: &str) -> OpenFangResult<Vec<PlazaCommentRecord>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn.lock().map_err(|e| OpenFangError::Internal(e.to_string()))?;
                let mut stmt = conn.prepare(
                    "SELECT id, post_id, author_id, author_type, author_name, content, created_at
                     FROM plaza_comments WHERE post_id = ?1 ORDER BY created_at ASC",
                )
                .map_err(memory_error)?;
                let rows = stmt
                    .query_map(params![post_id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                        ))
                    })
                    .map_err(memory_error)?;
                rows.map(|row| comment_from_sqlite_row(row.map_err(memory_error)?))
                    .collect()
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let post_id = post_id.to_string();
                block_on(async move {
                    let rows = sqlx::query(
                        "SELECT id, post_id, author_id, author_type, author_name, content, created_at
                         FROM plaza_comments WHERE post_id = $1 ORDER BY created_at ASC",
                    )
                    .bind(post_id)
                    .fetch_all(&*pool)
                    .await?;
                    rows.into_iter()
                        .map(|row| {
                            comment_from_pg_row(row).map_err(|e| {
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

    pub fn create_plaza_comment(&self, comment: &PlazaCommentRecord) -> OpenFangResult<()> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn.lock().map_err(|e| OpenFangError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO plaza_comments (id, post_id, author_id, author_type, author_name, content, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        comment.id,
                        comment.post_id,
                        comment.author_id,
                        comment.author_type,
                        comment.author_name,
                        comment.content,
                        comment.created_at.to_rfc3339(),
                    ],
                )
                .map_err(memory_error)?;
                conn.execute(
                    "UPDATE plaza_posts SET comments_count = comments_count + 1 WHERE id = ?1",
                    params![comment.post_id],
                )
                .map_err(memory_error)?;
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let comment = comment.clone();
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO plaza_comments (id, post_id, author_id, author_type, author_name, content, created_at)
                         VALUES ($1, $2, $3, $4, $5, $6, $7)",
                    )
                    .bind(comment.id)
                    .bind(&comment.post_id)
                    .bind(comment.author_id)
                    .bind(comment.author_type)
                    .bind(comment.author_name)
                    .bind(comment.content)
                    .bind(comment.created_at.to_rfc3339())
                    .execute(&*pool)
                    .await?;
                    sqlx::query("UPDATE plaza_posts SET comments_count = comments_count + 1 WHERE id = $1")
                        .bind(comment.post_id)
                        .execute(&*pool)
                        .await
                })
                .map_err(memory_error)?;
            }
        }
        Ok(())
    }

    pub fn toggle_plaza_like(
        &self,
        post_id: &str,
        author_id: &str,
        author_type: &str,
    ) -> OpenFangResult<bool> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn.lock().map_err(|e| OpenFangError::Internal(e.to_string()))?;
                let existing: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM plaza_likes WHERE post_id = ?1 AND author_id = ?2 AND author_type = ?3",
                        params![post_id, author_id, author_type],
                        |row| row.get(0),
                    )
                    .map_err(memory_error)?;
                if existing > 0 {
                    conn.execute(
                        "DELETE FROM plaza_likes WHERE post_id = ?1 AND author_id = ?2 AND author_type = ?3",
                        params![post_id, author_id, author_type],
                    )
                    .map_err(memory_error)?;
                    conn.execute(
                        "UPDATE plaza_posts SET likes_count = MAX(likes_count - 1, 0) WHERE id = ?1",
                        params![post_id],
                    )
                    .map_err(memory_error)?;
                    Ok(false)
                } else {
                    conn.execute(
                        "INSERT INTO plaza_likes (id, post_id, author_id, author_type, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            uuid::Uuid::new_v4().to_string(),
                            post_id,
                            author_id,
                            author_type,
                            Utc::now().to_rfc3339(),
                        ],
                    )
                    .map_err(memory_error)?;
                    conn.execute(
                        "UPDATE plaza_posts SET likes_count = likes_count + 1 WHERE id = ?1",
                        params![post_id],
                    )
                    .map_err(memory_error)?;
                    Ok(true)
                }
            }
            SharedDb::Postgres(pool) => {
                let pool = pool.clone();
                let post_id = post_id.to_string();
                let author_id = author_id.to_string();
                let author_type = author_type.to_string();
                block_on(async move {
                    let existing = sqlx::query(
                        "SELECT id FROM plaza_likes WHERE post_id = $1 AND author_id = $2 AND author_type = $3 LIMIT 1",
                    )
                    .bind(&post_id)
                    .bind(&author_id)
                    .bind(&author_type)
                    .fetch_optional(&*pool)
                    .await?;
                    if existing.is_some() {
                        sqlx::query(
                            "DELETE FROM plaza_likes WHERE post_id = $1 AND author_id = $2 AND author_type = $3",
                        )
                        .bind(&post_id)
                        .bind(&author_id)
                        .bind(&author_type)
                        .execute(&*pool)
                        .await?;
                        sqlx::query("UPDATE plaza_posts SET likes_count = GREATEST(likes_count - 1, 0) WHERE id = $1")
                            .bind(&post_id)
                            .execute(&*pool)
                            .await?;
                        Ok::<bool, sqlx::Error>(false)
                    } else {
                        sqlx::query(
                            "INSERT INTO plaza_likes (id, post_id, author_id, author_type, created_at)
                             VALUES ($1, $2, $3, $4, $5)",
                        )
                        .bind(uuid::Uuid::new_v4().to_string())
                        .bind(&post_id)
                        .bind(&author_id)
                        .bind(&author_type)
                        .bind(Utc::now().to_rfc3339())
                        .execute(&*pool)
                        .await?;
                        sqlx::query("UPDATE plaza_posts SET likes_count = likes_count + 1 WHERE id = $1")
                            .bind(&post_id)
                            .execute(&*pool)
                            .await?;
                        Ok::<bool, sqlx::Error>(true)
                    }
                })
                .map_err(memory_error)
            }
        }
    }

    pub fn plaza_stats(&self, tenant_id: Option<&str>) -> OpenFangResult<serde_json::Value> {
        let posts = self.list_plaza_posts(tenant_id, 10_000, 0)?;
        let total_posts = posts.len() as i64;
        let today = Utc::now().date_naive();
        let today_posts = posts
            .iter()
            .filter(|post| post.created_at.date_naive() == today)
            .count() as i64;
        let total_comments = posts.iter().map(|post| i64::from(post.comments_count)).sum::<i64>();

        let mut contributors: HashMap<(String, String), i64> = HashMap::new();
        for post in &posts {
            *contributors
                .entry((post.author_name.clone(), post.author_type.clone()))
                .or_insert(0) += 1;
        }
        let mut top_contributors = contributors
            .into_iter()
            .map(|((name, author_type), count)| serde_json::json!({
                "name": name,
                "type": author_type,
                "posts": count,
            }))
            .collect::<Vec<_>>();
        top_contributors.sort_by(|a, b| b["posts"].as_i64().cmp(&a["posts"].as_i64()));
        top_contributors.truncate(5);

        Ok(serde_json::json!({
            "total_posts": total_posts,
            "total_comments": total_comments,
            "today_posts": today_posts,
            "top_contributors": top_contributors,
        }))
    }
}
