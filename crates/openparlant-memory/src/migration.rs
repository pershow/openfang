//! SQLite schema creation and migration.
//!
//! Creates all tables needed by the memory substrate on first boot.

use rusqlite::Connection;

/// Current schema version.
const SCHEMA_VERSION: u32 = 11;

/// Run all migrations to bring the database up to date.
pub fn run_migrations(conn: &Connection) -> Result<(), rusqlite::Error> {
    let current_version = get_schema_version(conn);

    if current_version < 1 {
        migrate_v1(conn)?;
    }

    if current_version < 2 {
        migrate_v2(conn)?;
    }

    if current_version < 3 {
        migrate_v3(conn)?;
    }

    if current_version < 4 {
        migrate_v4(conn)?;
    }

    if current_version < 5 {
        migrate_v5(conn)?;
    }

    if current_version < 6 {
        migrate_v6(conn)?;
    }

    if current_version < 7 {
        migrate_v7(conn)?;
    }

    if current_version < 8 {
        migrate_v8(conn)?;
    }

    if current_version < 9 {
        migrate_v9(conn)?;
    }

    if current_version < 10 {
        migrate_v10(conn)?;
    }

    if current_version < 11 {
        migrate_v11(conn)?;
    }

    set_schema_version(conn, SCHEMA_VERSION)?;
    Ok(())
}

/// Get the current schema version from the database.
fn get_schema_version(conn: &Connection) -> u32 {
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap_or(0)
}

/// Check if a column exists in a table (SQLite has no ADD COLUMN IF NOT EXISTS).
fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    let sql = format!("PRAGMA table_info({})", table);
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return false;
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(1)) else {
        return false;
    };
    let names: Vec<String> = rows.filter_map(|r| r.ok()).collect();
    names.iter().any(|n| n == column)
}

/// Set the schema version in the database.
fn set_schema_version(conn: &Connection, version: u32) -> Result<(), rusqlite::Error> {
    conn.pragma_update(None, "user_version", version)
}

/// Version 1: Create all core tables.
fn migrate_v1(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "
        -- Agent registry
        CREATE TABLE IF NOT EXISTS agents (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            manifest BLOB NOT NULL,
            state TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        -- Session history
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            messages BLOB NOT NULL,
            context_window_tokens INTEGER DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        -- Event log
        CREATE TABLE IF NOT EXISTS events (
            id TEXT PRIMARY KEY,
            source_agent TEXT NOT NULL,
            target TEXT NOT NULL,
            payload BLOB NOT NULL,
            timestamp TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
        CREATE INDEX IF NOT EXISTS idx_events_source ON events(source_agent);

        -- Key-value store (per-agent)
        CREATE TABLE IF NOT EXISTS kv_store (
            agent_id TEXT NOT NULL,
            key TEXT NOT NULL,
            value BLOB NOT NULL,
            version INTEGER NOT NULL DEFAULT 1,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (agent_id, key)
        );

        -- Task queue
        CREATE TABLE IF NOT EXISTS task_queue (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            task_type TEXT NOT NULL,
            payload BLOB NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            priority INTEGER NOT NULL DEFAULT 0,
            scheduled_at TEXT,
            created_at TEXT NOT NULL,
            completed_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_task_status_priority ON task_queue(status, priority DESC);

        -- Semantic memories
        CREATE TABLE IF NOT EXISTS memories (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            content TEXT NOT NULL,
            source TEXT NOT NULL,
            scope TEXT NOT NULL DEFAULT 'episodic',
            confidence REAL NOT NULL DEFAULT 1.0,
            metadata TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            accessed_at TEXT NOT NULL,
            access_count INTEGER NOT NULL DEFAULT 0,
            deleted INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_memories_agent ON memories(agent_id);
        CREATE INDEX IF NOT EXISTS idx_memories_scope ON memories(scope);

        -- Knowledge graph entities
        CREATE TABLE IF NOT EXISTS entities (
            id TEXT PRIMARY KEY,
            entity_type TEXT NOT NULL,
            name TEXT NOT NULL,
            properties TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        -- Knowledge graph relations
        CREATE TABLE IF NOT EXISTS relations (
            id TEXT PRIMARY KEY,
            source_entity TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            target_entity TEXT NOT NULL,
            properties TEXT NOT NULL DEFAULT '{}',
            confidence REAL NOT NULL DEFAULT 1.0,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_relations_source ON relations(source_entity);
        CREATE INDEX IF NOT EXISTS idx_relations_target ON relations(target_entity);
        CREATE INDEX IF NOT EXISTS idx_relations_type ON relations(relation_type);

        -- Migration tracking
        CREATE TABLE IF NOT EXISTS migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL,
            description TEXT
        );

        INSERT OR IGNORE INTO migrations (version, applied_at, description)
        VALUES (1, datetime('now'), 'Initial schema');
        ",
    )?;
    Ok(())
}

/// Version 2: Add collaboration columns to task_queue for agent task delegation.
fn migrate_v2(conn: &Connection) -> Result<(), rusqlite::Error> {
    // SQLite requires one ALTER TABLE per statement; check before adding
    let cols = [
        ("title", "TEXT DEFAULT ''"),
        ("description", "TEXT DEFAULT ''"),
        ("assigned_to", "TEXT DEFAULT ''"),
        ("created_by", "TEXT DEFAULT ''"),
        ("result", "TEXT DEFAULT ''"),
    ];
    for (name, typedef) in &cols {
        if !column_exists(conn, "task_queue", name) {
            conn.execute(
                &format!("ALTER TABLE task_queue ADD COLUMN {} {}", name, typedef),
                [],
            )?;
        }
    }

    conn.execute(
        "INSERT OR IGNORE INTO migrations (version, applied_at, description) VALUES (2, datetime('now'), 'Add collaboration columns to task_queue')",
        [],
    )?;

    Ok(())
}

/// Version 3: Add embedding column to memories table for vector search.
fn migrate_v3(conn: &Connection) -> Result<(), rusqlite::Error> {
    if !column_exists(conn, "memories", "embedding") {
        conn.execute(
            "ALTER TABLE memories ADD COLUMN embedding BLOB DEFAULT NULL",
            [],
        )?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO migrations (version, applied_at, description) VALUES (3, datetime('now'), 'Add embedding column to memories')",
        [],
    )?;
    Ok(())
}

/// Version 4: Add usage_events table for cost tracking and metering.
fn migrate_v4(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS usage_events (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            model TEXT NOT NULL,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cost_usd REAL NOT NULL DEFAULT 0.0,
            tool_calls INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_usage_agent_time ON usage_events(agent_id, timestamp);
        CREATE INDEX IF NOT EXISTS idx_usage_timestamp ON usage_events(timestamp);

        INSERT OR IGNORE INTO migrations (version, applied_at, description)
        VALUES (4, datetime('now'), 'Add usage_events table for cost tracking');
        ",
    )?;
    Ok(())
}

/// Version 5: Add canonical_sessions table for cross-channel persistent memory.
fn migrate_v5(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS canonical_sessions (
            agent_id TEXT PRIMARY KEY,
            messages BLOB NOT NULL,
            compaction_cursor INTEGER NOT NULL DEFAULT 0,
            compacted_summary TEXT,
            updated_at TEXT NOT NULL
        );

        INSERT OR IGNORE INTO migrations (version, applied_at, description)
        VALUES (5, datetime('now'), 'Add canonical_sessions for cross-channel memory');
        ",
    )?;
    Ok(())
}

/// Version 6: Add label column to sessions table.
fn migrate_v6(conn: &Connection) -> Result<(), rusqlite::Error> {
    // Check if column already exists before ALTER (SQLite has no ADD COLUMN IF NOT EXISTS)
    if !column_exists(conn, "sessions", "label") {
        conn.execute("ALTER TABLE sessions ADD COLUMN label TEXT", [])?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO migrations (version, applied_at, description) VALUES (6, datetime('now'), 'Add label column to sessions for human-readable labels')",
        [],
    )?;
    Ok(())
}

/// Version 7: Add paired_devices table for device pairing persistence.
fn migrate_v7(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS paired_devices (
            device_id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            platform TEXT NOT NULL,
            paired_at TEXT NOT NULL,
            last_seen TEXT NOT NULL,
            push_token TEXT
        );

        INSERT OR IGNORE INTO migrations (version, applied_at, description)
        VALUES (7, datetime('now'), 'Add paired_devices table for device pairing');
        ",
    )?;
    Ok(())
}

/// Version 8: Add audit_entries table for persistent Merkle audit trail.
fn migrate_v8(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS audit_entries (
            seq INTEGER PRIMARY KEY,
            timestamp TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            action TEXT NOT NULL,
            detail TEXT NOT NULL,
            outcome TEXT NOT NULL,
            prev_hash TEXT NOT NULL,
            hash TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_audit_agent ON audit_entries(agent_id);
        CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_entries(timestamp);
        CREATE INDEX IF NOT EXISTS idx_audit_action ON audit_entries(action);

        INSERT OR IGNORE INTO migrations (version, applied_at, description)
        VALUES (8, datetime('now'), 'Add audit_entries table for persistent Merkle audit trail');
        ",
    )?;
    Ok(())
}

/// Version 9: Add first-batch control-plane tables for scopes, policy, journeys, and traces.
fn migrate_v9(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS control_scopes (
            scope_id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            scope_type TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_control_scopes_type_status
            ON control_scopes(scope_type, status);

        CREATE TABLE IF NOT EXISTS session_bindings (
            binding_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            channel_type TEXT NOT NULL,
            external_user_id TEXT,
            external_chat_id TEXT,
            agent_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            manual_mode INTEGER NOT NULL DEFAULT 0 CHECK (manual_mode IN (0, 1)),
            active_journey_instance_id TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_session_bindings_scope_channel
            ON session_bindings(scope_id, channel_type);
        CREATE INDEX IF NOT EXISTS idx_session_bindings_session
            ON session_bindings(session_id);
        CREATE INDEX IF NOT EXISTS idx_session_bindings_agent
            ON session_bindings(agent_id);
        CREATE INDEX IF NOT EXISTS idx_session_bindings_external
            ON session_bindings(scope_id, channel_type, external_chat_id, external_user_id);

        CREATE TABLE IF NOT EXISTS observations (
            observation_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            name TEXT NOT NULL,
            matcher_type TEXT NOT NULL,
            matcher_config TEXT NOT NULL,
            priority INTEGER NOT NULL DEFAULT 0,
            enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_observations_scope_name
            ON observations(scope_id, name);
        CREATE INDEX IF NOT EXISTS idx_observations_scope_enabled_priority
            ON observations(scope_id, enabled, priority DESC);

        CREATE TABLE IF NOT EXISTS guidelines (
            guideline_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            name TEXT NOT NULL,
            condition_ref TEXT NOT NULL,
            action_text TEXT NOT NULL,
            composition_mode TEXT NOT NULL,
            priority INTEGER NOT NULL DEFAULT 0,
            enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_guidelines_scope_name
            ON guidelines(scope_id, name);
        CREATE INDEX IF NOT EXISTS idx_guidelines_scope_enabled_priority
            ON guidelines(scope_id, enabled, priority DESC);

        CREATE TABLE IF NOT EXISTS journeys (
            journey_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            name TEXT NOT NULL,
            trigger_config TEXT NOT NULL,
            completion_rule TEXT,
            enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_journeys_scope_name
            ON journeys(scope_id, name);
        CREATE INDEX IF NOT EXISTS idx_journeys_scope_enabled
            ON journeys(scope_id, enabled);

        CREATE TABLE IF NOT EXISTS turn_traces (
            trace_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            channel_type TEXT NOT NULL,
            request_message_ref TEXT,
            compiled_context_hash TEXT,
            response_mode TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_turn_traces_session_created
            ON turn_traces(session_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_turn_traces_scope_created
            ON turn_traces(scope_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_turn_traces_agent_created
            ON turn_traces(agent_id, created_at DESC);

        INSERT OR IGNORE INTO migrations (version, applied_at, description)
        VALUES (
            9,
            datetime('now'),
            'Add control-plane seed tables for scopes, bindings, observations, guidelines, journeys, and traces'
        );
        ",
    )?;
    Ok(())
}

/// Version 10: Add journey graph and explainability record tables for the control plane.
fn migrate_v10(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS guideline_relationships (
            relationship_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            from_guideline_id TEXT NOT NULL,
            to_guideline_id TEXT NOT NULL,
            relation_type TEXT NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_guideline_relationships_edge
            ON guideline_relationships(scope_id, from_guideline_id, to_guideline_id, relation_type);
        CREATE INDEX IF NOT EXISTS idx_guideline_relationships_from
            ON guideline_relationships(from_guideline_id);
        CREATE INDEX IF NOT EXISTS idx_guideline_relationships_to
            ON guideline_relationships(to_guideline_id);

        CREATE TABLE IF NOT EXISTS journey_states (
            state_id TEXT PRIMARY KEY,
            journey_id TEXT NOT NULL,
            name TEXT NOT NULL,
            description TEXT,
            required_fields TEXT NOT NULL DEFAULT '[]'
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_journey_states_journey_name
            ON journey_states(journey_id, name);
        CREATE INDEX IF NOT EXISTS idx_journey_states_journey
            ON journey_states(journey_id);

        CREATE TABLE IF NOT EXISTS journey_transitions (
            transition_id TEXT PRIMARY KEY,
            journey_id TEXT NOT NULL,
            from_state_id TEXT NOT NULL,
            to_state_id TEXT NOT NULL,
            condition_config TEXT NOT NULL DEFAULT '{}',
            transition_type TEXT NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_journey_transitions_edge
            ON journey_transitions(journey_id, from_state_id, to_state_id, transition_type);
        CREATE INDEX IF NOT EXISTS idx_journey_transitions_journey_from
            ON journey_transitions(journey_id, from_state_id);
        CREATE INDEX IF NOT EXISTS idx_journey_transitions_to
            ON journey_transitions(to_state_id);

        CREATE TABLE IF NOT EXISTS journey_instances (
            journey_instance_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            journey_id TEXT NOT NULL,
            current_state_id TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            state_payload TEXT NOT NULL DEFAULT '{}',
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_journey_instances_session
            ON journey_instances(session_id);
        CREATE INDEX IF NOT EXISTS idx_journey_instances_scope_status
            ON journey_instances(scope_id, status);
        CREATE INDEX IF NOT EXISTS idx_journey_instances_journey
            ON journey_instances(journey_id);
        CREATE INDEX IF NOT EXISTS idx_journey_instances_current_state
            ON journey_instances(current_state_id);

        CREATE TABLE IF NOT EXISTS policy_match_records (
            record_id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            observation_hits_json TEXT NOT NULL DEFAULT '[]',
            guideline_hits_json TEXT NOT NULL DEFAULT '[]',
            guideline_exclusions_json TEXT NOT NULL DEFAULT '[]'
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_policy_match_records_trace
            ON policy_match_records(trace_id);

        CREATE TABLE IF NOT EXISTS journey_transition_records (
            record_id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            journey_instance_id TEXT NOT NULL,
            before_state_id TEXT,
            after_state_id TEXT,
            decision_json TEXT NOT NULL DEFAULT '{}'
        );
        CREATE INDEX IF NOT EXISTS idx_journey_transition_records_trace
            ON journey_transition_records(trace_id);
        CREATE INDEX IF NOT EXISTS idx_journey_transition_records_instance
            ON journey_transition_records(journey_instance_id);

        CREATE TABLE IF NOT EXISTS tool_authorization_records (
            record_id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            allowed_tools_json TEXT NOT NULL DEFAULT '[]',
            authorization_reasons_json TEXT NOT NULL DEFAULT '{}',
            approval_requirements_json TEXT NOT NULL DEFAULT '{}'
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_tool_authorization_records_trace
            ON tool_authorization_records(trace_id);

        CREATE TABLE IF NOT EXISTS handoff_records (
            handoff_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            reason TEXT NOT NULL,
            summary TEXT,
            status TEXT NOT NULL DEFAULT 'requested',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_handoff_records_session
            ON handoff_records(session_id);
        CREATE INDEX IF NOT EXISTS idx_handoff_records_scope_status
            ON handoff_records(scope_id, status);

        INSERT OR IGNORE INTO migrations (version, applied_at, description)
        VALUES (
            10,
            datetime('now'),
            'Add journey graph and explainability record tables for the control plane'
        );
        ",
    )?;
    Ok(())
}

/// Version 11: Add knowledge/context policy tables and release metadata for the control plane.
fn migrate_v11(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS retrievers (
            retriever_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            name TEXT NOT NULL,
            retriever_type TEXT NOT NULL,
            config_json TEXT NOT NULL DEFAULT '{}',
            enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_retrievers_scope_name
            ON retrievers(scope_id, name);
        CREATE INDEX IF NOT EXISTS idx_retrievers_scope_enabled
            ON retrievers(scope_id, enabled);

        CREATE TABLE IF NOT EXISTS retriever_bindings (
            binding_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            retriever_id TEXT NOT NULL,
            bind_type TEXT NOT NULL,
            bind_ref TEXT NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_retriever_bindings_unique
            ON retriever_bindings(scope_id, retriever_id, bind_type, bind_ref);
        CREATE INDEX IF NOT EXISTS idx_retriever_bindings_scope_bind
            ON retriever_bindings(scope_id, bind_type, bind_ref);
        CREATE INDEX IF NOT EXISTS idx_retriever_bindings_retriever
            ON retriever_bindings(retriever_id);

        CREATE TABLE IF NOT EXISTS glossary_terms (
            term_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            name TEXT NOT NULL,
            description TEXT NOT NULL,
            synonyms_json TEXT NOT NULL DEFAULT '[]',
            enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_glossary_terms_scope_name
            ON glossary_terms(scope_id, name);
        CREATE INDEX IF NOT EXISTS idx_glossary_terms_scope_enabled
            ON glossary_terms(scope_id, enabled);

        CREATE TABLE IF NOT EXISTS context_variables (
            variable_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            name TEXT NOT NULL,
            value_source_type TEXT NOT NULL,
            value_source_config TEXT NOT NULL DEFAULT '{}',
            visibility_rule TEXT,
            enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_context_variables_scope_name
            ON context_variables(scope_id, name);
        CREATE INDEX IF NOT EXISTS idx_context_variables_scope_enabled
            ON context_variables(scope_id, enabled);
        CREATE INDEX IF NOT EXISTS idx_context_variables_source_type
            ON context_variables(value_source_type);

        CREATE TABLE IF NOT EXISTS canned_responses (
            response_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            name TEXT NOT NULL,
            template_text TEXT NOT NULL,
            trigger_rule TEXT,
            enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_canned_responses_scope_name
            ON canned_responses(scope_id, name);
        CREATE INDEX IF NOT EXISTS idx_canned_responses_scope_enabled
            ON canned_responses(scope_id, enabled);

        CREATE TABLE IF NOT EXISTS tool_exposure_policies (
            policy_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            tool_name TEXT NOT NULL,
            skill_ref TEXT,
            observation_ref TEXT,
            journey_state_ref TEXT,
            guideline_ref TEXT,
            approval_mode TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
        );
        CREATE INDEX IF NOT EXISTS idx_tool_exposure_policies_scope_tool
            ON tool_exposure_policies(scope_id, tool_name);
        CREATE INDEX IF NOT EXISTS idx_tool_exposure_policies_scope_enabled
            ON tool_exposure_policies(scope_id, enabled);
        CREATE INDEX IF NOT EXISTS idx_tool_exposure_policies_scope_approval
            ON tool_exposure_policies(scope_id, approval_mode);

        CREATE TABLE IF NOT EXISTS control_releases (
            release_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            version TEXT NOT NULL,
            status TEXT NOT NULL,
            published_by TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_control_releases_scope_version
            ON control_releases(scope_id, version);
        CREATE INDEX IF NOT EXISTS idx_control_releases_scope_status
            ON control_releases(scope_id, status);
        CREATE INDEX IF NOT EXISTS idx_control_releases_created
            ON control_releases(created_at DESC);

        INSERT OR IGNORE INTO migrations (version, applied_at, description)
        VALUES (
            11,
            datetime('now'),
            'Add knowledge, tool policy, and release tables for the control plane'
        );
        ",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migration_creates_tables() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        // Verify tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"agents".to_string()));
        assert!(tables.contains(&"sessions".to_string()));
        assert!(tables.contains(&"kv_store".to_string()));
        assert!(tables.contains(&"memories".to_string()));
        assert!(tables.contains(&"entities".to_string()));
        assert!(tables.contains(&"relations".to_string()));
        assert!(tables.contains(&"control_scopes".to_string()));
        assert!(tables.contains(&"session_bindings".to_string()));
        assert!(tables.contains(&"observations".to_string()));
        assert!(tables.contains(&"guidelines".to_string()));
        assert!(tables.contains(&"journeys".to_string()));
        assert!(tables.contains(&"turn_traces".to_string()));
        assert!(tables.contains(&"guideline_relationships".to_string()));
        assert!(tables.contains(&"journey_states".to_string()));
        assert!(tables.contains(&"journey_transitions".to_string()));
        assert!(tables.contains(&"journey_instances".to_string()));
        assert!(tables.contains(&"policy_match_records".to_string()));
        assert!(tables.contains(&"journey_transition_records".to_string()));
        assert!(tables.contains(&"tool_authorization_records".to_string()));
        assert!(tables.contains(&"handoff_records".to_string()));
        assert!(tables.contains(&"retrievers".to_string()));
        assert!(tables.contains(&"retriever_bindings".to_string()));
        assert!(tables.contains(&"glossary_terms".to_string()));
        assert!(tables.contains(&"context_variables".to_string()));
        assert!(tables.contains(&"canned_responses".to_string()));
        assert!(tables.contains(&"tool_exposure_policies".to_string()));
        assert!(tables.contains(&"control_releases".to_string()));
    }

    #[test]
    fn test_migration_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap(); // Should not error
    }
}
