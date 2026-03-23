# Control Plane Completion Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 补齐 Parlant 风格控制逻辑的五个关键缺口：LLM 语义 guideline matching、preparation iteration loop、journey 投影为 guideline、向量检索 retriever、Feishu channel bridge 接入控制面。

**Architecture:** 在现有 `silicrew-policy` / `silicrew-journey` / `silicrew-context` / `silicrew-control` 骨架上增量补强，不改变接口签名。所有新增能力均通过可选配置开关激活，默认退化为现有 deterministic 行为，保持零破坏。

**Tech Stack:** Rust, rusqlite / sqlx, async-trait, serde_json, reqwest（LLM 调用复用现有 `LlmDriver`）

---

## Task 1: LLM 语义 Guideline Matcher

**目标：** 当 observation matcher_type = `"semantic"` 时，把当前消息 + 所有 guideline condition 发给 LLM 判断哪些规则生效，返回 score + rationale，替换现在的 `None`（永远不命中）。

**Files:**
- Modify: `crates/silicrew-policy/src/lib.rs`
- Modify: `crates/silicrew-types/src/control.rs`
- Modify: `crates/silicrew-control/src/lib.rs`

**Step 1: 在 `TurnInput` 中携带 LLM driver 引用**

`TurnInput` 需要能传递 LLM 调用能力。当前做法：给 `DefaultTurnControlCoordinator` 增加一个可选的 `llm_caller` 字段，类型为 `Option<Arc<dyn LlmCaller>>`。

在 `crates/silicrew-types/src/control.rs` 新增 trait：

```rust
/// Minimal LLM call abstraction for control-plane semantic matching.
/// Deliberately thin so it can be bridged to the existing LlmDriver.
#[async_trait]
pub trait ControlLlmCaller: Send + Sync {
    /// Call the LLM with a single prompt; return the text response.
    async fn call(&self, prompt: &str) -> anyhow::Result<String>;
}
```

**Step 2: 在 `silicrew-policy/src/lib.rs` 新增 `LlmObservationMatcher`**

```rust
pub struct LlmObservationMatcher {
    store: PolicyStore,
    llm: Arc<dyn silicrew_types::control::ControlLlmCaller>,
}
```

实现 `ObservationMatcher`：
1. 先用 `SqliteObservationMatcher` 跑所有 deterministic/keyword/regex/always 规则
2. 对 `matcher_type = "semantic"` 的 observation，构造 prompt 发给 LLM：

```
You are evaluating whether a conversational rule condition is currently triggered.

User message: "{message}"

Condition to evaluate: "{condition_text}"

Reply with a JSON object:
{"triggered": true|false, "rationale": "..."}
```

3. 解析 JSON，`triggered: true` 则 push `ObservationHit { confidence: score, matched_by: "llm_semantic" }`

**Step 3: 在 `silicrew-policy/src/lib.rs` 新增 `LlmPolicyResolver`**

与 `SqlitePolicyResolver` 逻辑相同，但把 guideline `condition_ref` 为空的情况扩展到也调用 LLM 对 `action_text` 里的 condition 做语义匹配：

```
You are deciding which behavioral guidelines apply to the current conversation turn.

User message: "{message}"

Guidelines to evaluate (JSON array):
[{"id": "...", "condition": "..."}, ...]

For each guideline reply with:
{"results": [{"id": "...", "applies": true|false, "score": 0.0-1.0, "rationale": "..."}]}
```

解析结果，`applies: true` 的追加到 `active_guidelines`。

**Step 4: 在 `crates/silicrew-control/src/lib.rs` 中给 `DefaultTurnControlCoordinator` 增加 `llm_caller` 字段**

```rust
pub struct DefaultTurnControlCoordinator<OM, PR, JR, KC, TG = NoopToolGate> {
    // ... existing fields ...
    llm_caller: Option<Arc<dyn ControlLlmCaller>>,
}
```

新增 builder：
```rust
pub fn with_llm_caller(mut self, caller: Arc<dyn ControlLlmCaller>) -> Self {
    self.llm_caller = Some(caller);
    self
}
```

**Step 5: 在 `crates/silicrew-kernel/src/kernel.rs` 实现 `ControlLlmCaller` bridge**

```rust
struct KernelLlmCaller {
    driver: Arc<dyn LlmDriver>,
    model: String,
}

#[async_trait]
impl ControlLlmCaller for KernelLlmCaller {
    async fn call(&self, prompt: &str) -> anyhow::Result<String> {
        let req = CompletionRequest {
            model: self.model.clone(),
            messages: vec![/* system + user prompt */],
            ..Default::default()
        };
        let resp = self.driver.complete(req).await?;
        Ok(resp.content)
    }
}
```

在 server.rs bootstrap 时把 KernelLlmCaller 传入 coordinator。

**Step 6: 测试**

在 `crates/silicrew-policy/src/lib.rs` 加集成测试（mock LLM）：

```rust
#[tokio::test]
async fn llm_matcher_calls_llm_for_semantic_obs() {
    // mock LLM that returns {"triggered":true,"rationale":"test"}
    // assert ObservationHit is returned with matched_by="llm_semantic"
}
```

**Step 7: Commit**

```bash
git add crates/silicrew-types/src/control.rs \
        crates/silicrew-policy/src/lib.rs \
        crates/silicrew-control/src/lib.rs \
        crates/silicrew-kernel/src/kernel.rs \
        crates/silicrew-api/src/server.rs
git commit -m "feat(policy): add LLM-based semantic guideline/observation matcher"
```

---

## Task 2: Preparation Iteration Loop

**目标：** 在 `compile_turn` 阶段引入迭代：工具调用结果可触发新一轮 observation matching + policy resolve，最多 3 轮，直至稳定。

**Files:**
- Modify: `crates/silicrew-control/src/lib.rs`
- Modify: `crates/silicrew-types/src/control.rs`

**Step 1: 在 `TurnInput` 增加 `prior_tool_calls` 字段**

```rust
pub struct TurnInput {
    pub scope_id: ScopeId,
    pub agent_id: AgentId,
    pub session_id: SessionId,
    pub message: CanonicalMessage,
    /// Tool calls from the current loop iteration (for iteration 2+).
    #[serde(default)]
    pub prior_tool_calls: Vec<ToolCallRecord>,
}
```

**Step 2: 在 `DefaultTurnControlCoordinator::compile_turn` 引入迭代循环**

```rust
const MAX_PREP_ITERATIONS: usize = 3;

pub async fn compile_turn_iterative(
    &self,
    mut input: TurnInput,
    tool_calls_from_prev_iter: Vec<ToolCallRecord>,
) -> Result<CompiledTurnContext> {
    let mut last_ctx = self.compile_turn(input.clone()).await?;
    
    for _iter in 1..MAX_PREP_ITERATIONS {
        if tool_calls_from_prev_iter.is_empty() { break; }
        
        // Append tool call results as additional context in message
        let enriched_text = format!(
            "{}\n\n[Tool results: {}]",
            input.message.text,
            serde_json::to_string(&tool_calls_from_prev_iter)?
        );
        input.message.text = enriched_text;
        input.prior_tool_calls = tool_calls_from_prev_iter.clone();
        
        let new_ctx = self.compile_turn(input.clone()).await?;
        
        // Check stability: same active guidelines & same allowed tools
        if guidelines_stable(&last_ctx, &new_ctx) { break; }
        
        last_ctx = new_ctx;
    }
    
    Ok(last_ctx)
}
```

**Step 3: 在 kernel `execute_llm_agent` 的 tool call 回调中调用 `compile_turn_iterative`**

在 kernel.rs 的 `run_agent_loop` 调用处（约第 2670 行），当 loop 返回中间 tool call 结果时，调用 `compile_turn_iterative` 更新 `compiled_ctx_opt`，并把新的 `allowed_tools` 传给下一轮 loop。

**Step 4: 稳定性判断函数**

```rust
fn guidelines_stable(prev: &CompiledTurnContext, next: &CompiledTurnContext) -> bool {
    let prev_ids: std::collections::HashSet<_> =
        prev.active_guidelines.iter().map(|g| g.guideline_id).collect();
    let next_ids: std::collections::HashSet<_> =
        next.active_guidelines.iter().map(|g| g.guideline_id).collect();
    prev_ids == next_ids && prev.allowed_tools == next.allowed_tools
}
```

**Step 5: Commit**

```bash
git commit -m "feat(control): add preparation iteration loop for tool-triggered guideline re-evaluation"
```

---

## Task 3: Journey 节点投影为 Guideline

**目标：** Journey 的每个 state 可以携带一组 guideline action_text，这些 guideline 在该 journey state 激活时自动加入 `active_guidelines`，与普通 guideline 统一走 policy resolution。

**Files:**
- Modify: `crates/silicrew-journey/src/lib.rs`
- Modify: `crates/silicrew-journey/src/store.rs`
- Modify: `crates/silicrew-control/src/lib.rs`
- Modify: `crates/silicrew-types/src/control.rs`

**Step 1: 在 `journey_states` 表增加 `guideline_actions_json` 字段**

在 `JourneyStore` 的建表 SQL 和 `JourneyState` struct 中增加：

```rust
pub struct JourneyState {
    pub state_id: String,
    pub journey_id: String,
    pub name: String,
    pub description: String,
    pub required_fields: serde_json::Value,
    /// Guidelines projected from this journey state (list of action_text strings).
    pub guideline_actions: Vec<String>,
}
```

**Step 2: 在 `JourneyResolution` 增加 `projected_guidelines`**

```rust
pub struct JourneyResolution {
    pub active_journey: Option<JourneyActivation>,
    /// Guidelines injected by the active journey state.
    pub projected_guidelines: Vec<GuidelineActivation>,
}
```

在 `SqliteJourneyRuntime::resolve_journey` 中，当找到 active state 时，读取 `guideline_actions_json` 并构建 `GuidelineActivation` 列表（`source_observations` 为空，`priority` 取 journey 级配置）。

**Step 3: 在 `DefaultTurnControlCoordinator::compile_turn` 合并 journey 投影的 guidelines**

```rust
// After journey resolution:
let mut all_active_guidelines = policy.active_guidelines;
all_active_guidelines.extend(journey.projected_guidelines);
```

**Step 4: 在 `allowed_next_actions` 填充当前 state 的出边 transition 名称**

```rust
if let Some(ref j_activation) = journey.active_journey {
    let transitions = store.list_transitions_from(&journey_id, &current_state_id)?;
    j_activation.allowed_next_actions = transitions.iter().map(|t| t.name.clone()).collect();
}
```

**Step 5: API 端：`create_journey_state` 接受 `guideline_actions` 字段**

修改 `control_routes.rs` 的 `create_journey_state` handler 保存该字段。

**Step 6: Commit**

```bash
git commit -m "feat(journey): project journey state nodes as guidelines into policy resolution"
```

---

## Task 4: 向量检索 Retriever

**目标：** 新增 `retriever_type = "embedding"` 支持，将 query 通过 LLM embedding API 转为向量，与预存的 chunk 向量做余弦相似度检索。MVP 采用 SQLite 内联存储向量（JSON 数组）。

**Files:**
- Modify: `crates/silicrew-context/src/store.rs`
- Modify: `crates/silicrew-context/src/lib.rs`
- Modify: `crates/silicrew-types/src/control.rs`

**Step 1: 新增 `ControlEmbedder` trait**

在 `crates/silicrew-types/src/control.rs`：

```rust
/// Thin embedding abstraction for the control-plane retriever.
#[async_trait]
pub trait ControlEmbedder: Send + Sync {
    /// Return a unit-normalized embedding vector for the given text.
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>>;
}
```

**Step 2: 在 `retrievers` 表增加 `chunks_json` 字段（MVP 内联存储）**

`config_json` 已经存储配置，对 `embedding` 类型：

```json
{
  "chunks": [
    {"id": "c1", "text": "退款政策：...", "vector": [0.12, -0.34, ...]}
  ]
}
```

**Step 3: 在 `ContextStore::run_retrievers` 增加 embedding 分支**

```rust
"embedding" => {
    let Some(embedder) = embedder_opt else {
        tracing::warn!("embedding retriever requires embedder, skipping");
        continue;
    };
    let query_vec = embedder.embed(&query_lower).await?;
    if let Some(chunks) = retriever.config_json.get("chunks").and_then(|v| v.as_array()) {
        for chunk in chunks {
            let text = chunk.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let vec_json = chunk.get("vector").and_then(|v| v.as_array());
            if let Some(stored_vec) = vec_json {
                let stored: Vec<f32> = stored_vec.iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect();
                let score = cosine_similarity(&query_vec, &stored);
                if score > 0.7 {
                    chunks_out.push(RetrievedChunk {
                        source: format!("embedding:{}", retriever.name),
                        content: text.to_string(),
                        score: Some(score as f64),
                        metadata: None,
                    });
                }
            }
        }
    }
}
```

**Step 4: 实现余弦相似度**

```rust
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() { return 0.0; }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if mag_a == 0.0 || mag_b == 0.0 { 0.0 } else { dot / (mag_a * mag_b) }
}
```

**Step 5: 给 `SqliteKnowledgeCompiler` 增加 `embedder` 字段**

```rust
pub struct SqliteKnowledgeCompiler {
    store: ContextStore,
    embedder: Option<Arc<dyn ControlEmbedder>>,
}
```

**Step 6: 在 server.rs bootstrap 时注入 embedder**

实现 `KernelEmbedder`（类似 Task 1 的 `KernelLlmCaller`），使用现有 LLM driver 调用 embedding 端点。

**Step 7: Commit**

```bash
git commit -m "feat(context): add embedding-based vector retriever with cosine similarity"
```

---

## Task 5: Feishu Channel Bridge 接入控制面

**目标：** Feishu/channel bridge 的 `dispatch_message` 在调用 `handle.send_message` 前，先调用 `ChannelBridgeHandle::check_manual_mode`，手动模式时静默；正常模式时消息已通过 kernel 的 `execute_llm_agent` 自然经过控制面（因为 bridge 调用 `send_message` → kernel → `execute_llm_agent` → `compile_turn`）。

实际上 **Feishu bridge 已间接接入控制面**（通过 `send_message` → kernel → `execute_llm_agent`）。缺失的是：bridge 层不感知 `manual_mode`，以及 `touch_control_session_binding` 只在 bridge 侧调用但不同步 scope_id。

**Files:**
- Modify: `crates/silicrew-channels/src/bridge.rs`
- Modify: `crates/silicrew-kernel/src/kernel.rs`（`touch_control_session_binding` 实现）

**Step 1: 在 `ChannelBridgeHandle` trait 增加 `is_manual_mode` 方法**

```rust
/// Check if a session is currently in manual mode (human takeover).
async fn is_manual_mode(
    &self,
    agent_id: AgentId,
    channel_type: &str,
    external_user_id: Option<&str>,
    external_chat_id: Option<&str>,
) -> bool {
    false // default: no manual mode
}
```

**Step 2: 在 `dispatch_message` 中加 manual_mode 检查**

在 `crates/silicrew-channels/src/bridge.rs` 的 `dispatch_message` 函数，调用 `handle.send_message` 前加：

```rust
// ── Manual mode check ──────────────────────────────────────────────────
if handle
    .is_manual_mode(agent_id, ct_str, Some(&message.sender.platform_id), message.thread_id.as_deref())
    .await
{
    debug!("Session is in manual mode — skipping AI response for {ct_str}");
    return;
}
```

**Step 3: 在 kernel 中实现 `is_manual_mode`**

```rust
async fn is_manual_mode(
    &self,
    agent_id: AgentId,
    _channel_type: &str,
    external_user_id: Option<&str>,
    external_chat_id: Option<&str>,
) -> bool {
    let Some(store) = self.control_store.as_ref() else { return false; };
    // Look up session_bindings by agent_id + external identifiers
    let session_id = self.session_id_for_agent(agent_id);
    store.get_session_binding(&session_id.to_string())
        .ok()
        .flatten()
        .map(|b| b.manual_mode)
        .unwrap_or(false)
}
```

**Step 4: 在 kernel 中实现 `touch_control_session_binding` 真正写入 scope**

当前 kernel 中该函数实现为空。补全：

```rust
async fn touch_control_session_binding(
    &self,
    agent_id: AgentId,
    channel_type: &str,
    external_user_id: Option<&str>,
    external_chat_id: Option<&str>,
) -> Result<(), String> {
    let Some(store) = self.control_store.as_ref() else { return Ok(()); };
    let session_id = self.session_id_for_agent(agent_id);
    let scope_id = self.default_scope_id();
    let binding = SessionBinding {
        binding_id: session_id.to_string(),
        scope_id,
        channel_type: channel_type.to_string(),
        external_user_id: external_user_id.unwrap_or("").to_string(),
        external_chat_id: external_chat_id.unwrap_or("").to_string(),
        agent_id,
        session_id,
        manual_mode: false,
        active_journey_instance_id: None,
        last_message_at: Utc::now(),
    };
    store.upsert_session_binding(&binding)
        .map_err(|e| e.to_string())
}
```

**Step 5: Commit**

```bash
git commit -m "feat(bridge): wire channel dispatch through control-plane manual mode check"
```

---

## 执行顺序建议

按以下顺序执行，依赖最小：

```
Task 5 (Feishu bridge) → Task 1 (LLM matcher) → Task 3 (Journey projection) → Task 2 (Iteration loop) → Task 4 (Vector retriever)
```

Task 5 改动最小、风险最低，先做最快验证。Task 4 向量检索可独立并行做。
