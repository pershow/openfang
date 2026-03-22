//! Optional PostgreSQL integration coverage for the control-plane storage path.
//!
//! Set `OPENFANG_TEST_POSTGRES_URL` to run this test against a real PostgreSQL
//! database. The test creates an isolated schema, runs migrations inside it,
//! and exercises the shared-db control-plane stack end to end.

use chrono::Utc;
use openparlant_context::StoreKnowledgeCompiler;
use openparlant_control::{ControlStore, DefaultTurnControlCoordinator};
use openparlant_journey::{JourneyStore, StoreJourneyRuntime};
use openparlant_memory::db::SharedDb;
use openparlant_memory::migration::run_postgres_migrations;
use openparlant_memory::usage::{UsageRecord, UsageStore};
use openparlant_policy::{
    PolicyStore, StoreObservationMatcher, StorePolicyResolver, StoreToolGate,
};
use openparlant_runtime::audit::{AuditAction, AuditLog};
use openparlant_types::agent::{AgentId, SessionId};
use openparlant_types::control::{
    ApprovalMode, CanonicalMessage, ControlExplainabilitySnapshot, ControlScope,
    GuidelineDefinition, GuidelineId, JourneyDefinition, JourneyId, ObservationDefinition,
    ObservationId, ScopeId, ToolCallRecord, ToolCandidate, ToolExposurePolicy,
    TurnControlCoordinator, TurnInput, TurnOutcome,
};
use serde_json::json;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

const TEST_POSTGRES_URL_ENV: &str = "OPENFANG_TEST_POSTGRES_URL";

struct TestPgSchema {
    admin_pool: sqlx::PgPool,
    pool: sqlx::PgPool,
    schema: String,
}

impl TestPgSchema {
    async fn from_env() -> Option<Self> {
        let database_url = match std::env::var(TEST_POSTGRES_URL_ENV) {
            Ok(value) if !value.trim().is_empty() => value,
            _ => {
                eprintln!(
                    "skipping postgres control-plane test; set {TEST_POSTGRES_URL_ENV} to enable it"
                );
                return None;
            }
        };

        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("connect admin postgres pool");

        let schema = format!("openparlant_test_{}", Uuid::new_v4().simple());
        let create_schema = format!("CREATE SCHEMA \"{schema}\"");
        sqlx::query(&create_schema)
            .execute(&admin_pool)
            .await
            .expect("create isolated postgres schema");

        let options = PgConnectOptions::from_str(&database_url).expect("parse postgres url");
        let search_path = schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .after_connect(move |conn, _meta| {
                let statement = format!("SET search_path TO \"{search_path}\"");
                Box::pin(async move {
                    sqlx::query(&statement).execute(conn).await?;
                    Ok(())
                })
            })
            .connect_with(options)
            .await
            .expect("connect scoped postgres pool");

        run_postgres_migrations(&pool)
            .await
            .expect("run postgres migrations");

        Some(Self {
            admin_pool,
            pool,
            schema,
        })
    }

    async fn cleanup(self) {
        let schema = self.schema;
        let admin_pool = self.admin_pool;
        let pool = self.pool;
        drop(pool);

        let drop_schema = format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE");
        sqlx::query(&drop_schema)
            .execute(&admin_pool)
            .await
            .expect("drop isolated postgres schema");
        admin_pool.close().await;
    }
}

#[tokio::test]
async fn postgres_control_plane_roundtrip_when_configured() {
    let Some(pg) = TestPgSchema::from_env().await else {
        return;
    };

    {
        let db = SharedDb::from(Arc::new(pg.pool.clone()));
        let control_store = ControlStore::new(db.clone());
        let policy_store = PolicyStore::new(db.clone());
        let journey_store = JourneyStore::new(db.clone());
        let usage_store = UsageStore::new(db.clone());

        let scope_id = ScopeId::new(format!("pg-scope-{}", Uuid::new_v4().simple()));
        let scope = ControlScope {
            scope_id: scope_id.clone(),
            name: "postgres control plane".to_string(),
            scope_type: "agent".to_string(),
            status: "active".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        control_store.upsert_scope(&scope).unwrap();
        assert_eq!(
            control_store.get_scope(&scope_id).unwrap().unwrap().name,
            "postgres control plane"
        );

        control_store
            .upsert_glossary_term(
                &Uuid::new_v4().to_string(),
                &scope_id.0,
                "refund",
                "Refunds are allowed for VIP customers.",
                &json!(["chargeback"]).to_string(),
                true,
                false,
            )
            .unwrap();
        control_store
            .upsert_context_variable(
                &Uuid::new_v4().to_string(),
                &scope_id.0,
                "account_tier",
                "static",
                &json!({ "value": "vip" }).to_string(),
                Some("contains:vip"),
                true,
            )
            .unwrap();
        control_store
            .upsert_canned_response(
                &Uuid::new_v4().to_string(),
                &scope_id.0,
                "refund_reply",
                "We can process your refund.",
                Some("contains:refund"),
                100,
                true,
            )
            .unwrap();

        let retriever_id = Uuid::new_v4().to_string();
        control_store
            .upsert_retriever(&json!({
                "retriever_id": retriever_id,
                "scope_id": scope_id.0,
                "name": "refund_faq",
                "retriever_type": "static",
                "config_json": {
                    "items": [
                        {
                            "title": "Refund policy",
                            "content": "VIP refunds are processed in priority order."
                        }
                    ]
                },
                "enabled": true
            }))
            .unwrap();
        control_store
            .insert_retriever_binding(&scope_id, &retriever_id, "always", "")
            .unwrap();

        let observation = ObservationDefinition {
            observation_id: ObservationId::new(),
            scope_id: scope_id.clone(),
            name: "vip_customer".to_string(),
            matcher_type: "keyword".to_string(),
            matcher_config: json!({ "contains": ["vip"] }),
            priority: 10,
            enabled: true,
        };
        policy_store.upsert_observation(&observation).unwrap();

        let guideline = GuidelineDefinition {
            guideline_id: GuidelineId::new(),
            scope_id: scope_id.clone(),
            name: "priority_refund".to_string(),
            condition_ref: "vip_customer".to_string(),
            action_text: "Handle the refund with priority.".to_string(),
            composition_mode: "append".to_string(),
            priority: 100,
            enabled: true,
        };
        policy_store.upsert_guideline(&guideline).unwrap();
        policy_store
            .upsert_tool_exposure_policy(&ToolExposurePolicy {
                policy_id: Uuid::new_v4().to_string(),
                scope_id: scope_id.clone(),
                tool_name: "browser_navigate".to_string(),
                skill_ref: None,
                observation_ref: Some("vip_customer".to_string()),
                journey_state_ref: None,
                guideline_ref: None,
                approval_mode: ApprovalMode::Required,
                enabled: true,
            })
            .unwrap();

        let journey = JourneyDefinition {
            journey_id: JourneyId::new(),
            scope_id: scope_id.clone(),
            name: "vip_refund_flow".to_string(),
            trigger_config: json!({ "contains": ["vip"] }),
            completion_rule: None,
            entry_state_id: None,
            enabled: true,
        };
        journey_store.upsert_journey(&journey).unwrap();
        let start_state_id = "zzz-start".to_string();
        let child_state_id = "aaa-child".to_string();
        control_store
            .upsert_journey_state(
                &start_state_id,
                &journey.journey_id.0.to_string(),
                "start",
                Some("Entry state"),
                &json!(["order_number"]).to_string(),
                &json!(["Ask for the order number first."]).to_string(),
            )
            .unwrap();
        control_store
            .upsert_journey_state(
                &child_state_id,
                &journey.journey_id.0.to_string(),
                "collect_details",
                Some("Collect the remaining refund details."),
                &json!(["email"]).to_string(),
                &json!(["Confirm the refund email address."]).to_string(),
            )
            .unwrap();
        let other_root_state_id = "bbb-alt-root".to_string();
        control_store
            .upsert_journey_state(
                &other_root_state_id,
                &journey.journey_id.0.to_string(),
                "alternate_root",
                Some("Ambiguous root that should be ignored once entry_state_id is set."),
                &json!([]).to_string(),
                &json!([]).to_string(),
            )
            .unwrap();
        control_store
            .upsert_journey_transition(
                &Uuid::new_v4().to_string(),
                &journey.journey_id.0.to_string(),
                &start_state_id,
                &child_state_id,
                &json!({ "always": true }).to_string(),
                "auto",
            )
            .unwrap();
        journey_store
            .set_entry_state(&journey.journey_id, Some(&start_state_id))
            .unwrap();
        assert_eq!(
            journey_store
                .get_journey(&journey.journey_id)
                .await
                .unwrap()
                .and_then(|journey| journey.entry_state_id),
            Some(start_state_id.clone())
        );

        let agent_id = AgentId::new();
        let session_id = SessionId::new();
        let coordinator = DefaultTurnControlCoordinator::new_with_gate(
            StoreObservationMatcher::new(db.clone()),
            StorePolicyResolver::new(db.clone()),
            StoreJourneyRuntime::new(db.clone()),
            StoreKnowledgeCompiler::new(db.clone()),
            StoreToolGate::new(db.clone()),
        )
        .with_store(control_store.clone());

        let compiled = coordinator
            .compile_turn(TurnInput {
                scope_id: scope_id.clone(),
                agent_id,
                session_id,
                message: CanonicalMessage::text(
                    scope_id.clone(),
                    "web",
                    "vip customer asking for a refund",
                ),
                candidate_tools: vec![
                    ToolCandidate {
                        tool_name: "browser_navigate".to_string(),
                        skill_ref: None,
                    },
                    ToolCandidate {
                        tool_name: "file_read".to_string(),
                        skill_ref: None,
                    },
                ],
                prior_tool_calls: Vec::new(),
            })
            .await
            .unwrap();

        assert!(compiled
            .active_observations
            .iter()
            .any(|hit| hit.name == "vip_customer"));
        assert!(compiled
            .active_guidelines
            .iter()
            .any(|g| g.name == "priority_refund"));
        assert!(compiled
            .active_guidelines
            .iter()
            .any(|g| g.action_text == "Ask for the order number first."));
        assert_eq!(
            compiled
                .active_journey
                .as_ref()
                .map(|journey| journey.current_state.as_str()),
            Some("start")
        );
        assert!(compiled
            .active_journey
            .as_ref()
            .is_some_and(|journey| journey
                .allowed_next_actions
                .iter()
                .any(|action| action.contains(&child_state_id))));
        assert!(compiled
            .glossary_terms
            .iter()
            .any(|entry| entry.name == "refund"));
        assert!(compiled
            .context_variables
            .iter()
            .any(|var| var.name == "account_tier" && var.value == "vip"));
        assert!(compiled
            .canned_response_candidates
            .iter()
            .any(|resp| resp.name == "refund_reply"));
        assert!(compiled
            .retrieved_chunks
            .iter()
            .any(|chunk| chunk.content.contains("priority order")));
        assert_eq!(compiled.allowed_tools, vec!["browser_navigate"]);
        assert_eq!(compiled.approval_required_tools, vec!["browser_navigate"]);

        let instances = journey_store
            .list_active_instances_for_session(&session_id.to_string())
            .await
            .unwrap();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].current_state_id, start_state_id);

        let traces = control_store
            .list_turn_traces_by_session(session_id, 10)
            .unwrap();
        assert_eq!(traces.len(), 1);

        coordinator
            .after_response(&TurnOutcome {
                trace_id: compiled.audit_meta.trace_id,
                scope_id: scope_id.clone(),
                session_id,
                response_text: "We can help with that refund.".to_string(),
                tool_calls: vec![ToolCallRecord {
                    tool_name: "browser_navigate".to_string(),
                    approved: Some(true),
                    success: true,
                    result: Some("{\"ok\":true}".to_string()),
                }],
                handoff_suggested: false,
                explainability: Some(ControlExplainabilitySnapshot::from_compiled(&compiled)),
            })
            .await
            .unwrap();

        let enriched = control_store
            .enrich_turn_traces_json(
                control_store
                    .list_turn_traces_by_session(session_id, 10)
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(enriched.len(), 1);
        assert_eq!(enriched[0]["allowed_tools"], json!(["browser_navigate"]));
        assert_eq!(
            enriched[0]["approval_required_tools"],
            json!({ "tools": ["browser_navigate"] })
        );

        usage_store
            .record(&UsageRecord {
                agent_id,
                model: "test-model".to_string(),
                input_tokens: 120,
                output_tokens: 45,
                cost_usd: 0.0012,
                tool_calls: 1,
            })
            .unwrap();
        let usage = usage_store.query_summary(Some(agent_id)).unwrap();
        assert_eq!(usage.call_count, 1);
        assert_eq!(usage.total_input_tokens, 120);

        let audit_log = AuditLog::with_db(db.clone());
        audit_log.record(
            agent_id.to_string(),
            AuditAction::ConfigChange,
            "postgres test",
            "ok",
        );
        let reloaded = AuditLog::with_db(db);
        assert_eq!(reloaded.len(), 1);
        assert!(reloaded.verify_integrity().is_ok());
    }

    pg.cleanup().await;
}
