# SiliCrew 对齐 Parlant 缺口分析与开发计划

日期：2026-03-23

## 1. 结论

当前 `openfang` 已经把 `policy / journey / context / trace / handoff` 这些控制层骨架接进了主运行链路，属于可运行的 Parlant 风格控制面 MVP。

但它还不是 `parlant` 语义控制层的等价 Rust 迁移。更准确地说：

- 已经有控制层框架
- 已经有主链路集成
- 已经有 explainability / iterative compile / handoff
- 但对象模型、关系语义、journey 图能力、动态变量、canned response 语义和控制面 API 完整度还明显落后于 `parlant`

## 2. 当前已具备能力

### 2.1 主链路已接入

- `compile_turn` 与 `compile_turn_iterative` 已进入 kernel 执行路径
- 工具调用结果可回灌到下一轮控制编译
- explainability snapshot 可在 `after_response` 阶段落表
- manual mode / handoff 已纳入 session binding 与 API

### 2.2 控制层 crate 已存在

- `openparlant-policy`
- `openparlant-journey`
- `openparlant-context`
- `openparlant-control`

### 2.3 已有一部分 Parlant 风格对象

- observation
- guideline
- journey
- glossary
- context variable
- canned response
- tool policy
- retriever binding

## 3. 主要缺口

### P0. 规则与流程语义仍偏薄

#### 3.1 Guideline 领域模型缩水

当前 `openfang` guideline 主要只有：

- `name`
- `condition_ref`
- `action_text`
- `composition_mode`
- `priority`
- `enabled`

与 `parlant` 相比仍缺：

- `description`
- `tags`
- `metadata`
- `criticality`
- `labels`
- `track`
- 工具关联与 canned response 关联的一等建模
- 稳定 ID 级别的依赖与引用

#### 3.2 Relationship 语义不完整

当前实际生效关系主要是：

- `depends_on`
- `excludes`
- `prioritizes_over`

仍缺少 `parlant` 的更完整语义：

- `entailment`
- `disambiguation`
- `reevaluation`
- `overlap`
- guideline / tag / tool 跨实体关系图

#### 3.3 Journey 仍是 MVP 状态机

当前 `openfang` journey 能做：

- 触发 journey
- 激活 state
- 基于简化 transition 前进
- 将 state 上的 `guideline_actions` 投影为 guideline

与 `parlant` 相比仍缺：

- 完整图结构语义
- node/edge metadata
- node tools
- composition_mode 继承
- labels/tags 的全链路治理
- 多 active journey 与更强状态解析
- 更丰富的 transition 条件与回退/跳步控制

### P1. 知识与上下文对象仍是简化版

#### 3.4 Context Variable 不等价

当前真实可用 source type 主要是：

- `static`
- `literal`
- `agent_kv`
- `disabled`

仍缺：

- 对等的 value CRUD
- `tool_id`
- `freshness_rules`
- tags
- 更丰富 provider/type
- 与会话/客户实体绑定的真实动态值系统

#### 3.5 Canned Response 语义不足

当前更像是：

- `template_text`
- `trigger_rule`
- `priority`

与 `parlant` 相比仍缺：

- `fields`
- `signals`
- `metadata`
- `tags`
- `field_dependencies`
- 更细的 strict / composited / fluid canned 模式

#### 3.6 Retriever / Glossary 仍偏 MVP

当前已支持：

- static retriever
- faq_sqlite retriever
- embedding retriever
- glossary 按相关度筛选
- always_include pin

仍缺：

- 更完整的 retrieval 策略
- 更强实体/标签绑定
- 与 rules / journeys / tools 的更细颗粒联动

### P1. 语义匹配和解释性仍需补强

#### 3.7 Semantic matcher 仍是 opt-in

目前只有设置 `OPENPARLANT_CONTROL_SEMANTIC=true` 才启用 LLM matcher。

这意味着：

- 默认行为仍偏 deterministic
- 生产行为与测试行为差距较大
- 解释性字段里没有完整保留 LLM score / rationale

#### 3.8 composition_mode 还没成为真正控制信号

目前 `composition_mode` 已入库，但对 `response_mode` 的实际影响很弱，未形成 Parlant 那种“规则输出模式直接约束本轮回答”的闭环。

### P2. 控制面 API 和治理能力不完整

#### 3.9 控制面 CRUD 不完整

当前很多控制对象还只有：

- create
- list
- get

明显缺：

- update
- delete
- patch 语义更新
- value 级别 API

#### 3.10 还缺 Parlant 平台侧实体

相对 `parlant` API，当前 `openfang` 控制面还缺少成体系支持：

- tags
- customers
- capabilities
- evaluations
- services

## 4. 开发优先级

### P0

1. 补齐 guideline / relationship / journey 关键语义
2. 让 `composition_mode` 真正影响 turn response mode
3. 逐步把字符串引用收敛到稳定 ID 引用

### P1

1. context variable value CRUD
2. canned response 完整对象语义
3. semantic matcher 解释性补强
4. journey 图运行时增强

### P2

1. tags / customers / capabilities / evaluations / services 控制面实体
2. 完整 update/delete API
3. 更强策略治理与版本化能力

## 5. 本轮启动开发项

本轮先落一个高收益且改动面可控的 P0：

### 5.1 目标

让 active guideline 的 `composition_mode` 真正参与 `response_mode` 推断。

### 5.2 原因

这是当前 `openfang` 与 `parlant` 在“输出约束”上的一个核心缺口：

- 规则里虽然存了 `composition_mode`
- 但编译出来的 `response_mode` 还主要由 canned candidates / approval / glossary / variables 粗略决定
- 导致 strict / canned 类型 guideline 无法稳定约束本轮输出模式

### 5.3 本轮实现范围

- 在 `GuidelineActivation` 中保留 `composition_mode`
- policy resolver 在激活 guideline 时传递该字段
- journey projection 明确给 projected guideline 标记为无 composition mode
- control coordinator 的 `infer_response_mode` 参考 active guideline 的 composition mode
- 增加测试覆盖

### 5.4 本轮不做

- 不改前端表单枚举
- 不大规模改 API 面
- 不引入新的 composition mode 数据表
- 不改变 journey 定义模型

## 6. 后续建议顺序

1. 完成 guideline `composition_mode` 驱动 response mode
2. 补 context variable value CRUD
3. 补 guideline/journey update/delete API
4. 扩 relationship 语义到 `reevaluation / disambiguation / entailment`
5. 提升 journey 图运行时
6. 引入 tags/customers/evaluations/capabilities 控制面实体

