import type { ResourceConfig, TabKey } from './types';
import { parseJson, pretty, prettyJsonValue } from './utils';

export const TABS: Array<{ key: TabKey; label: string }> = [
    { key: 'observations', label: 'Observations' },
    { key: 'guidelines', label: 'Guidelines' },
    { key: 'journeys', label: 'Journeys' },
    { key: 'knowledge', label: 'Knowledge' },
    { key: 'governance', label: 'Governance' },
];

export const RESOURCES: ResourceConfig[] = [
    {
        key: 'observations',
        title: 'Observations',
        listKey: 'observations',
        tab: 'observations',
        idField: 'observation_id',
        createPath: '/control/observations',
        itemPath: (id) => `/control/observations/${id}`,
        empty: { id: '', name: '', matcher_type: 'keyword', matcher_config: '{\n  "contains": []\n}', priority: 0, enabled: true },
        fields: [
            { key: 'name', label: 'Name', kind: 'text' },
            { key: 'matcher_type', label: 'Matcher Type', kind: 'select', options: ['keyword', 'regex', 'always', 'semantic'] },
            { key: 'matcher_config', label: 'Matcher Config JSON', kind: 'textarea' },
            { key: 'priority', label: 'Priority', kind: 'number' },
            { key: 'enabled', label: 'Enabled', kind: 'checkbox' },
        ],
        encode: (form, scopeId) => ({
            scope_id: scopeId,
            name: form.name.trim(),
            matcher_type: form.matcher_type,
            matcher_config: parseJson(form.matcher_config, {}),
            priority: Number(form.priority) || 0,
            enabled: form.enabled,
        }),
        decode: (item) => ({
            id: item.observation_id,
            name: item.name,
            matcher_type: item.matcher_type,
            matcher_config: pretty(item.matcher_config || {}),
            priority: item.priority || 0,
            enabled: !!item.enabled,
        }),
        summary: (item) => (
            <>
                <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginTop: '4px' }}>
                    {item.matcher_type} · priority {item.priority}
                </div>
                <pre style={{ marginTop: '8px', fontSize: '11px', color: 'var(--text-secondary)', whiteSpace: 'pre-wrap' }}>
                    {pretty(item.matcher_config || {})}
                </pre>
            </>
        ),
        description: '删除 observation 时会同步清理 observation 级 tool policy 引用。',
    },
    {
        key: 'guidelines',
        title: 'Guidelines',
        listKey: 'guidelines',
        tab: 'guidelines',
        idField: 'guideline_id',
        createPath: '/control/guidelines',
        itemPath: (id) => `/control/guidelines/${id}`,
        empty: { id: '', name: '', condition_ref: '', action_text: '', composition_mode: 'append', priority: 0, enabled: true },
        fields: [
            { key: 'name', label: 'Name', kind: 'text' },
            { key: 'condition_ref', label: 'Condition Ref', kind: 'text' },
            { key: 'action_text', label: 'Action', kind: 'textarea' },
            { key: 'composition_mode', label: 'Composition Mode', kind: 'select', options: ['append', 'guided', 'strict', 'canned_only', 'canned_strict'] },
            { key: 'priority', label: 'Priority', kind: 'number' },
            { key: 'enabled', label: 'Enabled', kind: 'checkbox' },
        ],
        encode: (form, scopeId) => ({
            scope_id: scopeId,
            name: form.name.trim(),
            condition_ref: form.condition_ref.trim(),
            action_text: form.action_text.trim(),
            composition_mode: form.composition_mode,
            priority: Number(form.priority) || 0,
            enabled: form.enabled,
        }),
        decode: (item) => ({
            id: item.guideline_id,
            name: item.name,
            condition_ref: item.condition_ref || '',
            action_text: item.action_text || '',
            composition_mode: item.composition_mode || 'append',
            priority: item.priority || 0,
            enabled: !!item.enabled,
        }),
        summary: (item) => (
            <>
                <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginTop: '4px' }}>
                    {item.condition_ref || 'always'} · {item.composition_mode} · priority {item.priority}
                </div>
                <div style={{ marginTop: '8px', fontSize: '12px', color: 'var(--text-secondary)', whiteSpace: 'pre-wrap' }}>
                    {item.action_text}
                </div>
            </>
        ),
    },
    {
        key: 'journeys',
        title: 'Journeys',
        listKey: 'journeys',
        tab: 'journeys',
        idField: 'journey_id',
        createPath: '/control/journeys',
        itemPath: (id) => `/control/journeys/${id}`,
        empty: { id: '', name: '', trigger_config: '{\n  "contains": []\n}', completion_rule: '', enabled: true },
        fields: [
            { key: 'name', label: 'Name', kind: 'text' },
            { key: 'trigger_config', label: 'Trigger Config JSON', kind: 'textarea' },
            { key: 'completion_rule', label: 'Completion Rule', kind: 'text' },
            { key: 'enabled', label: 'Enabled', kind: 'checkbox' },
        ],
        encode: (form, scopeId) => ({
            scope_id: scopeId,
            name: form.name.trim(),
            trigger_config: parseJson(form.trigger_config, {}),
            completion_rule: form.completion_rule.trim() || null,
            enabled: form.enabled,
        }),
        decode: (item) => ({
            id: item.journey_id,
            name: item.name,
            trigger_config: pretty(item.trigger_config || {}),
            completion_rule: item.completion_rule || '',
            enabled: !!item.enabled,
        }),
        summary: (item) => (
            <>
                <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginTop: '4px' }}>
                    entry {item.entry_state_id || 'unset'} · {item.enabled ? 'enabled' : 'disabled'}
                </div>
                <pre style={{ marginTop: '8px', fontSize: '11px', color: 'var(--text-secondary)', whiteSpace: 'pre-wrap' }}>
                    {pretty(item.trigger_config || {})}
                </pre>
            </>
        ),
        description: 'State 和 transition 的前端编辑放下一轮，这里先把 Journey 顶层对象接起来。',
    },
    {
        key: 'glossary',
        title: 'Glossary Terms',
        listKey: 'glossaryTerms',
        tab: 'knowledge',
        idField: 'term_id',
        createPath: '/control/glossary-terms',
        itemPath: (id) => `/control/glossary-terms/${id}`,
        empty: { id: '', name: '', description: '', synonyms: '', enabled: true, always_include: false },
        fields: [
            { key: 'name', label: 'Name', kind: 'text' },
            { key: 'description', label: 'Description', kind: 'textarea' },
            { key: 'synonyms', label: 'Synonyms (comma separated)', kind: 'text' },
            { key: 'enabled', label: 'Enabled', kind: 'checkbox' },
            { key: 'always_include', label: 'Always Include', kind: 'checkbox' },
        ],
        encode: (form, scopeId) => ({
            scope_id: scopeId,
            name: form.name.trim(),
            description: form.description.trim(),
            synonyms: form.synonyms.split(',').map((value: string) => value.trim()).filter(Boolean),
            enabled: form.enabled,
            always_include: form.always_include,
        }),
        decode: (item) => ({
            id: item.term_id,
            name: item.name,
            description: item.description,
            synonyms: (JSON.parse(item.synonyms_json || '[]') as string[]).join(', '),
            enabled: !!item.enabled,
            always_include: !!item.always_include,
        }),
        summary: (item) => <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{item.description}</div>,
    },
    {
        key: 'variables',
        title: 'Context Variables',
        listKey: 'contextVariables',
        tab: 'knowledge',
        idField: 'variable_id',
        createPath: '/control/context-variables',
        itemPath: (id) => `/control/context-variables/${id}`,
        empty: { id: '', name: '', value_source_type: 'static', value_source_config: '{\n  "value": ""\n}', visibility_rule: '', enabled: true },
        fields: [
            { key: 'name', label: 'Name', kind: 'text' },
            { key: 'value_source_type', label: 'Value Source Type', kind: 'select', options: ['static', 'literal', 'agent_kv', 'session_value', 'disabled'] },
            { key: 'value_source_config', label: 'Value Source Config JSON', kind: 'textarea' },
            { key: 'visibility_rule', label: 'Visibility Rule', kind: 'text' },
            { key: 'enabled', label: 'Enabled', kind: 'checkbox' },
        ],
        encode: (form, scopeId) => ({
            scope_id: scopeId,
            name: form.name.trim(),
            value_source_type: form.value_source_type,
            value_source_config: parseJson(form.value_source_config, {}),
            visibility_rule: form.visibility_rule.trim() || null,
            enabled: form.enabled,
        }),
        decode: (item) => ({
            id: item.variable_id,
            name: item.name,
            value_source_type: item.value_source_type,
            value_source_config: prettyJsonValue(item.value_source_config || {}, {}),
            visibility_rule: item.visibility_rule || '',
            enabled: !!item.enabled,
        }),
        summary: (item) => (
            <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginTop: '4px' }}>
                {item.value_source_type} · {item.visibility_rule || 'always visible'}
            </div>
        ),
    },
    {
        key: 'canned',
        title: 'Canned Responses',
        listKey: 'cannedResponses',
        tab: 'knowledge',
        idField: 'response_id',
        createPath: '/control/canned-responses',
        itemPath: (id) => `/control/canned-responses/${id}`,
        empty: { id: '', name: '', template_text: '', trigger_rule: '', priority: 0, enabled: true },
        fields: [
            { key: 'name', label: 'Name', kind: 'text' },
            { key: 'template_text', label: 'Template Text', kind: 'textarea' },
            { key: 'trigger_rule', label: 'Trigger Rule', kind: 'text' },
            { key: 'priority', label: 'Priority', kind: 'number' },
            { key: 'enabled', label: 'Enabled', kind: 'checkbox' },
        ],
        encode: (form, scopeId) => ({
            scope_id: scopeId,
            name: form.name.trim(),
            template_text: form.template_text.trim(),
            trigger_rule: form.trigger_rule.trim() || null,
            priority: Number(form.priority) || 0,
            enabled: form.enabled,
        }),
        decode: (item) => ({
            id: item.response_id,
            name: item.name,
            template_text: item.template_text,
            trigger_rule: item.trigger_rule || '',
            priority: item.priority || 0,
            enabled: !!item.enabled,
        }),
        summary: (item) => (
            <>
                <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>
                    priority {item.priority} · {item.trigger_rule || 'no trigger'}
                </div>
                <div style={{ marginTop: '8px', fontSize: '12px', color: 'var(--text-secondary)', whiteSpace: 'pre-wrap' }}>
                    {item.template_text}
                </div>
            </>
        ),
    },
];
