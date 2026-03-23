//! Knowledge graph backed by the shared SQL database.
//!
//! Stores entities and relations with support for graph pattern queries.

use crate::db::{block_on, SharedDb};
use chrono::Utc;
use openparlant_types::error::{SiliCrewError, SiliCrewResult};
use openparlant_types::memory::{
    Entity, EntityType, GraphMatch, GraphPattern, Relation, RelationType,
};
#[cfg(test)]
use rusqlite::Connection;
use sqlx::{Postgres, QueryBuilder, Row};
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
use uuid::Uuid;

/// Knowledge graph store backed by the shared SQL database.
#[derive(Clone)]
pub struct KnowledgeStore {
    db: SharedDb,
}

impl KnowledgeStore {
    /// Create a new knowledge store wrapping the given connection.
    pub fn new(db: impl Into<SharedDb>) -> Self {
        Self { db: db.into() }
    }

    /// Add an entity to the knowledge graph.
    pub fn add_entity(&self, entity: Entity) -> SiliCrewResult<String> {
        let id = if entity.id.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            entity.id.clone()
        };
        let entity_type_str = serde_json::to_string(&entity.entity_type)
            .map_err(|e| SiliCrewError::Serialization(e.to_string()))?;
        let props_str = serde_json::to_string(&entity.properties)
            .map_err(|e| SiliCrewError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO entities (id, entity_type, name, properties, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                     ON CONFLICT(id) DO UPDATE SET name = ?3, properties = ?4, updated_at = ?5",
                    rusqlite::params![id, entity_type_str, entity.name, props_str, now],
                )
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let entity_id = id.clone();
                let name = entity.name;
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO entities (id, entity_type, name, properties, created_at, updated_at)
                         VALUES ($1, $2, $3, $4, $5, $5)
                         ON CONFLICT(id) DO UPDATE SET
                            name = EXCLUDED.name,
                            properties = EXCLUDED.properties,
                            updated_at = EXCLUDED.updated_at",
                    )
                    .bind(entity_id)
                    .bind(entity_type_str)
                    .bind(name)
                    .bind(props_str)
                    .bind(now)
                    .execute(&*pool)
                    .await
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
        }
        Ok(id)
    }

    /// Add a relation between two entities.
    pub fn add_relation(&self, relation: Relation) -> SiliCrewResult<String> {
        let id = Uuid::new_v4().to_string();
        let rel_type_str = serde_json::to_string(&relation.relation)
            .map_err(|e| SiliCrewError::Serialization(e.to_string()))?;
        let props_str = serde_json::to_string(&relation.properties)
            .map_err(|e| SiliCrewError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                conn.execute(
                    "INSERT INTO relations (id, source_entity, relation_type, target_entity, properties, confidence, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        id,
                        relation.source,
                        rel_type_str,
                        relation.target,
                        props_str,
                        relation.confidence as f64,
                        now,
                    ],
                )
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let relation_id = id.clone();
                let source = relation.source;
                let target = relation.target;
                let confidence = relation.confidence as f64;
                block_on(async move {
                    sqlx::query(
                        "INSERT INTO relations (id, source_entity, relation_type, target_entity, properties, confidence, created_at)
                         VALUES ($1, $2, $3, $4, $5, $6, $7)",
                    )
                    .bind(relation_id)
                    .bind(source)
                    .bind(rel_type_str)
                    .bind(target)
                    .bind(props_str)
                    .bind(confidence)
                    .bind(now)
                    .execute(&*pool)
                    .await
                })
                .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
            }
        }
        Ok(id)
    }

    /// Query the knowledge graph with a pattern.
    pub fn query_graph(&self, pattern: GraphPattern) -> SiliCrewResult<Vec<GraphMatch>> {
        match &self.db {
            SharedDb::Sqlite(conn) => {
                let conn = conn
                    .lock()
                    .map_err(|e| SiliCrewError::Internal(e.to_string()))?;
                let mut sql = String::from(
                    "SELECT
                        s.id, s.entity_type, s.name, s.properties, s.created_at, s.updated_at,
                        r.id, r.source_entity, r.relation_type, r.target_entity, r.properties, r.confidence, r.created_at,
                        t.id, t.entity_type, t.name, t.properties, t.created_at, t.updated_at
                     FROM relations r
                     JOIN entities s ON r.source_entity = s.id
                     JOIN entities t ON r.target_entity = t.id
                     WHERE 1=1",
                );
                let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
                let mut idx = 1;
                if let Some(ref source) = pattern.source {
                    sql.push_str(&format!(" AND (s.id = ?{} OR s.name = ?{})", idx, idx + 1));
                    params.push(Box::new(source.clone()));
                    params.push(Box::new(source.clone()));
                    idx += 2;
                }
                if let Some(ref relation) = pattern.relation {
                    let rel_str = serde_json::to_string(relation)
                        .map_err(|e| SiliCrewError::Serialization(e.to_string()))?;
                    sql.push_str(&format!(" AND r.relation_type = ?{idx}"));
                    params.push(Box::new(rel_str));
                    idx += 1;
                }
                if let Some(ref target) = pattern.target {
                    sql.push_str(&format!(" AND (t.id = ?{} OR t.name = ?{})", idx, idx + 1));
                    params.push(Box::new(target.clone()));
                    params.push(Box::new(target.clone()));
                }
                sql.push_str(" LIMIT 100");

                let mut stmt = conn
                    .prepare(&sql)
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();
                let rows = stmt
                    .query_map(param_refs.as_slice(), |row| {
                        Ok(RawGraphRow {
                            s_id: row.get(0)?,
                            s_type: row.get(1)?,
                            s_name: row.get(2)?,
                            s_props: row.get(3)?,
                            s_created: row.get(4)?,
                            s_updated: row.get(5)?,
                            r_id: row.get(6)?,
                            r_source: row.get(7)?,
                            r_type: row.get(8)?,
                            r_target: row.get(9)?,
                            r_props: row.get(10)?,
                            r_confidence: row.get(11)?,
                            r_created: row.get(12)?,
                            t_id: row.get(13)?,
                            t_type: row.get(14)?,
                            t_name: row.get(15)?,
                            t_props: row.get(16)?,
                            t_created: row.get(17)?,
                            t_updated: row.get(18)?,
                        })
                    })
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let mut matches = Vec::new();
                for row_result in rows {
                    matches.push(raw_graph_row_to_match(
                        row_result.map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                    ));
                }
                Ok(matches)
            }
            SharedDb::Postgres(pool) => {
                let pool = Arc::clone(pool);
                let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new(
                    "SELECT
                        s.id, s.entity_type, s.name, s.properties, s.created_at, s.updated_at,
                        r.id, r.source_entity, r.relation_type, r.target_entity, r.properties, r.confidence, r.created_at,
                        t.id, t.entity_type, t.name, t.properties, t.created_at, t.updated_at
                     FROM relations r
                     JOIN entities s ON r.source_entity = s.id
                     JOIN entities t ON r.target_entity = t.id
                     WHERE 1=1",
                );
                if let Some(ref source) = pattern.source {
                    qb.push(" AND (s.id = ");
                    qb.push_bind(source);
                    qb.push(" OR s.name = ");
                    qb.push_bind(source);
                    qb.push(")");
                }
                if let Some(ref relation) = pattern.relation {
                    let rel_str = serde_json::to_string(relation)
                        .map_err(|e| SiliCrewError::Serialization(e.to_string()))?;
                    qb.push(" AND r.relation_type = ");
                    qb.push_bind(rel_str);
                }
                if let Some(ref target) = pattern.target {
                    qb.push(" AND (t.id = ");
                    qb.push_bind(target);
                    qb.push(" OR t.name = ");
                    qb.push_bind(target);
                    qb.push(")");
                }
                qb.push(" LIMIT 100");
                let rows = block_on(async move { qb.build().fetch_all(&*pool).await })
                    .map_err(|e| SiliCrewError::Memory(e.to_string()))?;
                let mut matches = Vec::with_capacity(rows.len());
                for row in rows {
                    matches.push(raw_graph_row_to_match(RawGraphRow {
                        s_id: row
                            .try_get(0)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        s_type: row
                            .try_get(1)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        s_name: row
                            .try_get(2)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        s_props: row
                            .try_get(3)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        s_created: row
                            .try_get(4)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        s_updated: row
                            .try_get(5)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        r_id: row
                            .try_get(6)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        r_source: row
                            .try_get(7)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        r_type: row
                            .try_get(8)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        r_target: row
                            .try_get(9)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        r_props: row
                            .try_get(10)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        r_confidence: row
                            .try_get(11)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        r_created: row
                            .try_get(12)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        t_id: row
                            .try_get(13)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        t_type: row
                            .try_get(14)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        t_name: row
                            .try_get(15)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        t_props: row
                            .try_get(16)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        t_created: row
                            .try_get(17)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                        t_updated: row
                            .try_get(18)
                            .map_err(|e| SiliCrewError::Memory(e.to_string()))?,
                    }));
                }
                Ok(matches)
            }
        }
    }
}

/// Raw row from a graph query.
struct RawGraphRow {
    s_id: String,
    s_type: String,
    s_name: String,
    s_props: String,
    s_created: String,
    s_updated: String,
    r_id: String,
    r_source: String,
    r_type: String,
    r_target: String,
    r_props: String,
    r_confidence: f64,
    r_created: String,
    t_id: String,
    t_type: String,
    t_name: String,
    t_props: String,
    t_created: String,
    t_updated: String,
}

// Suppress the unused field warning — r_id is part of the schema
impl RawGraphRow {
    #[allow(dead_code)]
    fn relation_id(&self) -> &str {
        &self.r_id
    }
}

fn raw_graph_row_to_match(r: RawGraphRow) -> GraphMatch {
    GraphMatch {
        source: parse_entity(
            &r.s_id,
            &r.s_type,
            &r.s_name,
            &r.s_props,
            &r.s_created,
            &r.s_updated,
        ),
        relation: parse_relation(
            &r.r_source,
            &r.r_type,
            &r.r_target,
            &r.r_props,
            r.r_confidence,
            &r.r_created,
        ),
        target: parse_entity(
            &r.t_id,
            &r.t_type,
            &r.t_name,
            &r.t_props,
            &r.t_created,
            &r.t_updated,
        ),
    }
}

fn parse_entity(
    id: &str,
    etype: &str,
    name: &str,
    props: &str,
    created: &str,
    updated: &str,
) -> Entity {
    let entity_type: EntityType =
        serde_json::from_str(etype).unwrap_or(EntityType::Custom("unknown".to_string()));
    let properties: HashMap<String, serde_json::Value> =
        serde_json::from_str(props).unwrap_or_default();
    let created_at = chrono::DateTime::parse_from_rfc3339(created)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let updated_at = chrono::DateTime::parse_from_rfc3339(updated)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    Entity {
        id: id.to_string(),
        entity_type,
        name: name.to_string(),
        properties,
        created_at,
        updated_at,
    }
}

fn parse_relation(
    source: &str,
    rtype: &str,
    target: &str,
    props: &str,
    confidence: f64,
    created: &str,
) -> Relation {
    let relation: RelationType = serde_json::from_str(rtype).unwrap_or(RelationType::RelatedTo);
    let properties: HashMap<String, serde_json::Value> =
        serde_json::from_str(props).unwrap_or_default();
    let created_at = chrono::DateTime::parse_from_rfc3339(created)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    Relation {
        source: source.to_string(),
        relation,
        target: target.to_string(),
        properties,
        confidence: confidence as f32,
        created_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::run_migrations;

    fn setup() -> KnowledgeStore {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        KnowledgeStore::new(Arc::new(Mutex::new(conn)))
    }

    #[test]
    fn test_add_and_query_entity() {
        let store = setup();
        let id = store
            .add_entity(Entity {
                id: String::new(),
                entity_type: EntityType::Person,
                name: "Alice".to_string(),
                properties: HashMap::new(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .unwrap();
        assert!(!id.is_empty());
    }

    #[test]
    fn test_add_relation_and_query() {
        let store = setup();
        let alice_id = store
            .add_entity(Entity {
                id: "alice".to_string(),
                entity_type: EntityType::Person,
                name: "Alice".to_string(),
                properties: HashMap::new(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .unwrap();
        let company_id = store
            .add_entity(Entity {
                id: "acme".to_string(),
                entity_type: EntityType::Organization,
                name: "Acme Corp".to_string(),
                properties: HashMap::new(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .unwrap();
        store
            .add_relation(Relation {
                source: alice_id.clone(),
                relation: RelationType::WorksAt,
                target: company_id,
                properties: HashMap::new(),
                confidence: 0.95,
                created_at: Utc::now(),
            })
            .unwrap();

        let matches = store
            .query_graph(GraphPattern {
                source: Some(alice_id),
                relation: Some(RelationType::WorksAt),
                target: None,
                max_depth: 1,
            })
            .unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].target.name, "Acme Corp");
    }
}
