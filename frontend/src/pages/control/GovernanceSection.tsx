import type { BindingForm, RelationshipForm, RetrieverForm, ScopeDetails, StateSetter, ToolPolicyForm } from './types';
import Card from './Card';
import { matchesSearch, prettyJsonValue } from './utils';

type GovernanceSectionProps = {
    details?: ScopeDetails;
    selectedScope: string;
    searchText: string;
    busy: string | null;
    journeyStates: any[];
    relationshipForm: RelationshipForm;
    setRelationshipForm: StateSetter<RelationshipForm>;
    toolPolicyForm: ToolPolicyForm;
    setToolPolicyForm: StateSetter<ToolPolicyForm>;
    retrieverForm: RetrieverForm;
    setRetrieverForm: StateSetter<RetrieverForm>;
    bindingForm: BindingForm;
    setBindingForm: StateSetter<BindingForm>;
    releaseVersion: string;
    setReleaseVersion: (value: string) => void;
    onSaveRelationship: () => void;
    onDeleteRelationship: (relationship: any) => void;
    onSaveToolPolicy: () => void;
    onDeleteToolPolicy: (policy: any) => void;
    onSaveRetriever: () => void;
    onDeleteRetriever: (retriever: any) => void;
    onSaveRetrieverBinding: () => void;
    onDeleteRetrieverBinding: (binding: any) => void;
    onPublishRelease: () => void;
    onRollbackRelease: () => void;
};

export default function GovernanceSection({
    details,
    selectedScope,
    searchText,
    busy,
    journeyStates,
    relationshipForm,
    setRelationshipForm,
    toolPolicyForm,
    setToolPolicyForm,
    retrieverForm,
    setRetrieverForm,
    bindingForm,
    setBindingForm,
    releaseVersion,
    setReleaseVersion,
    onSaveRelationship,
    onDeleteRelationship,
    onSaveToolPolicy,
    onDeleteToolPolicy,
    onSaveRetriever,
    onDeleteRetriever,
    onSaveRetrieverBinding,
    onDeleteRetrieverBinding,
    onPublishRelease,
    onRollbackRelease,
}: GovernanceSectionProps) {
    const observations = details?.observations || [];
    const guidelines = details?.guidelines || [];
    const retrievers = details?.retrievers || [];
    const relationships = (details?.guidelineRelationships || []).filter((item) => matchesSearch(item, searchText));
    const toolPolicies = (details?.toolPolicies || []).filter((item) => matchesSearch(item, searchText));
    const visibleRetrievers = retrievers.filter((item) => matchesSearch(item, searchText));
    const retrieverBindings = (details?.retrieverBindings || []).filter((item) => matchesSearch(item, searchText));
    const releases = (details?.releases || []).filter((item) => matchesSearch(item, searchText));
    const activeRelease = (details?.releases || []).find((item) => item.status === 'published');

    const observationNameById = new Map(observations.map((item) => [item.observation_id, item.name]));
    const guidelineNameById = new Map(guidelines.map((item) => [item.guideline_id, item.name]));
    const retrieverNameById = new Map(retrievers.map((item) => [item.retriever_id, item.name]));
    const journeyStateById = new Map(journeyStates.map((item) => [item.state_id, item]));

    const relationLabel = (relationType: string) => {
        switch (relationType) {
            case 'overrides':
                return 'Overrides';
            case 'conflicts_with':
                return 'Conflicts With';
            case 'requires':
                return 'Requires';
            case 'complements':
                return 'Complements';
            default:
                return relationType;
        }
    };

    const getToolPolicyTargetType = (policy: ToolPolicyForm | any) => {
        if (policy.observation_ref) return 'observation';
        if (policy.guideline_ref) return 'guideline';
        if (policy.journey_state_ref) return 'journey_state';
        if (policy.skill_ref) return 'skill';
        return 'global';
    };

    const toolPolicyTargetType = getToolPolicyTargetType(toolPolicyForm);

    const setToolPolicyTargetType = (targetType: string) => {
        setToolPolicyForm((prev) => ({
            ...prev,
            skill_ref: '',
            observation_ref: '',
            journey_state_ref: '',
            guideline_ref: '',
        }));
    };

    const setToolPolicyTargetValue = (targetType: string, value: string) => {
        setToolPolicyForm((prev) => ({
            ...prev,
            skill_ref: targetType === 'skill' ? value : '',
            observation_ref: targetType === 'observation' ? value : '',
            journey_state_ref: targetType === 'journey_state' ? value : '',
            guideline_ref: targetType === 'guideline' ? value : '',
        }));
    };

    const bindTypeLabel = (bindType: string) => {
        switch (bindType) {
            case 'always':
                return 'Always';
            case 'scope':
                return 'Scope';
            case 'guideline':
                return 'Guideline';
            case 'journey_state':
                return 'Journey State';
            default:
                return bindType;
        }
    };

    const renderBindingRefInput = () => {
        if (bindingForm.bind_type === 'always') {
            return (
                <div className="form-group">
                    <label className="form-label">Bind Ref</label>
                    <input className="form-input" value="always" disabled />
                </div>
            );
        }

        if (bindingForm.bind_type === 'scope') {
            return (
                <div className="form-group">
                    <label className="form-label">Bind Ref</label>
                    <input className="form-input" value={selectedScope || 'current scope'} disabled />
                </div>
            );
        }

        if (bindingForm.bind_type === 'guideline') {
            return (
                <div className="form-group">
                    <label className="form-label">Bind Guideline</label>
                    <select className="form-input" value={bindingForm.bind_ref} onChange={(event) => setBindingForm((prev) => ({ ...prev, bind_ref: event.target.value }))}>
                        <option value="">Select guideline</option>
                        {guidelines.map((item) => <option key={item.guideline_id} value={item.name}>{item.name}</option>)}
                    </select>
                </div>
            );
        }

        return (
            <div className="form-group">
                <label className="form-label">Bind Journey State</label>
                <select className="form-input" value={bindingForm.bind_ref} onChange={(event) => setBindingForm((prev) => ({ ...prev, bind_ref: event.target.value }))}>
                    <option value="">Select journey state</option>
                    {journeyStates.map((item) => (
                        <option key={item.state_id} value={item.state_id}>
                            {item.name}{item.journey_name ? ` · ${item.journey_name}` : ''}
                        </option>
                    ))}
                </select>
            </div>
        );
    };

    const resolveBindingRef = (item: any) => {
        if (item.bind_type === 'always') return 'Always active';
        if (item.bind_type === 'scope') return 'Whole scope';
        if (item.bind_type === 'guideline') return item.bind_ref;
        if (item.bind_type === 'journey_state') {
            const state = journeyStateById.get(item.bind_ref);
            return state ? `${state.name}${state.journey_name ? ` · ${state.journey_name}` : ''}` : item.bind_ref;
        }
        return item.bind_ref;
    };

    const resolvePolicyTarget = (item: any) => {
        if (item.observation_ref) {
            return `Observation · ${observationNameById.get(item.observation_ref) || item.observation_ref}`;
        }
        if (item.guideline_ref) {
            return `Guideline · ${guidelineNameById.get(item.guideline_ref) || item.guideline_ref}`;
        }
        if (item.journey_state_ref) {
            const state = journeyStateById.get(item.journey_state_ref);
            return `Journey State · ${state ? `${state.name}${state.journey_name ? ` · ${state.journey_name}` : ''}` : item.journey_state_ref}`;
        }
        if (item.skill_ref) {
            return `Skill · ${item.skill_ref}`;
        }
        return 'Global';
    };

    const formatReleaseStatus = (status: string) => {
        switch (status) {
            case 'published':
                return { label: 'Published', color: 'var(--success)', background: 'var(--success-subtle)' };
            case 'rolled_back':
                return { label: 'Rolled Back', color: 'var(--warning)', background: 'rgba(245, 158, 11, 0.12)' };
            default:
                return { label: status, color: 'var(--text-secondary)', background: 'var(--bg-secondary)' };
        }
    };

    return (
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '16px' }}>
            <Card title="Guideline Relationships" subtitle="建立规则间依赖、排斥与优先级关系。">
                <div className="form-group">
                    <label className="form-label">From Guideline</label>
                    <select className="form-input" value={relationshipForm.from_guideline_id} onChange={(event) => setRelationshipForm((prev) => ({ ...prev, from_guideline_id: event.target.value }))}>
                        <option value="">Select guideline</option>
                        {guidelines.map((item) => <option key={item.guideline_id} value={item.guideline_id}>{item.name}</option>)}
                    </select>
                </div>
                <div className="form-group">
                    <label className="form-label">To Guideline</label>
                    <select className="form-input" value={relationshipForm.to_guideline_id} onChange={(event) => setRelationshipForm((prev) => ({ ...prev, to_guideline_id: event.target.value }))}>
                        <option value="">Select guideline</option>
                        {guidelines.map((item) => <option key={item.guideline_id} value={item.guideline_id}>{item.name}</option>)}
                    </select>
                </div>
                <div className="form-group">
                    <label className="form-label">Relation Type</label>
                    <select className="form-input" value={relationshipForm.relation_type} onChange={(event) => setRelationshipForm((prev) => ({ ...prev, relation_type: event.target.value }))}>
                        <option value="overrides">Overrides</option>
                        <option value="conflicts_with">Conflicts With</option>
                        <option value="requires">Requires</option>
                        <option value="complements">Complements</option>
                    </select>
                </div>
                <button className="btn btn-primary" disabled={!relationshipForm.from_guideline_id || !relationshipForm.to_guideline_id || relationshipForm.from_guideline_id === relationshipForm.to_guideline_id || busy === 'relationship'} onClick={onSaveRelationship}>
                    {busy === 'relationship' ? 'Saving...' : 'Create Relationship'}
                </button>
                <div style={{ display: 'flex', flexDirection: 'column', gap: '10px', marginTop: '16px' }}>
                    {relationships.map((item) => (
                        <div key={item.relationship_id} style={{ border: '1px solid var(--border-subtle)', borderRadius: '10px', padding: '10px 12px', display: 'flex', justifyContent: 'space-between', gap: '8px' }}>
                            <div>
                                <div style={{ fontSize: '13px', fontWeight: 600 }}>
                                    {guidelineNameById.get(item.from_guideline_id) || item.from_guideline_id} → {guidelineNameById.get(item.to_guideline_id) || item.to_guideline_id}
                                </div>
                                <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{relationLabel(item.relation_type)}</div>
                            </div>
                            <button className="btn btn-danger" disabled={busy === `relationship-${item.relationship_id}`} onClick={() => onDeleteRelationship(item)}>Delete</button>
                        </div>
                    ))}
                </div>
            </Card>

            <Card title="Tool Policies" subtitle="将工具暴露绑定到 observation / guideline / journey state。">
                <div className="form-group">
                    <label className="form-label">Tool Name</label>
                    <input className="form-input" value={toolPolicyForm.tool_name} onChange={(event) => setToolPolicyForm((prev) => ({ ...prev, tool_name: event.target.value }))} />
                </div>
                <div className="form-group">
                    <label className="form-label">Target Type</label>
                    <select className="form-input" value={toolPolicyTargetType} onChange={(event) => setToolPolicyTargetType(event.target.value)}>
                        <option value="global">Global</option>
                        <option value="skill">Skill</option>
                        <option value="observation">Observation</option>
                        <option value="guideline">Guideline</option>
                        <option value="journey_state">Journey State</option>
                    </select>
                </div>
                {toolPolicyTargetType === 'skill' && (
                    <div className="form-group">
                        <label className="form-label">Skill Ref</label>
                        <input className="form-input" value={toolPolicyForm.skill_ref} onChange={(event) => setToolPolicyTargetValue('skill', event.target.value)} />
                    </div>
                )}
                {toolPolicyTargetType === 'observation' && (
                    <div className="form-group">
                        <label className="form-label">Observation Ref</label>
                        <select className="form-input" value={toolPolicyForm.observation_ref} onChange={(event) => setToolPolicyTargetValue('observation', event.target.value)}>
                            <option value="">Select observation</option>
                            {observations.map((item) => <option key={item.observation_id} value={item.observation_id}>{item.name}</option>)}
                        </select>
                    </div>
                )}
                {toolPolicyTargetType === 'guideline' && (
                    <div className="form-group">
                        <label className="form-label">Guideline Ref</label>
                        <select className="form-input" value={toolPolicyForm.guideline_ref} onChange={(event) => setToolPolicyTargetValue('guideline', event.target.value)}>
                            <option value="">Select guideline</option>
                            {guidelines.map((item) => <option key={item.guideline_id} value={item.guideline_id}>{item.name}</option>)}
                        </select>
                    </div>
                )}
                {toolPolicyTargetType === 'journey_state' && (
                    <div className="form-group">
                        <label className="form-label">Journey State Ref</label>
                        <select className="form-input" value={toolPolicyForm.journey_state_ref} onChange={(event) => setToolPolicyTargetValue('journey_state', event.target.value)}>
                            <option value="">Select journey state</option>
                            {journeyStates.map((item) => (
                                <option key={item.state_id} value={item.state_id}>
                                    {item.name}{item.journey_name ? ` · ${item.journey_name}` : ''}
                                </option>
                            ))}
                        </select>
                    </div>
                )}
                <div className="form-group">
                    <label className="form-label">Approval Mode</label>
                    <select className="form-input" value={toolPolicyForm.approval_mode} onChange={(event) => setToolPolicyForm((prev) => ({ ...prev, approval_mode: event.target.value }))}>
                        <option value="none">none</option>
                        <option value="required">required</option>
                        <option value="conditional">conditional</option>
                    </select>
                </div>
                <label style={{ display: 'flex', gap: '8px', marginBottom: '12px', fontSize: '13px' }}>
                    <input type="checkbox" checked={toolPolicyForm.enabled} onChange={(event) => setToolPolicyForm((prev) => ({ ...prev, enabled: event.target.checked }))} />
                    Enabled
                </label>
                <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginBottom: '12px' }}>
                    每条 tool policy 只绑定一个目标类型，避免 observation / guideline / journey state 混配造成歧义。
                </div>
                <div style={{ display: 'flex', gap: '8px', marginBottom: '16px' }}>
                    <button className="btn btn-primary" disabled={!toolPolicyForm.tool_name.trim() || busy === 'tool-policy'} onClick={onSaveToolPolicy}>
                        {busy === 'tool-policy' ? 'Saving...' : toolPolicyForm.id ? 'Update' : 'Create'}
                    </button>
                    {toolPolicyForm.id && (
                        <button className="btn btn-secondary" onClick={() => setToolPolicyForm({ id: '', tool_name: '', skill_ref: '', observation_ref: '', journey_state_ref: '', guideline_ref: '', approval_mode: 'none', enabled: true })}>
                            Cancel
                        </button>
                    )}
                </div>
                <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                    {toolPolicies.map((item) => (
                        <div key={item.policy_id} style={{ border: '1px solid var(--border-subtle)', borderRadius: '10px', padding: '10px 12px', display: 'flex', justifyContent: 'space-between', gap: '8px' }}>
                            <div>
                                <div style={{ fontSize: '13px', fontWeight: 600 }}>{item.tool_name}</div>
                                <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{item.approval_mode} · {resolvePolicyTarget(item)}</div>
                            </div>
                            <div style={{ display: 'flex', gap: '8px' }}>
                                <button
                                    className="btn btn-secondary"
                                    onClick={() => setToolPolicyForm({
                                        id: item.policy_id,
                                        tool_name: item.tool_name || '',
                                        skill_ref: getToolPolicyTargetType(item) === 'skill' ? (item.skill_ref || '') : '',
                                        observation_ref: getToolPolicyTargetType(item) === 'observation' ? (item.observation_ref || '') : '',
                                        journey_state_ref: getToolPolicyTargetType(item) === 'journey_state' ? (item.journey_state_ref || '') : '',
                                        guideline_ref: getToolPolicyTargetType(item) === 'guideline' ? (item.guideline_ref || '') : '',
                                        approval_mode: item.approval_mode || 'none',
                                        enabled: !!item.enabled,
                                    })}
                                >
                                    Edit
                                </button>
                                <button className="btn btn-danger" disabled={busy === `tool-policy-${item.policy_id}`} onClick={() => onDeleteToolPolicy(item)}>Delete</button>
                            </div>
                        </div>
                    ))}
                </div>
            </Card>

            <Card title="Retrievers" subtitle="配置检索源并绑定到规则或 journey 上下文。">
                <div className="form-group">
                    <label className="form-label">Name</label>
                    <input className="form-input" value={retrieverForm.name} onChange={(event) => setRetrieverForm((prev) => ({ ...prev, name: event.target.value }))} />
                </div>
                <div className="form-group">
                    <label className="form-label">Retriever Type</label>
                    <select className="form-input" value={retrieverForm.retriever_type} onChange={(event) => setRetrieverForm((prev) => ({ ...prev, retriever_type: event.target.value }))}>
                        <option value="static">static</option>
                        <option value="faq_sqlite">faq_sqlite</option>
                        <option value="embedding">embedding</option>
                    </select>
                </div>
                <div className="form-group">
                    <label className="form-label">Config JSON</label>
                    <textarea className="form-textarea" value={retrieverForm.config_json} onChange={(event) => setRetrieverForm((prev) => ({ ...prev, config_json: event.target.value }))} />
                </div>
                <label style={{ display: 'flex', gap: '8px', marginBottom: '12px', fontSize: '13px' }}>
                    <input type="checkbox" checked={retrieverForm.enabled} onChange={(event) => setRetrieverForm((prev) => ({ ...prev, enabled: event.target.checked }))} />
                    Enabled
                </label>
                <div style={{ display: 'flex', gap: '8px', marginBottom: '16px' }}>
                    <button className="btn btn-primary" disabled={!retrieverForm.name.trim() || busy === 'retriever'} onClick={onSaveRetriever}>
                        {busy === 'retriever' ? 'Saving...' : retrieverForm.id ? 'Update' : 'Create'}
                    </button>
                    {retrieverForm.id && (
                        <button className="btn btn-secondary" onClick={() => setRetrieverForm({ id: '', name: '', retriever_type: 'static', config_json: '{\n  "items": []\n}', enabled: true })}>
                            Cancel
                        </button>
                    )}
                </div>
                <div className="form-group">
                    <label className="form-label">Binding Retriever</label>
                    <select className="form-input" value={bindingForm.retriever_id} onChange={(event) => setBindingForm((prev) => ({ ...prev, retriever_id: event.target.value }))}>
                        <option value="">Select retriever</option>
                        {retrievers.map((item) => <option key={item.retriever_id} value={item.retriever_id}>{item.name}</option>)}
                    </select>
                </div>
                <div className="form-group">
                    <label className="form-label">Bind Type</label>
                    <select
                        className="form-input"
                        value={bindingForm.bind_type}
                        onChange={(event) => {
                            const bindType = event.target.value;
                            const defaultBindRef =
                                bindType === 'always' ? 'always' :
                                    bindType === 'scope' ? selectedScope :
                                        '';
                            setBindingForm((prev) => ({ ...prev, bind_type: bindType, bind_ref: defaultBindRef }));
                        }}
                    >
                        <option value="always">Always</option>
                        <option value="scope">Scope</option>
                        <option value="guideline">Guideline</option>
                        <option value="journey_state">Journey State</option>
                    </select>
                </div>
                {renderBindingRefInput()}
                <button className="btn btn-secondary" disabled={!bindingForm.retriever_id || !bindingForm.bind_ref || busy === 'retriever-binding'} onClick={onSaveRetrieverBinding}>
                    {busy === 'retriever-binding' ? 'Saving...' : 'Create Binding'}
                </button>
                <div style={{ display: 'flex', flexDirection: 'column', gap: '10px', marginTop: '16px' }}>
                    {visibleRetrievers.map((item) => (
                        <div key={item.retriever_id} style={{ border: '1px solid var(--border-subtle)', borderRadius: '10px', padding: '10px 12px' }}>
                            <div style={{ display: 'flex', justifyContent: 'space-between', gap: '8px' }}>
                                <div style={{ minWidth: 0 }}>
                                    <div style={{ fontSize: '13px', fontWeight: 600 }}>{item.name}</div>
                                    <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{item.retriever_type} · {item.enabled ? 'enabled' : 'disabled'}</div>
                                </div>
                                <div style={{ display: 'flex', gap: '8px' }}>
                                    <button
                                        className="btn btn-secondary"
                                        onClick={() => setRetrieverForm({
                                            id: item.retriever_id,
                                            name: item.name,
                                            retriever_type: item.retriever_type || 'static',
                                            config_json: prettyJsonValue(item.config_json || {}, {}),
                                            enabled: !!item.enabled,
                                        })}
                                    >
                                        Edit
                                    </button>
                                    <button className="btn btn-danger" disabled={busy === `retriever-${item.retriever_id}`} onClick={() => onDeleteRetriever(item)}>Delete</button>
                                </div>
                            </div>
                        </div>
                    ))}
                    {retrieverBindings.map((item) => (
                        <div key={item.binding_id} style={{ border: '1px dashed var(--border-strong)', borderRadius: '10px', padding: '10px 12px', display: 'flex', justifyContent: 'space-between', gap: '8px' }}>
                            <div>
                                <div style={{ fontSize: '13px', fontWeight: 600 }}>{retrieverNameById.get(item.retriever_id) || item.retriever_id}</div>
                                <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{bindTypeLabel(item.bind_type)} · {resolveBindingRef(item)}</div>
                            </div>
                            <button className="btn btn-danger" disabled={busy === `retriever-binding-${item.binding_id}`} onClick={() => onDeleteRetrieverBinding(item)}>Delete</button>
                        </div>
                    ))}
                </div>
            </Card>

            <Card title="Releases" subtitle="发布控制面版本并在必要时回滚。">
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '10px', marginBottom: '16px' }}>
                    <div style={{ border: '1px solid var(--border-subtle)', borderRadius: '10px', padding: '10px 12px' }}>
                        <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginBottom: '4px' }}>Current Published</div>
                        <div style={{ fontSize: '13px', fontWeight: 600 }}>{activeRelease?.version || 'None'}</div>
                    </div>
                    <div style={{ border: '1px solid var(--border-subtle)', borderRadius: '10px', padding: '10px 12px' }}>
                        <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginBottom: '4px' }}>Published By</div>
                        <div style={{ fontSize: '13px', fontWeight: 600 }}>{activeRelease?.published_by || '-'}</div>
                    </div>
                </div>
                <div className="form-group">
                    <label className="form-label">New Release Version</label>
                    <input className="form-input" value={releaseVersion} onChange={(event) => setReleaseVersion(event.target.value)} placeholder="v2026.03.23-1" />
                </div>
                <div style={{ display: 'flex', gap: '8px', marginBottom: '16px' }}>
                    <button className="btn btn-primary" disabled={!releaseVersion.trim() || busy === 'release-publish'} onClick={onPublishRelease}>
                        {busy === 'release-publish' ? 'Publishing...' : 'Publish Release'}
                    </button>
                    <button className="btn btn-danger" disabled={!activeRelease || busy === 'release-rollback'} onClick={onRollbackRelease}>
                        {busy === 'release-rollback' ? 'Rolling back...' : 'Rollback Latest'}
                    </button>
                </div>
                <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                    {releases.map((item) => (
                        <div key={item.release_id} style={{ border: '1px solid var(--border-subtle)', borderRadius: '10px', padding: '10px 12px' }}>
                            <div style={{ display: 'flex', justifyContent: 'space-between', gap: '8px', alignItems: 'center' }}>
                                <div style={{ fontSize: '13px', fontWeight: 600 }}>{item.version}</div>
                                <span
                                    style={{
                                        fontSize: '11px',
                                        padding: '4px 8px',
                                        borderRadius: '999px',
                                        color: formatReleaseStatus(item.status).color,
                                        background: formatReleaseStatus(item.status).background,
                                    }}
                                >
                                    {formatReleaseStatus(item.status).label}
                                </span>
                            </div>
                            <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '6px' }}>{item.published_by}</div>
                            <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{new Date(item.created_at).toLocaleString()}</div>
                        </div>
                    ))}
                    {!details?.releases?.length && <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>No releases yet.</div>}
                </div>
            </Card>
        </div>
    );
}
