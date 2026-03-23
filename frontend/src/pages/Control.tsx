import { useEffect, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { request } from '../services/api';
import Card from './control/Card';
import { RESOURCES, TABS } from './control/controlConfig';
import GovernanceSection from './control/GovernanceSection';
import JourneySection from './control/JourneySection';
import ResourceSection from './control/ResourceSection';
import ScopeSidebar from './control/ScopeSidebar';
import type {
    BindingForm,
    Feedback,
    JourneyStateForm,
    JourneyTransitionForm,
    RelationshipForm,
    RetrieverForm,
    Scope,
    ScopeDetails,
    TabKey,
    ToolPolicyForm,
    VariableValueForm,
} from './control/types';
import { parseJson, toSingularTitle } from './control/utils';
import VariableValuesSection from './control/VariableValuesSection';

export default function Control() {
    const queryClient = useQueryClient();
    const [selectedTab, setSelectedTab] = useState<TabKey>('observations');
    const [selectedScope, setSelectedScope] = useState('');
    const [newScopeName, setNewScopeName] = useState('');
    const [feedback, setFeedback] = useState<Feedback | null>(null);
    const [busy, setBusy] = useState<string | null>(null);
    const [forms, setForms] = useState<Record<string, any>>(
        Object.fromEntries(RESOURCES.map((resource) => [resource.key, { ...resource.empty }]))
    );
    const [searchText, setSearchText] = useState('');
    const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
    const [selectedJourneyId, setSelectedJourneyId] = useState('');
    const [selectedVariableId, setSelectedVariableId] = useState('');
    const [journeyStateForm, setJourneyStateForm] = useState<JourneyStateForm>({ id: '', name: '', description: '', required_fields: '', guideline_actions: '' });
    const [journeyTransitionForm, setJourneyTransitionForm] = useState<JourneyTransitionForm>({ id: '', from_state_id: '', to_state_id: '', transition_type: 'auto', condition_config: '{}' });
    const [variableValueForm, setVariableValueForm] = useState<VariableValueForm>({ key: '', data: '{\n  "value": ""\n}' });
    const [relationshipForm, setRelationshipForm] = useState<RelationshipForm>({ from_guideline_id: '', to_guideline_id: '', relation_type: 'overrides' });
    const [toolPolicyForm, setToolPolicyForm] = useState<ToolPolicyForm>({ id: '', tool_name: '', skill_ref: '', observation_ref: '', journey_state_ref: '', guideline_ref: '', approval_mode: 'none', enabled: true });
    const [retrieverForm, setRetrieverForm] = useState<RetrieverForm>({ id: '', name: '', retriever_type: 'static', config_json: '{\n  "items": []\n}', enabled: true });
    const [bindingForm, setBindingForm] = useState<BindingForm>({ retriever_id: '', bind_type: 'always', bind_ref: 'always' });
    const [releaseVersion, setReleaseVersion] = useState('');

    const scopesQuery = useQuery({
        queryKey: ['control-scopes'],
        queryFn: () => request<Scope[]>('/control/scopes'),
        refetchInterval: 15000,
    });

    const detailsQuery = useQuery({
        queryKey: ['control-scope-details', selectedScope],
        enabled: !!selectedScope,
        refetchInterval: 15000,
        queryFn: async (): Promise<ScopeDetails> => {
            const [observations, guidelines, journeys, glossaryTerms, contextVariables, cannedResponses, guidelineRelationships, toolPolicies, retrievers, retrieverBindings, releases] = await Promise.all([
                request<any[]>(`/control/scopes/${selectedScope}/observations`).catch(() => []),
                request<any[]>(`/control/scopes/${selectedScope}/guidelines`).catch(() => []),
                request<any[]>(`/control/scopes/${selectedScope}/journeys`).catch(() => []),
                request<any[]>(`/control/scopes/${selectedScope}/glossary-terms`).catch(() => []),
                request<any[]>(`/control/scopes/${selectedScope}/context-variables`).catch(() => []),
                request<any[]>(`/control/scopes/${selectedScope}/canned-responses`).catch(() => []),
                request<any[]>(`/control/scopes/${selectedScope}/guideline-relationships`).catch(() => []),
                request<any[]>(`/control/scopes/${selectedScope}/tool-policies`).catch(() => []),
                request<any[]>(`/control/scopes/${selectedScope}/retrievers`).catch(() => []),
                request<any[]>(`/control/scopes/${selectedScope}/retriever-bindings`).catch(() => []),
                request<any[]>(`/control/scopes/${selectedScope}/releases`).catch(() => []),
            ]);
            return { observations, guidelines, journeys, glossaryTerms, contextVariables, cannedResponses, guidelineRelationships, toolPolicies, retrievers, retrieverBindings, releases };
        },
    });

    const journeyStatesQuery = useQuery({
        queryKey: ['control-journey-states', selectedJourneyId],
        enabled: !!selectedJourneyId,
        refetchInterval: 15000,
        queryFn: () => request<any[]>(`/control/journeys/${selectedJourneyId}/states`),
    });

    const journeyTransitionsQuery = useQuery({
        queryKey: ['control-journey-transitions', selectedJourneyId],
        enabled: !!selectedJourneyId,
        refetchInterval: 15000,
        queryFn: () => request<any[]>(`/control/journeys/${selectedJourneyId}/transitions`),
    });

    const governanceJourneyStatesQuery = useQuery({
        queryKey: ['control-scope-journey-states', selectedScope, (detailsQuery.data?.journeys || []).map((item) => item.journey_id).join(',')],
        enabled: selectedTab === 'governance' && !!selectedScope && !!detailsQuery.data?.journeys?.length,
        refetchInterval: 15000,
        queryFn: async () => {
            const journeys = detailsQuery.data?.journeys || [];
            const statesByJourney = await Promise.all(
                journeys.map(async (journey) => {
                    const states = await request<any[]>(`/control/journeys/${journey.journey_id}/states`).catch(() => []);
                    return states.map((state) => ({
                        ...state,
                        journey_id: state.journey_id || journey.journey_id,
                        journey_name: journey.name,
                    }));
                }),
            );
            return statesByJourney.flat();
        },
    });

    const variableValuesQuery = useQuery({
        queryKey: ['control-variable-values', selectedVariableId],
        enabled: !!selectedVariableId,
        refetchInterval: 15000,
        queryFn: () => request<any[]>(`/control/context-variables/${selectedVariableId}/values`),
    });

    const details = detailsQuery.data;
    const scopes = scopesQuery.data || [];
    const knowledgeResources = RESOURCES.filter((resource) => resource.tab === 'knowledge');
    const activeResource = RESOURCES.find((resource) => resource.tab === selectedTab);

    const updateForm = (key: string, patch: Record<string, any>) => {
        setForms((current) => ({ ...current, [key]: { ...current[key], ...patch } }));
    };

    const toggleCollapse = (key: string) => {
        setCollapsed((current) => ({ ...current, [key]: !current[key] }));
    };

    const invalidateJourneyChildren = async () => {
        if (!selectedJourneyId) return;
        await queryClient.invalidateQueries({ queryKey: ['control-journey-states', selectedJourneyId] });
        await queryClient.invalidateQueries({ queryKey: ['control-journey-transitions', selectedJourneyId] });
        if (selectedScope) {
            await queryClient.invalidateQueries({ queryKey: ['control-scope-journey-states', selectedScope] });
        }
    };

    const invalidateVariableValues = async () => {
        if (!selectedVariableId) return;
        await queryClient.invalidateQueries({ queryKey: ['control-variable-values', selectedVariableId] });
    };

    const resetForm = (key: string) => {
        const resource = RESOURCES.find((item) => item.key === key);
        if (!resource) return;
        setForms((current) => ({ ...current, [key]: { ...resource.empty } }));
    };

    const refreshScope = async () => {
        await queryClient.invalidateQueries({ queryKey: ['control-scopes'] });
        if (selectedScope) {
            await queryClient.invalidateQueries({ queryKey: ['control-scope-details', selectedScope] });
        }
    };

    const runAction = async (key: string, success: string, fn: () => Promise<void>) => {
        setBusy(key);
        try {
            await fn();
            setFeedback({ type: 'success', message: success });
        } catch (error: any) {
            setFeedback({ type: 'error', message: error.message || 'Operation failed.' });
        } finally {
            setBusy((current) => (current === key ? null : current));
        }
    };

    useEffect(() => {
        if (!selectedScope && scopes.length) {
            setSelectedScope(scopes[0].scope_id);
        }
    }, [scopes, selectedScope]);

    useEffect(() => {
        const journeys = details?.journeys || [];
        if (!journeys.length) {
            setSelectedJourneyId('');
            return;
        }
        if (!selectedJourneyId || !journeys.some((item) => item.journey_id === selectedJourneyId)) {
            setSelectedJourneyId(journeys[0].journey_id);
        }
    }, [details?.journeys, selectedJourneyId]);

    useEffect(() => {
        const variables = details?.contextVariables || [];
        if (!variables.length) {
            setSelectedVariableId('');
            return;
        }
        if (!selectedVariableId || !variables.some((item) => item.variable_id === selectedVariableId)) {
            setSelectedVariableId(variables[0].variable_id);
        }
    }, [details?.contextVariables, selectedVariableId]);

    useEffect(() => {
        if (!feedback) return;
        const timer = window.setTimeout(() => setFeedback(null), 3200);
        return () => window.clearTimeout(timer);
    }, [feedback]);

    useEffect(() => {
        if (bindingForm.bind_type === 'scope' && bindingForm.bind_ref !== selectedScope) {
            setBindingForm((prev) => ({ ...prev, bind_ref: selectedScope }));
        }
    }, [bindingForm.bind_ref, bindingForm.bind_type, selectedScope]);

    const validateJsonField = (label: string, text: string) => {
        try {
            JSON.parse(text);
        } catch (error: any) {
            throw new Error(`${label} JSON is invalid: ${error.message}`);
        }
    };

    const saveScope = () => runAction('scope', 'Scope created.', async () => {
        await request('/control/scopes', { method: 'POST', body: JSON.stringify({ name: newScopeName.trim() }) });
        setNewScopeName('');
        await refreshScope();
    });

    const saveJourneyState = () => runAction('journey-state', journeyStateForm.id ? 'Journey state updated.' : 'Journey state created.', async () => {
        if (!selectedJourneyId) throw new Error('Select a journey first.');
        const payload = {
            name: journeyStateForm.name.trim(),
            description: journeyStateForm.description.trim() || null,
            required_fields: journeyStateForm.required_fields.split(',').map((value) => value.trim()).filter(Boolean),
            guideline_actions: journeyStateForm.guideline_actions.split('\n').map((value) => value.trim()).filter(Boolean),
        };
        await request(journeyStateForm.id ? `/control/journey-states/${journeyStateForm.id}` : `/control/journeys/${selectedJourneyId}/states`, {
            method: journeyStateForm.id ? 'PUT' : 'POST',
            body: JSON.stringify(payload),
        });
        setJourneyStateForm({ id: '', name: '', description: '', required_fields: '', guideline_actions: '' });
        await invalidateJourneyChildren();
        await refreshScope();
    });

    const deleteJourneyState = (state: any) => runAction(`journey-state-${state.state_id}`, 'Journey state deleted.', async () => {
        if (!window.confirm(`Delete journey state "${state.name}"?`)) return;
        await request(`/control/journey-states/${state.state_id}`, { method: 'DELETE' });
        if (journeyStateForm.id === state.state_id) {
            setJourneyStateForm({ id: '', name: '', description: '', required_fields: '', guideline_actions: '' });
        }
        await invalidateJourneyChildren();
        await refreshScope();
    });

    const saveJourneyTransition = () => runAction('journey-transition', journeyTransitionForm.id ? 'Journey transition updated.' : 'Journey transition created.', async () => {
        if (!selectedJourneyId) throw new Error('Select a journey first.');
        validateJsonField('Condition Config', journeyTransitionForm.condition_config);
        const payload = {
            from_state_id: journeyTransitionForm.from_state_id,
            to_state_id: journeyTransitionForm.to_state_id,
            transition_type: journeyTransitionForm.transition_type,
            condition_config: parseJson(journeyTransitionForm.condition_config, {}),
        };
        await request(journeyTransitionForm.id ? `/control/journey-transitions/${journeyTransitionForm.id}` : `/control/journeys/${selectedJourneyId}/transitions`, {
            method: journeyTransitionForm.id ? 'PUT' : 'POST',
            body: JSON.stringify(payload),
        });
        setJourneyTransitionForm({ id: '', from_state_id: '', to_state_id: '', transition_type: 'auto', condition_config: '{}' });
        await invalidateJourneyChildren();
    });

    const deleteJourneyTransition = (transition: any) => runAction(`journey-transition-${transition.transition_id}`, 'Journey transition deleted.', async () => {
        if (!window.confirm(`Delete transition ${transition.from_state_id} → ${transition.to_state_id}?`)) return;
        await request(`/control/journey-transitions/${transition.transition_id}`, { method: 'DELETE' });
        if (journeyTransitionForm.id === transition.transition_id) {
            setJourneyTransitionForm({ id: '', from_state_id: '', to_state_id: '', transition_type: 'auto', condition_config: '{}' });
        }
        await invalidateJourneyChildren();
    });

    const saveVariableValue = () => runAction('variable-value', 'Context variable value saved.', async () => {
        if (!selectedVariableId) throw new Error('Select a context variable first.');
        validateJsonField('Value Data', variableValueForm.data);
        await request(`/control/context-variables/${selectedVariableId}/values/${encodeURIComponent(variableValueForm.key.trim())}`, {
            method: 'PUT',
            body: JSON.stringify({ data: parseJson(variableValueForm.data, null) }),
        });
        setVariableValueForm({ key: '', data: '{\n  "value": ""\n}' });
        await invalidateVariableValues();
    });

    const deleteVariableValue = (value: any) => runAction(`variable-value-${value.key}`, 'Context variable value deleted.', async () => {
        if (!window.confirm(`Delete variable value "${value.key}"?`)) return;
        await request(`/control/context-variables/${value.variable_id}/values/${encodeURIComponent(value.key)}`, { method: 'DELETE' });
        await invalidateVariableValues();
    });

    const saveRelationship = () => runAction('relationship', 'Guideline relationship created.', async () => {
        if (!selectedScope) throw new Error('Select a scope first.');
        await request('/control/guideline-relationships', {
            method: 'POST',
            body: JSON.stringify({
                scope_id: selectedScope,
                from_guideline_id: relationshipForm.from_guideline_id,
                to_guideline_id: relationshipForm.to_guideline_id,
                relation_type: relationshipForm.relation_type,
            }),
        });
        setRelationshipForm({ from_guideline_id: '', to_guideline_id: '', relation_type: 'overrides' });
        await refreshScope();
    });

    const deleteRelationship = (relationship: any) => runAction(`relationship-${relationship.relationship_id}`, 'Guideline relationship deleted.', async () => {
        if (!window.confirm('Delete this guideline relationship?')) return;
        await request(`/control/guideline-relationships/${relationship.relationship_id}`, { method: 'DELETE' });
        await refreshScope();
    });

    const saveToolPolicy = () => runAction('tool-policy', toolPolicyForm.id ? 'Tool policy updated.' : 'Tool policy created.', async () => {
        if (!selectedScope) throw new Error('Select a scope first.');
        const payload = {
            scope_id: selectedScope,
            tool_name: toolPolicyForm.tool_name.trim(),
            skill_ref: toolPolicyForm.skill_ref.trim() || null,
            observation_ref: toolPolicyForm.observation_ref.trim() || null,
            journey_state_ref: toolPolicyForm.journey_state_ref.trim() || null,
            guideline_ref: toolPolicyForm.guideline_ref.trim() || null,
            approval_mode: toolPolicyForm.approval_mode,
            enabled: toolPolicyForm.enabled,
        };
        await request(toolPolicyForm.id ? `/control/tool-policies/${toolPolicyForm.id}` : '/control/tool-policies', {
            method: toolPolicyForm.id ? 'PUT' : 'POST',
            body: JSON.stringify(payload),
        });
        setToolPolicyForm({ id: '', tool_name: '', skill_ref: '', observation_ref: '', journey_state_ref: '', guideline_ref: '', approval_mode: 'none', enabled: true });
        await refreshScope();
    });

    const deleteToolPolicy = (policy: any) => runAction(`tool-policy-${policy.policy_id}`, 'Tool policy deleted.', async () => {
        if (!window.confirm('Delete this tool policy?')) return;
        await request(`/control/tool-policies/${policy.policy_id}`, { method: 'DELETE' });
        if (toolPolicyForm.id === policy.policy_id) {
            setToolPolicyForm({ id: '', tool_name: '', skill_ref: '', observation_ref: '', journey_state_ref: '', guideline_ref: '', approval_mode: 'none', enabled: true });
        }
        await refreshScope();
    });

    const saveRetriever = () => runAction('retriever', retrieverForm.id ? 'Retriever updated.' : 'Retriever created.', async () => {
        if (!selectedScope) throw new Error('Select a scope first.');
        validateJsonField('Retriever Config', retrieverForm.config_json);
        const payload = {
            scope_id: selectedScope,
            name: retrieverForm.name.trim(),
            retriever_type: retrieverForm.retriever_type,
            config_json: parseJson(retrieverForm.config_json, {}),
            enabled: retrieverForm.enabled,
        };
        await request(retrieverForm.id ? `/control/retrievers/${retrieverForm.id}` : '/control/retrievers', {
            method: retrieverForm.id ? 'PUT' : 'POST',
            body: JSON.stringify(payload),
        });
        setRetrieverForm({ id: '', name: '', retriever_type: 'static', config_json: '{\n  "items": []\n}', enabled: true });
        await refreshScope();
    });

    const deleteRetriever = (retriever: any) => runAction(`retriever-${retriever.retriever_id}`, 'Retriever deleted.', async () => {
        if (!window.confirm('Delete this retriever and its bindings?')) return;
        await request(`/control/retrievers/${retriever.retriever_id}`, { method: 'DELETE' });
        if (retrieverForm.id === retriever.retriever_id) {
            setRetrieverForm({ id: '', name: '', retriever_type: 'static', config_json: '{\n  "items": []\n}', enabled: true });
        }
        await refreshScope();
    });

    const saveRetrieverBinding = () => runAction('retriever-binding', 'Retriever binding created.', async () => {
        if (!selectedScope) throw new Error('Select a scope first.');
        await request('/control/retriever-bindings', {
            method: 'POST',
            body: JSON.stringify({
                scope_id: selectedScope,
                retriever_id: bindingForm.retriever_id,
                bind_type: bindingForm.bind_type,
                bind_ref: bindingForm.bind_ref.trim(),
            }),
        });
        setBindingForm({ retriever_id: '', bind_type: 'always', bind_ref: 'always' });
        await refreshScope();
    });

    const deleteRetrieverBinding = (binding: any) => runAction(`retriever-binding-${binding.binding_id}`, 'Retriever binding deleted.', async () => {
        if (!window.confirm('Delete this retriever binding?')) return;
        await request(`/control/retriever-bindings/${binding.binding_id}`, { method: 'DELETE' });
        await refreshScope();
    });

    const publishRelease = () => runAction('release-publish', 'Release published.', async () => {
        if (!selectedScope) throw new Error('Select a scope first.');
        await request('/control/releases/publish', {
            method: 'POST',
            body: JSON.stringify({ scope_id: selectedScope, version: releaseVersion.trim(), published_by: 'ui' }),
        });
        setReleaseVersion('');
        await refreshScope();
    });

    const rollbackRelease = () => runAction('release-rollback', 'Rollback triggered.', async () => {
        if (!selectedScope) throw new Error('Select a scope first.');
        if (!window.confirm('Rollback the current published release?')) return;
        await request('/control/releases/rollback', {
            method: 'POST',
            body: JSON.stringify({ scope_id: selectedScope }),
        });
        await refreshScope();
    });

    const saveResource = (resource: typeof RESOURCES[number]) => runAction(
        resource.key,
        `${toSingularTitle(resource.title)} ${forms[resource.key].id ? 'updated' : 'created'}.`,
        async () => {
            const form = forms[resource.key];
            resource.fields
                .filter((field) => field.kind === 'textarea' && field.key.includes('config'))
                .forEach((field) => validateJsonField(field.label, String(form[field.key] ?? '')));
            await request(form.id ? resource.itemPath(form.id) : resource.createPath, {
                method: form.id ? 'PUT' : 'POST',
                body: JSON.stringify(resource.encode(form, selectedScope)),
            });
            resetForm(resource.key);
            await refreshScope();
        },
    );

    const deleteResource = (resource: typeof RESOURCES[number], item: any) => runAction(
        `${resource.key}-${item[resource.idField]}`,
        `${toSingularTitle(resource.title)} deleted.`,
        async () => {
            if (!window.confirm(`Delete this ${toSingularTitle(resource.title).toLowerCase()}?`)) return;
            await request(resource.itemPath(item[resource.idField]), { method: 'DELETE' });
            if (forms[resource.key].id === item[resource.idField]) {
                resetForm(resource.key);
            }
            await refreshScope();
        },
    );

    const stats = [
        ['Observations', details?.observations.length || 0],
        ['Guidelines', details?.guidelines.length || 0],
        ['Journeys', details?.journeys.length || 0],
        ['Glossary', details?.glossaryTerms.length || 0],
        ['Variables', details?.contextVariables.length || 0],
        ['Canned', details?.cannedResponses.length || 0],
        ['Policies', details?.toolPolicies.length || 0],
        ['Releases', details?.releases.length || 0],
    ];

    return (
        <div>
            <div className="page-header">
                <div>
                    <h1 className="page-title">Control Plane</h1>
                    <div className="page-subtitle">把控制面对象真正接进 React 控制台，先覆盖顶层对象 CRUD。</div>
                </div>
            </div>

            {feedback && (
                <div
                    className="card"
                    style={{
                        marginBottom: '16px',
                        padding: '12px 16px',
                        borderColor: feedback.type === 'success' ? 'var(--success)' : 'var(--error)',
                        background: feedback.type === 'success' ? 'var(--success-subtle)' : 'var(--error-subtle)',
                        color: feedback.type === 'success' ? 'var(--success)' : 'var(--error)',
                    }}
                >
                    {feedback.message}
                </div>
            )}

            <div style={{ display: 'grid', gridTemplateColumns: '300px 1fr', gap: '16px', alignItems: 'start' }}>
                <ScopeSidebar
                    scopes={scopes}
                    isLoading={scopesQuery.isLoading}
                    selectedScope={selectedScope}
                    newScopeName={newScopeName}
                    busy={busy}
                    onNewScopeNameChange={setNewScopeName}
                    onCreateScope={saveScope}
                    onSelectScope={setSelectedScope}
                />

                <div style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
                    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(8, minmax(0, 1fr))', gap: '12px' }}>
                        {stats.map(([label, value]) => (
                            <div key={String(label)} className="card" style={{ padding: '16px' }}>
                                <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginBottom: '6px' }}>{label}</div>
                                <div style={{ fontSize: '24px', fontWeight: 700 }}>{value as number}</div>
                            </div>
                        ))}
                    </div>

                    <div className="tabs" style={{ marginBottom: 0 }}>
                        {TABS.map((tab) => (
                            <button key={tab.key} className={`tab ${selectedTab === tab.key ? 'active' : ''}`} onClick={() => setSelectedTab(tab.key)}>
                                {tab.label}
                            </button>
                        ))}
                    </div>

                    <Card title="Search" subtitle="按名称、配置内容和描述过滤当前页对象。">
                        <input className="form-input" placeholder={`Search ${selectedTab}...`} value={searchText} onChange={(event) => setSearchText(event.target.value)} />
                    </Card>

                    {selectedTab !== 'knowledge' && activeResource && (
                        <ResourceSection
                            resource={activeResource}
                            details={details}
                            forms={forms}
                            searchText={searchText}
                            selectedScope={selectedScope}
                            busy={busy}
                            collapsed={collapsed}
                            onToggleCollapse={toggleCollapse}
                            onUpdateForm={updateForm}
                            onSaveResource={saveResource}
                            onResetForm={resetForm}
                            onDeleteResource={deleteResource}
                            onSelectJourney={setSelectedJourneyId}
                            onSelectVariable={setSelectedVariableId}
                        />
                    )}

                    {selectedTab === 'journeys' && (
                        <JourneySection
                            details={details}
                            selectedJourneyId={selectedJourneyId}
                            journeyStates={journeyStatesQuery.data || []}
                            journeyTransitions={journeyTransitionsQuery.data || []}
                            searchText={searchText}
                            busy={busy}
                            journeyStateForm={journeyStateForm}
                            setJourneyStateForm={setJourneyStateForm}
                            journeyTransitionForm={journeyTransitionForm}
                            setJourneyTransitionForm={setJourneyTransitionForm}
                            onSaveJourneyState={saveJourneyState}
                            onDeleteJourneyState={deleteJourneyState}
                            onSaveJourneyTransition={saveJourneyTransition}
                            onDeleteJourneyTransition={deleteJourneyTransition}
                        />
                    )}

                    {selectedTab === 'knowledge' && (
                        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '16px', alignItems: 'start' }}>
                            {knowledgeResources.map((resource) => (
                                <ResourceSection
                                    key={resource.key}
                                    resource={resource}
                                    details={details}
                                    forms={forms}
                                    searchText={searchText}
                                    selectedScope={selectedScope}
                                    busy={busy}
                                    collapsed={collapsed}
                                    onToggleCollapse={toggleCollapse}
                                    onUpdateForm={updateForm}
                                    onSaveResource={saveResource}
                                    onResetForm={resetForm}
                                    onDeleteResource={deleteResource}
                                    onSelectJourney={setSelectedJourneyId}
                                    onSelectVariable={setSelectedVariableId}
                                    stacked
                                />
                            ))}
                        </div>
                    )}

                    {selectedTab === 'knowledge' && (
                        <VariableValuesSection
                            details={details}
                            selectedVariableId={selectedVariableId}
                            variableValues={variableValuesQuery.data || []}
                            searchText={searchText}
                            busy={busy}
                            variableValueForm={variableValueForm}
                            setVariableValueForm={setVariableValueForm}
                            onSaveVariableValue={saveVariableValue}
                            onDeleteVariableValue={deleteVariableValue}
                        />
                    )}

                    {selectedTab === 'governance' && (
                        <GovernanceSection
                            details={details}
                            selectedScope={selectedScope}
                            searchText={searchText}
                            busy={busy}
                            journeyStates={governanceJourneyStatesQuery.data || []}
                            relationshipForm={relationshipForm}
                            setRelationshipForm={setRelationshipForm}
                            toolPolicyForm={toolPolicyForm}
                            setToolPolicyForm={setToolPolicyForm}
                            retrieverForm={retrieverForm}
                            setRetrieverForm={setRetrieverForm}
                            bindingForm={bindingForm}
                            setBindingForm={setBindingForm}
                            releaseVersion={releaseVersion}
                            setReleaseVersion={setReleaseVersion}
                            onSaveRelationship={saveRelationship}
                            onDeleteRelationship={deleteRelationship}
                            onSaveToolPolicy={saveToolPolicy}
                            onDeleteToolPolicy={deleteToolPolicy}
                            onSaveRetriever={saveRetriever}
                            onDeleteRetriever={deleteRetriever}
                            onSaveRetrieverBinding={saveRetrieverBinding}
                            onDeleteRetrieverBinding={deleteRetrieverBinding}
                            onPublishRelease={publishRelease}
                            onRollbackRelease={rollbackRelease}
                        />
                    )}
                </div>
            </div>
        </div>
    );
}
