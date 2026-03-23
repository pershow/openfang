//! SQLite schema creation and migration.
//!
//! Creates all tables needed by the memory substrate on first boot.

use rusqlite::Connection;
use sqlx::PgPool;

/// Current schema version.
const SCHEMA_VERSION: u32 = 22;

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

    if current_version < 12 {
        migrate_v12(conn)?;
    }
    if current_version < 13 {
        migrate_v13(conn)?;
    }
    if current_version < 14 {
        migrate_v14(conn)?;
    }
    if current_version < 15 {
        migrate_v15(conn)?;
    }
    if current_version < 16 {
        migrate_v16(conn)?;
    }
    if current_version < 17 {
        migrate_v17(conn)?;
    }
    if current_version < 18 {
        migrate_v18(conn)?;
    }
    if current_version < 19 {
        migrate_v19(conn)?;
    }
    if current_version < 20 {
        migrate_v20(conn)?;
    }
    if current_version < 21 {
        migrate_v21(conn)?;
    }
    if current_version < 22 {
        migrate_v22(conn)?;
    }

    set_schema_version(conn, SCHEMA_VERSION)?;
    Ok(())
}

/// Create or update the PostgreSQL schema used by the runtime and control plane.
pub async fn run_postgres_migrations(pool: &PgPool) -> Result<(), sqlx::Error> {
    let statements = [
        "CREATE TABLE IF NOT EXISTS agents (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            manifest BYTEA NOT NULL,
            state TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            session_id TEXT NOT NULL DEFAULT '',
            identity TEXT NOT NULL DEFAULT '{}'
        )",
        "CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            messages BYTEA NOT NULL,
            context_window_tokens BIGINT NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            label TEXT
        )",
        "CREATE TABLE IF NOT EXISTS events (
            id TEXT PRIMARY KEY,
            source_agent TEXT NOT NULL,
            target TEXT NOT NULL,
            payload BYTEA NOT NULL,
            timestamp TEXT NOT NULL
        )",
        "CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp)",
        "CREATE INDEX IF NOT EXISTS idx_events_source ON events(source_agent)",
        "CREATE TABLE IF NOT EXISTS kv_store (
            agent_id TEXT NOT NULL,
            key TEXT NOT NULL,
            value BYTEA NOT NULL,
            version BIGINT NOT NULL DEFAULT 1,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (agent_id, key)
        )",
        "CREATE TABLE IF NOT EXISTS task_queue (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            task_type TEXT NOT NULL,
            payload BYTEA NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            priority BIGINT NOT NULL DEFAULT 0,
            scheduled_at TEXT,
            created_at TEXT NOT NULL,
            completed_at TEXT,
            title TEXT NOT NULL DEFAULT '',
            description TEXT NOT NULL DEFAULT '',
            assigned_to TEXT NOT NULL DEFAULT '',
            created_by TEXT NOT NULL DEFAULT '',
            result TEXT NOT NULL DEFAULT ''
        )",
        "CREATE INDEX IF NOT EXISTS idx_task_status_priority ON task_queue(status, priority DESC)",
        "CREATE TABLE IF NOT EXISTS memories (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            content TEXT NOT NULL,
            source TEXT NOT NULL,
            scope TEXT NOT NULL DEFAULT 'episodic',
            confidence DOUBLE PRECISION NOT NULL DEFAULT 1.0,
            metadata TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            accessed_at TEXT NOT NULL,
            access_count BIGINT NOT NULL DEFAULT 0,
            deleted BOOLEAN NOT NULL DEFAULT FALSE,
            embedding BYTEA
        )",
        "CREATE INDEX IF NOT EXISTS idx_memories_agent ON memories(agent_id)",
        "CREATE INDEX IF NOT EXISTS idx_memories_scope ON memories(scope)",
        "CREATE TABLE IF NOT EXISTS entities (
            id TEXT PRIMARY KEY,
            entity_type TEXT NOT NULL,
            name TEXT NOT NULL,
            properties TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        "CREATE TABLE IF NOT EXISTS relations (
            id TEXT PRIMARY KEY,
            source_entity TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            target_entity TEXT NOT NULL,
            properties TEXT NOT NULL DEFAULT '{}',
            confidence DOUBLE PRECISION NOT NULL DEFAULT 1.0,
            created_at TEXT NOT NULL
        )",
        "CREATE INDEX IF NOT EXISTS idx_relations_source ON relations(source_entity)",
        "CREATE INDEX IF NOT EXISTS idx_relations_target ON relations(target_entity)",
        "CREATE INDEX IF NOT EXISTS idx_relations_type ON relations(relation_type)",
        "CREATE TABLE IF NOT EXISTS migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL,
            description TEXT
        )",
        "CREATE TABLE IF NOT EXISTS usage_events (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            model TEXT NOT NULL,
            input_tokens BIGINT NOT NULL DEFAULT 0,
            output_tokens BIGINT NOT NULL DEFAULT 0,
            cost_usd DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            tool_calls BIGINT NOT NULL DEFAULT 0
        )",
        "CREATE INDEX IF NOT EXISTS idx_usage_agent_time ON usage_events(agent_id, timestamp)",
        "CREATE INDEX IF NOT EXISTS idx_usage_timestamp ON usage_events(timestamp)",
        "CREATE TABLE IF NOT EXISTS canonical_sessions (
            agent_id TEXT PRIMARY KEY,
            messages BYTEA NOT NULL,
            compaction_cursor BIGINT NOT NULL DEFAULT 0,
            compacted_summary TEXT,
            updated_at TEXT NOT NULL
        )",
        "CREATE TABLE IF NOT EXISTS paired_devices (
            device_id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            platform TEXT NOT NULL,
            paired_at TEXT NOT NULL,
            last_seen TEXT NOT NULL,
            push_token TEXT
        )",
        "CREATE TABLE IF NOT EXISTS audit_entries (
            seq BIGINT PRIMARY KEY,
            timestamp TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            action TEXT NOT NULL,
            detail TEXT NOT NULL,
            outcome TEXT NOT NULL,
            prev_hash TEXT NOT NULL,
            hash TEXT NOT NULL
        )",
        "CREATE INDEX IF NOT EXISTS idx_audit_agent ON audit_entries(agent_id)",
        "CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_entries(timestamp)",
        "CREATE INDEX IF NOT EXISTS idx_audit_action ON audit_entries(action)",
        "CREATE TABLE IF NOT EXISTS control_scopes (
            scope_id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            scope_type TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        "CREATE INDEX IF NOT EXISTS idx_control_scopes_type_status ON control_scopes(scope_type, status)",
        "CREATE TABLE IF NOT EXISTS session_bindings (
            binding_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            channel_type TEXT NOT NULL,
            external_user_id TEXT,
            external_chat_id TEXT,
            agent_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            manual_mode BOOLEAN NOT NULL DEFAULT FALSE,
            active_journey_instance_id TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        "CREATE INDEX IF NOT EXISTS idx_session_bindings_scope_channel ON session_bindings(scope_id, channel_type)",
        "CREATE INDEX IF NOT EXISTS idx_session_bindings_session ON session_bindings(session_id)",
        "CREATE INDEX IF NOT EXISTS idx_session_bindings_agent ON session_bindings(agent_id)",
        "CREATE INDEX IF NOT EXISTS idx_session_bindings_external ON session_bindings(scope_id, channel_type, external_chat_id, external_user_id)",
        "CREATE TABLE IF NOT EXISTS observations (
            observation_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            name TEXT NOT NULL,
            matcher_type TEXT NOT NULL,
            matcher_config TEXT NOT NULL,
            priority INTEGER NOT NULL DEFAULT 0,
            enabled BOOLEAN NOT NULL DEFAULT TRUE
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_observations_scope_name ON observations(scope_id, name)",
        "CREATE INDEX IF NOT EXISTS idx_observations_scope_enabled_priority ON observations(scope_id, enabled, priority DESC)",
        "CREATE TABLE IF NOT EXISTS guidelines (
            guideline_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            name TEXT NOT NULL,
            condition_ref TEXT NOT NULL,
            action_text TEXT NOT NULL,
            composition_mode TEXT NOT NULL,
            priority INTEGER NOT NULL DEFAULT 0,
            enabled BOOLEAN NOT NULL DEFAULT TRUE
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_guidelines_scope_name ON guidelines(scope_id, name)",
        "CREATE INDEX IF NOT EXISTS idx_guidelines_scope_enabled_priority ON guidelines(scope_id, enabled, priority DESC)",
        "CREATE TABLE IF NOT EXISTS journeys (
            journey_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            name TEXT NOT NULL,
            trigger_config TEXT NOT NULL,
            completion_rule TEXT,
            entry_state_id TEXT,
            enabled BOOLEAN NOT NULL DEFAULT TRUE
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_journeys_scope_name ON journeys(scope_id, name)",
        "CREATE INDEX IF NOT EXISTS idx_journeys_scope_enabled ON journeys(scope_id, enabled)",
        "ALTER TABLE journeys ADD COLUMN IF NOT EXISTS entry_state_id TEXT",
        "CREATE TABLE IF NOT EXISTS turn_traces (
            trace_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            channel_type TEXT NOT NULL,
            request_message_ref TEXT,
            compiled_context_hash TEXT,
            response_mode TEXT NOT NULL,
            created_at TEXT NOT NULL,
            release_version TEXT
        )",
        "CREATE INDEX IF NOT EXISTS idx_turn_traces_session_created ON turn_traces(session_id, created_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_turn_traces_scope_created ON turn_traces(scope_id, created_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_turn_traces_agent_created ON turn_traces(agent_id, created_at DESC)",
        "CREATE TABLE IF NOT EXISTS guideline_relationships (
            relationship_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            from_guideline_id TEXT NOT NULL,
            to_guideline_id TEXT NOT NULL,
            relation_type TEXT NOT NULL
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_guideline_relationships_edge ON guideline_relationships(scope_id, from_guideline_id, to_guideline_id, relation_type)",
        "CREATE INDEX IF NOT EXISTS idx_guideline_relationships_from ON guideline_relationships(from_guideline_id)",
        "CREATE INDEX IF NOT EXISTS idx_guideline_relationships_to ON guideline_relationships(to_guideline_id)",
        "CREATE TABLE IF NOT EXISTS journey_states (
            state_id TEXT PRIMARY KEY,
            journey_id TEXT NOT NULL,
            name TEXT NOT NULL,
            description TEXT,
            required_fields TEXT NOT NULL DEFAULT '[]',
            guideline_actions_json TEXT NOT NULL DEFAULT '[]'
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_journey_states_journey_name ON journey_states(journey_id, name)",
        "CREATE INDEX IF NOT EXISTS idx_journey_states_journey ON journey_states(journey_id)",
        "CREATE TABLE IF NOT EXISTS journey_transitions (
            transition_id TEXT PRIMARY KEY,
            journey_id TEXT NOT NULL,
            from_state_id TEXT NOT NULL,
            to_state_id TEXT NOT NULL,
            condition_config TEXT NOT NULL DEFAULT '{}',
            transition_type TEXT NOT NULL
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_journey_transitions_edge ON journey_transitions(journey_id, from_state_id, to_state_id, transition_type)",
        "CREATE INDEX IF NOT EXISTS idx_journey_transitions_journey_from ON journey_transitions(journey_id, from_state_id)",
        "CREATE INDEX IF NOT EXISTS idx_journey_transitions_to ON journey_transitions(to_state_id)",
        "CREATE TABLE IF NOT EXISTS journey_instances (
            journey_instance_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            journey_id TEXT NOT NULL,
            current_state_id TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            state_payload TEXT NOT NULL DEFAULT '{}',
            updated_at TEXT NOT NULL
        )",
        "CREATE INDEX IF NOT EXISTS idx_journey_instances_session ON journey_instances(session_id)",
        "CREATE INDEX IF NOT EXISTS idx_journey_instances_scope_status ON journey_instances(scope_id, status)",
        "CREATE INDEX IF NOT EXISTS idx_journey_instances_journey ON journey_instances(journey_id)",
        "CREATE INDEX IF NOT EXISTS idx_journey_instances_current_state ON journey_instances(current_state_id)",
        "CREATE TABLE IF NOT EXISTS policy_match_records (
            record_id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            observation_hits_json TEXT NOT NULL DEFAULT '[]',
            guideline_hits_json TEXT NOT NULL DEFAULT '[]',
            guideline_exclusions_json TEXT NOT NULL DEFAULT '[]'
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_policy_match_records_trace ON policy_match_records(trace_id)",
        "CREATE TABLE IF NOT EXISTS journey_transition_records (
            record_id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            journey_instance_id TEXT NOT NULL,
            before_state_id TEXT,
            after_state_id TEXT,
            decision_json TEXT NOT NULL DEFAULT '{}'
        )",
        "CREATE INDEX IF NOT EXISTS idx_journey_transition_records_trace ON journey_transition_records(trace_id)",
        "CREATE INDEX IF NOT EXISTS idx_journey_transition_records_instance ON journey_transition_records(journey_instance_id)",
        "CREATE TABLE IF NOT EXISTS tool_authorization_records (
            record_id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            allowed_tools_json TEXT NOT NULL DEFAULT '[]',
            authorization_reasons_json TEXT NOT NULL DEFAULT '{}',
            approval_requirements_json TEXT NOT NULL DEFAULT '{}'
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_tool_authorization_records_trace ON tool_authorization_records(trace_id)",
        "CREATE TABLE IF NOT EXISTS handoff_records (
            handoff_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            reason TEXT NOT NULL,
            summary TEXT,
            status TEXT NOT NULL DEFAULT 'requested',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        "CREATE INDEX IF NOT EXISTS idx_handoff_records_session ON handoff_records(session_id)",
        "CREATE INDEX IF NOT EXISTS idx_handoff_records_scope_status ON handoff_records(scope_id, status)",
        "CREATE TABLE IF NOT EXISTS retrievers (
            retriever_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            name TEXT NOT NULL,
            retriever_type TEXT NOT NULL,
            config_json TEXT NOT NULL DEFAULT '{}',
            enabled BOOLEAN NOT NULL DEFAULT TRUE
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_retrievers_scope_name ON retrievers(scope_id, name)",
        "CREATE INDEX IF NOT EXISTS idx_retrievers_scope_enabled ON retrievers(scope_id, enabled)",
        "CREATE TABLE IF NOT EXISTS retriever_bindings (
            binding_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            retriever_id TEXT NOT NULL,
            bind_type TEXT NOT NULL,
            bind_ref TEXT NOT NULL
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_retriever_bindings_unique ON retriever_bindings(scope_id, retriever_id, bind_type, bind_ref)",
        "CREATE INDEX IF NOT EXISTS idx_retriever_bindings_scope_bind ON retriever_bindings(scope_id, bind_type, bind_ref)",
        "CREATE INDEX IF NOT EXISTS idx_retriever_bindings_retriever ON retriever_bindings(retriever_id)",
        "CREATE TABLE IF NOT EXISTS glossary_terms (
            term_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            name TEXT NOT NULL,
            description TEXT NOT NULL,
            synonyms_json TEXT NOT NULL DEFAULT '[]',
            enabled BOOLEAN NOT NULL DEFAULT TRUE,
            always_include BOOLEAN NOT NULL DEFAULT FALSE
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_glossary_terms_scope_name ON glossary_terms(scope_id, name)",
        "CREATE INDEX IF NOT EXISTS idx_glossary_terms_scope_enabled ON glossary_terms(scope_id, enabled)",
        "CREATE TABLE IF NOT EXISTS context_variables (
            variable_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            name TEXT NOT NULL,
            value_source_type TEXT NOT NULL,
            value_source_config TEXT NOT NULL DEFAULT '{}',
            visibility_rule TEXT,
            enabled BOOLEAN NOT NULL DEFAULT TRUE
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_context_variables_scope_name ON context_variables(scope_id, name)",
        "CREATE INDEX IF NOT EXISTS idx_context_variables_scope_enabled ON context_variables(scope_id, enabled)",
        "CREATE INDEX IF NOT EXISTS idx_context_variables_source_type ON context_variables(value_source_type)",
        "CREATE TABLE IF NOT EXISTS context_variable_values (
            value_id TEXT PRIMARY KEY,
            variable_id TEXT NOT NULL,
            key TEXT NOT NULL,
            data_json TEXT NOT NULL DEFAULT 'null',
            updated_at TEXT NOT NULL
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_context_variable_values_variable_key ON context_variable_values(variable_id, key)",
        "CREATE INDEX IF NOT EXISTS idx_context_variable_values_variable_id ON context_variable_values(variable_id)",
        "CREATE TABLE IF NOT EXISTS canned_responses (
            response_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            name TEXT NOT NULL,
            template_text TEXT NOT NULL,
            trigger_rule TEXT,
            priority INTEGER NOT NULL DEFAULT 0,
            enabled BOOLEAN NOT NULL DEFAULT TRUE
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_canned_responses_scope_name ON canned_responses(scope_id, name)",
        "CREATE INDEX IF NOT EXISTS idx_canned_responses_scope_enabled ON canned_responses(scope_id, enabled)",
        "CREATE TABLE IF NOT EXISTS tool_exposure_policies (
            policy_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            tool_name TEXT NOT NULL,
            skill_ref TEXT,
            observation_ref TEXT,
            journey_state_ref TEXT,
            guideline_ref TEXT,
            approval_mode TEXT NOT NULL,
            enabled BOOLEAN NOT NULL DEFAULT TRUE
        )",
        "CREATE INDEX IF NOT EXISTS idx_tool_exposure_policies_scope_tool ON tool_exposure_policies(scope_id, tool_name)",
        "CREATE INDEX IF NOT EXISTS idx_tool_exposure_policies_scope_enabled ON tool_exposure_policies(scope_id, enabled)",
        "CREATE INDEX IF NOT EXISTS idx_tool_exposure_policies_scope_approval ON tool_exposure_policies(scope_id, approval_mode)",
        "CREATE TABLE IF NOT EXISTS control_releases (
            release_id TEXT PRIMARY KEY,
            scope_id TEXT NOT NULL,
            version TEXT NOT NULL,
            status TEXT NOT NULL,
            published_by TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_control_releases_scope_version ON control_releases(scope_id, version)",
        "CREATE INDEX IF NOT EXISTS idx_control_releases_scope_status ON control_releases(scope_id, status)",
        "CREATE INDEX IF NOT EXISTS idx_control_releases_created ON control_releases(created_at DESC)",
        "CREATE TABLE IF NOT EXISTS tenants (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            slug TEXT NOT NULL UNIQUE,
            im_provider TEXT NOT NULL DEFAULT 'web_only',
            timezone TEXT NOT NULL DEFAULT 'UTC',
            is_active BOOLEAN NOT NULL DEFAULT TRUE,
            created_at TEXT NOT NULL,
            default_message_limit INTEGER NOT NULL DEFAULT 50,
            default_message_period TEXT NOT NULL DEFAULT 'permanent',
            default_max_agents INTEGER NOT NULL DEFAULT 2,
            default_agent_ttl_hours INTEGER NOT NULL DEFAULT 48,
            default_max_llm_calls_per_day INTEGER NOT NULL DEFAULT 100,
            min_heartbeat_interval_minutes INTEGER NOT NULL DEFAULT 120,
            default_max_triggers INTEGER NOT NULL DEFAULT 20,
            min_poll_interval_floor INTEGER NOT NULL DEFAULT 5,
            max_webhook_rate_ceiling INTEGER NOT NULL DEFAULT 5
        )",
        "ALTER TABLE tenants ADD COLUMN IF NOT EXISTS im_provider TEXT NOT NULL DEFAULT 'web_only'",
        "ALTER TABLE tenants ADD COLUMN IF NOT EXISTS timezone TEXT NOT NULL DEFAULT 'UTC'",
        "ALTER TABLE tenants ADD COLUMN IF NOT EXISTS default_max_llm_calls_per_day INTEGER NOT NULL DEFAULT 100",
        "ALTER TABLE tenants ADD COLUMN IF NOT EXISTS min_heartbeat_interval_minutes INTEGER NOT NULL DEFAULT 120",
        "ALTER TABLE tenants ADD COLUMN IF NOT EXISTS default_max_triggers INTEGER NOT NULL DEFAULT 20",
        "ALTER TABLE tenants ADD COLUMN IF NOT EXISTS min_poll_interval_floor INTEGER NOT NULL DEFAULT 5",
        "ALTER TABLE tenants ADD COLUMN IF NOT EXISTS max_webhook_rate_ceiling INTEGER NOT NULL DEFAULT 5",
        "CREATE INDEX IF NOT EXISTS idx_tenants_slug ON tenants(slug)",
        "CREATE INDEX IF NOT EXISTS idx_tenants_active_created ON tenants(is_active, created_at DESC)",
        "CREATE TABLE IF NOT EXISTS users (
            user_id TEXT PRIMARY KEY,
            username TEXT NOT NULL UNIQUE,
            email TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            display_name TEXT NOT NULL,
            role TEXT NOT NULL,
            tenant_id TEXT,
            is_active BOOLEAN NOT NULL DEFAULT TRUE,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            quota_message_limit INTEGER NOT NULL DEFAULT 50,
            quota_message_period TEXT NOT NULL DEFAULT 'permanent',
            quota_messages_used INTEGER NOT NULL DEFAULT 0,
            quota_max_agents INTEGER NOT NULL DEFAULT 2,
            quota_agent_ttl_hours INTEGER NOT NULL DEFAULT 48,
            source TEXT NOT NULL DEFAULT 'registration'
        )",
        "CREATE INDEX IF NOT EXISTS idx_users_username ON users(username)",
        "CREATE INDEX IF NOT EXISTS idx_users_tenant_role ON users(tenant_id, role)",
        "CREATE TABLE IF NOT EXISTS invitation_codes (
            id TEXT PRIMARY KEY,
            code TEXT NOT NULL UNIQUE,
            tenant_id TEXT,
            max_uses INTEGER NOT NULL DEFAULT 1,
            used_count INTEGER NOT NULL DEFAULT 0,
            is_active BOOLEAN NOT NULL DEFAULT TRUE,
            created_by TEXT,
            created_at TEXT NOT NULL
        )",
        "CREATE INDEX IF NOT EXISTS idx_invitation_codes_tenant_active ON invitation_codes(tenant_id, is_active)",
        "CREATE INDEX IF NOT EXISTS idx_invitation_codes_created_at ON invitation_codes(created_at DESC)",
        "CREATE TABLE IF NOT EXISTS system_settings (
            key TEXT PRIMARY KEY,
            value_json TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        "CREATE TABLE IF NOT EXISTS plaza_posts (
            id TEXT PRIMARY KEY,
            author_id TEXT NOT NULL,
            author_type TEXT NOT NULL,
            author_name TEXT NOT NULL,
            content TEXT NOT NULL,
            tenant_id TEXT,
            likes_count INTEGER NOT NULL DEFAULT 0,
            comments_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        )",
        "CREATE INDEX IF NOT EXISTS idx_plaza_posts_tenant_created ON plaza_posts(tenant_id, created_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_plaza_posts_author ON plaza_posts(author_id, created_at DESC)",
        "CREATE TABLE IF NOT EXISTS plaza_comments (
            id TEXT PRIMARY KEY,
            post_id TEXT NOT NULL,
            author_id TEXT NOT NULL,
            author_type TEXT NOT NULL,
            author_name TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
        "CREATE INDEX IF NOT EXISTS idx_plaza_comments_post_created ON plaza_comments(post_id, created_at ASC)",
        "CREATE TABLE IF NOT EXISTS plaza_likes (
            id TEXT PRIMARY KEY,
            post_id TEXT NOT NULL,
            author_id TEXT NOT NULL,
            author_type TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_plaza_likes_unique ON plaza_likes(post_id, author_id, author_type)",
        "CREATE TABLE IF NOT EXISTS notifications (
            id TEXT PRIMARY KEY,
            tenant_id TEXT,
            user_id TEXT NOT NULL,
            type TEXT NOT NULL,
            category TEXT NOT NULL,
            title TEXT NOT NULL,
            body TEXT,
            link TEXT,
            sender_id TEXT,
            sender_name TEXT,
            created_at TEXT NOT NULL,
            read_at TEXT
        )",
        "CREATE INDEX IF NOT EXISTS idx_notifications_user_created ON notifications(user_id, created_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_notifications_user_category ON notifications(user_id, category, created_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_notifications_tenant_created ON notifications(tenant_id, created_at DESC)",
        "INSERT INTO migrations (version, applied_at, description)
         VALUES (15, NOW()::TEXT, 'PostgreSQL schema bootstrap')
         ON CONFLICT (version) DO NOTHING",
    ];

    for statement in statements {
        sqlx::query(statement).execute(pool).await?;
    }

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

fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
        [table],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .unwrap_or(false)
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
            required_fields TEXT NOT NULL DEFAULT '[]',
            guideline_actions_json TEXT NOT NULL DEFAULT '[]'
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

        CREATE TABLE IF NOT EXISTS context_variable_values (
            value_id TEXT PRIMARY KEY,
            variable_id TEXT NOT NULL,
            key TEXT NOT NULL,
            data_json TEXT NOT NULL DEFAULT 'null',
            updated_at TEXT NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_context_variable_values_variable_key
            ON context_variable_values(variable_id, key);
        CREATE INDEX IF NOT EXISTS idx_context_variable_values_variable_id
            ON context_variable_values(variable_id);

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

fn migrate_v12(conn: &Connection) -> Result<(), rusqlite::Error> {
    // Add guideline_actions_json column to journey_states (for journey→guideline projection).
    if !column_exists(conn, "journey_states", "guideline_actions_json") {
        conn.execute(
            "ALTER TABLE journey_states ADD COLUMN guideline_actions_json TEXT NOT NULL DEFAULT '[]'",
            [],
        )?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO migrations (version, applied_at, description)
         VALUES (12, datetime('now'), 'Add guideline_actions_json to journey_states for Parlant-style journey projection')",
        [],
    )?;
    Ok(())
}

fn migrate_v13(conn: &Connection) -> Result<(), rusqlite::Error> {
    if !column_exists(conn, "canned_responses", "priority") {
        conn.execute(
            "ALTER TABLE canned_responses ADD COLUMN priority INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO migrations (version, applied_at, description)
         VALUES (13, datetime('now'), 'Add priority column to canned_responses for response ranking')",
        [],
    )?;
    Ok(())
}

fn migrate_v14(conn: &Connection) -> Result<(), rusqlite::Error> {
    if !column_exists(conn, "turn_traces", "release_version") {
        conn.execute(
            "ALTER TABLE turn_traces ADD COLUMN release_version TEXT",
            [],
        )?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO migrations (version, applied_at, description)
         VALUES (14, datetime('now'), 'Add release_version column to turn_traces for control trace auditing')",
        [],
    )?;
    Ok(())
}

fn migrate_v15(conn: &Connection) -> Result<(), rusqlite::Error> {
    if !column_exists(conn, "glossary_terms", "always_include") {
        conn.execute(
            "ALTER TABLE glossary_terms ADD COLUMN always_include INTEGER NOT NULL DEFAULT 0 CHECK (always_include IN (0, 1))",
            [],
        )?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO migrations (version, applied_at, description)
         VALUES (15, datetime('now'), 'Add always_include to glossary_terms for pinned prompt terms')",
        [],
    )?;
    Ok(())
}

fn migrate_v16(conn: &Connection) -> Result<(), rusqlite::Error> {
    if !column_exists(conn, "journeys", "entry_state_id") {
        conn.execute("ALTER TABLE journeys ADD COLUMN entry_state_id TEXT", [])?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO migrations (version, applied_at, description)
         VALUES (16, datetime('now'), 'Add entry_state_id to journeys for explicit journey entry selection')",
        [],
    )?;
    Ok(())
}

fn migrate_v17(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS tenants (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            slug TEXT NOT NULL UNIQUE,
            im_provider TEXT NOT NULL DEFAULT 'web_only',
            timezone TEXT NOT NULL DEFAULT 'UTC',
            is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
            created_at TEXT NOT NULL,
            default_message_limit INTEGER NOT NULL DEFAULT 50,
            default_message_period TEXT NOT NULL DEFAULT 'permanent',
            default_max_agents INTEGER NOT NULL DEFAULT 2,
            default_agent_ttl_hours INTEGER NOT NULL DEFAULT 48,
            default_max_llm_calls_per_day INTEGER NOT NULL DEFAULT 100,
            min_heartbeat_interval_minutes INTEGER NOT NULL DEFAULT 120,
            default_max_triggers INTEGER NOT NULL DEFAULT 20,
            min_poll_interval_floor INTEGER NOT NULL DEFAULT 5,
            max_webhook_rate_ceiling INTEGER NOT NULL DEFAULT 5
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_tenants_slug ON tenants(slug)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_tenants_active_created ON tenants(is_active, created_at DESC)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (
            user_id TEXT PRIMARY KEY,
            username TEXT NOT NULL UNIQUE,
            email TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            display_name TEXT NOT NULL,
            role TEXT NOT NULL,
            tenant_id TEXT,
            is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            quota_message_limit INTEGER NOT NULL DEFAULT 50,
            quota_message_period TEXT NOT NULL DEFAULT 'permanent',
            quota_messages_used INTEGER NOT NULL DEFAULT 0,
            quota_max_agents INTEGER NOT NULL DEFAULT 2,
            quota_agent_ttl_hours INTEGER NOT NULL DEFAULT 48,
            source TEXT NOT NULL DEFAULT 'registration'
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_users_username ON users(username)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_users_tenant_role ON users(tenant_id, role)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS invitation_codes (
            id TEXT PRIMARY KEY,
            code TEXT NOT NULL UNIQUE,
            tenant_id TEXT,
            max_uses INTEGER NOT NULL DEFAULT 1,
            used_count INTEGER NOT NULL DEFAULT 0,
            is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
            created_by TEXT,
            created_at TEXT NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_invitation_codes_tenant_active ON invitation_codes(tenant_id, is_active)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_invitation_codes_created_at ON invitation_codes(created_at DESC)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS system_settings (
            key TEXT PRIMARY KEY,
            value_json TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "INSERT OR IGNORE INTO migrations (version, applied_at, description)
         VALUES (17, datetime('now'), 'Add dashboard multi-tenant entities: tenants, dashboard_users, invitation_codes, system_settings')",
        [],
    )?;
    Ok(())
}

fn migrate_v18(conn: &Connection) -> Result<(), rusqlite::Error> {
    if table_exists(conn, "dashboard_users") && !table_exists(conn, "users") {
        conn.execute("ALTER TABLE dashboard_users RENAME TO users", [])?;
    }
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (
            user_id TEXT PRIMARY KEY,
            username TEXT NOT NULL UNIQUE,
            email TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            display_name TEXT NOT NULL,
            role TEXT NOT NULL,
            tenant_id TEXT,
            is_active INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            quota_message_limit INTEGER NOT NULL DEFAULT 50,
            quota_message_period TEXT NOT NULL DEFAULT 'permanent',
            quota_messages_used INTEGER NOT NULL DEFAULT 0,
            quota_max_agents INTEGER NOT NULL DEFAULT 2,
            quota_agent_ttl_hours INTEGER NOT NULL DEFAULT 48,
            source TEXT NOT NULL DEFAULT 'registration'
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_users_username ON users(username)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_users_tenant_role ON users(tenant_id, role)",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO migrations (version, applied_at, description)
         VALUES (18, datetime('now'), 'Rename dashboard_users to users for tenant-aware platform accounts')",
        [],
    )?;
    Ok(())
}

fn migrate_v19(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS plaza_posts (
            id TEXT PRIMARY KEY,
            author_id TEXT NOT NULL,
            author_type TEXT NOT NULL,
            author_name TEXT NOT NULL,
            content TEXT NOT NULL,
            tenant_id TEXT,
            likes_count INTEGER NOT NULL DEFAULT 0,
            comments_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_plaza_posts_tenant_created ON plaza_posts(tenant_id, created_at DESC)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_plaza_posts_author ON plaza_posts(author_id, created_at DESC)",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS plaza_comments (
            id TEXT PRIMARY KEY,
            post_id TEXT NOT NULL,
            author_id TEXT NOT NULL,
            author_type TEXT NOT NULL,
            author_name TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_plaza_comments_post_created ON plaza_comments(post_id, created_at ASC)",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS plaza_likes (
            id TEXT PRIMARY KEY,
            post_id TEXT NOT NULL,
            author_id TEXT NOT NULL,
            author_type TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_plaza_likes_unique ON plaza_likes(post_id, author_id, author_type)",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO migrations (version, applied_at, description)
         VALUES (19, datetime('now'), 'Add plaza social feed tables for tenant-scoped posts, comments, and likes')",
        [],
    )?;
    Ok(())
}

fn migrate_v20(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS notifications (
            id TEXT PRIMARY KEY,
            tenant_id TEXT,
            user_id TEXT NOT NULL,
            type TEXT NOT NULL,
            category TEXT NOT NULL,
            title TEXT NOT NULL,
            body TEXT,
            link TEXT,
            sender_id TEXT,
            sender_name TEXT,
            created_at TEXT NOT NULL,
            read_at TEXT
        )",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_notifications_user_created ON notifications(user_id, created_at DESC)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_notifications_user_category ON notifications(user_id, category, created_at DESC)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_notifications_tenant_created ON notifications(tenant_id, created_at DESC)",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO migrations (version, applied_at, description)
         VALUES (20, datetime('now'), 'Add tenant-scoped user notifications for inbox and plaza activity')",
        [],
    )?;
    Ok(())
}

fn migrate_v21(conn: &Connection) -> Result<(), rusqlite::Error> {
    if !column_exists(conn, "tenants", "im_provider") {
        conn.execute(
            "ALTER TABLE tenants ADD COLUMN im_provider TEXT NOT NULL DEFAULT 'web_only'",
            [],
        )?;
    }
    if !column_exists(conn, "tenants", "timezone") {
        conn.execute(
            "ALTER TABLE tenants ADD COLUMN timezone TEXT NOT NULL DEFAULT 'UTC'",
            [],
        )?;
    }
    if !column_exists(conn, "tenants", "default_max_llm_calls_per_day") {
        conn.execute(
            "ALTER TABLE tenants ADD COLUMN default_max_llm_calls_per_day INTEGER NOT NULL DEFAULT 100",
            [],
        )?;
    }
    if !column_exists(conn, "tenants", "min_heartbeat_interval_minutes") {
        conn.execute(
            "ALTER TABLE tenants ADD COLUMN min_heartbeat_interval_minutes INTEGER NOT NULL DEFAULT 120",
            [],
        )?;
    }
    if !column_exists(conn, "tenants", "default_max_triggers") {
        conn.execute(
            "ALTER TABLE tenants ADD COLUMN default_max_triggers INTEGER NOT NULL DEFAULT 20",
            [],
        )?;
    }
    if !column_exists(conn, "tenants", "min_poll_interval_floor") {
        conn.execute(
            "ALTER TABLE tenants ADD COLUMN min_poll_interval_floor INTEGER NOT NULL DEFAULT 5",
            [],
        )?;
    }
    if !column_exists(conn, "tenants", "max_webhook_rate_ceiling") {
        conn.execute(
            "ALTER TABLE tenants ADD COLUMN max_webhook_rate_ceiling INTEGER NOT NULL DEFAULT 5",
            [],
        )?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO migrations (version, applied_at, description)
         VALUES (21, datetime('now'), 'Expand tenants with company settings for timezone and quota defaults')",
        [],
    )?;
    Ok(())
}

fn migrate_v22(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS context_variable_values (
            value_id TEXT PRIMARY KEY,
            variable_id TEXT NOT NULL,
            key TEXT NOT NULL,
            data_json TEXT NOT NULL DEFAULT 'null',
            updated_at TEXT NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_context_variable_values_variable_key
         ON context_variable_values(variable_id, key)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_context_variable_values_variable_id
         ON context_variable_values(variable_id)",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO migrations (version, applied_at, description)
         VALUES (22, datetime('now'), 'Add context variable value storage for dynamic session-scoped variables')",
        [],
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
        assert!(tables.contains(&"context_variable_values".to_string()));
        assert!(tables.contains(&"canned_responses".to_string()));
        assert!(tables.contains(&"tool_exposure_policies".to_string()));
        assert!(tables.contains(&"control_releases".to_string()));
        assert!(tables.contains(&"tenants".to_string()));
        assert!(tables.contains(&"users".to_string()));
        assert!(tables.contains(&"invitation_codes".to_string()));
        assert!(tables.contains(&"system_settings".to_string()));
        assert!(tables.contains(&"plaza_posts".to_string()));
        assert!(tables.contains(&"plaza_comments".to_string()));
        assert!(tables.contains(&"plaza_likes".to_string()));
        assert!(tables.contains(&"notifications".to_string()));
    }

    #[test]
    fn test_migration_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap(); // Should not error
    }
}
