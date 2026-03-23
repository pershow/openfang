import type { JourneyStateForm, JourneyTransitionForm, ScopeDetails, StateSetter } from './types';
import Card from './Card';
import { matchesSearch, pretty } from './utils';

type JourneySectionProps = {
    details?: ScopeDetails;
    selectedJourneyId: string;
    journeyStates: any[];
    journeyTransitions: any[];
    searchText: string;
    busy: string | null;
    journeyStateForm: JourneyStateForm;
    setJourneyStateForm: StateSetter<JourneyStateForm>;
    journeyTransitionForm: JourneyTransitionForm;
    setJourneyTransitionForm: StateSetter<JourneyTransitionForm>;
    onSaveJourneyState: () => void;
    onDeleteJourneyState: (state: any) => void;
    onSaveJourneyTransition: () => void;
    onDeleteJourneyTransition: (transition: any) => void;
};

export default function JourneySection({
    details,
    selectedJourneyId,
    journeyStates,
    journeyTransitions,
    searchText,
    busy,
    journeyStateForm,
    setJourneyStateForm,
    journeyTransitionForm,
    setJourneyTransitionForm,
    onSaveJourneyState,
    onDeleteJourneyState,
    onSaveJourneyTransition,
    onDeleteJourneyTransition,
}: JourneySectionProps) {
    if (!selectedJourneyId) return null;

    const selectedJourneyName = details?.journeys.find((item) => item.journey_id === selectedJourneyId)?.name || selectedJourneyId;

    return (
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '16px' }}>
            <Card title="Journey States" subtitle={`当前 Journey: ${selectedJourneyName}`}>
                <div className="form-group">
                    <label className="form-label">Name</label>
                    <input className="form-input" value={journeyStateForm.name} onChange={(event) => setJourneyStateForm((prev) => ({ ...prev, name: event.target.value }))} />
                </div>
                <div className="form-group">
                    <label className="form-label">Description</label>
                    <textarea className="form-textarea" value={journeyStateForm.description} onChange={(event) => setJourneyStateForm((prev) => ({ ...prev, description: event.target.value }))} />
                </div>
                <div className="form-group">
                    <label className="form-label">Required Fields (comma separated)</label>
                    <input className="form-input" value={journeyStateForm.required_fields} onChange={(event) => setJourneyStateForm((prev) => ({ ...prev, required_fields: event.target.value }))} />
                </div>
                <div className="form-group">
                    <label className="form-label">Guideline Actions (one per line)</label>
                    <textarea className="form-textarea" value={journeyStateForm.guideline_actions} onChange={(event) => setJourneyStateForm((prev) => ({ ...prev, guideline_actions: event.target.value }))} />
                </div>
                <div style={{ display: 'flex', gap: '8px', marginBottom: '16px' }}>
                    <button className="btn btn-primary" disabled={!journeyStateForm.name.trim() || busy === 'journey-state'} onClick={onSaveJourneyState}>
                        {busy === 'journey-state' ? 'Saving...' : journeyStateForm.id ? 'Update' : 'Create'}
                    </button>
                    {journeyStateForm.id && (
                        <button className="btn btn-secondary" onClick={() => setJourneyStateForm({ id: '', name: '', description: '', required_fields: '', guideline_actions: '' })}>
                            Cancel
                        </button>
                    )}
                </div>
                <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                    {journeyStates.filter((item) => matchesSearch(item, searchText)).map((state) => (
                        <div key={state.state_id} style={{ border: '1px solid var(--border-subtle)', borderRadius: '10px', padding: '10px 12px' }}>
                            <div style={{ display: 'flex', justifyContent: 'space-between', gap: '8px' }}>
                                <div style={{ minWidth: 0 }}>
                                    <div style={{ fontSize: '13px', fontWeight: 600 }}>{state.name}</div>
                                    {state.description && <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{state.description}</div>}
                                    <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>required: {(state.required_fields || []).join(', ') || 'none'}</div>
                                </div>
                                <div style={{ display: 'flex', gap: '8px' }}>
                                    <button
                                        className="btn btn-secondary"
                                        onClick={() => setJourneyStateForm({
                                            id: state.state_id,
                                            name: state.name,
                                            description: state.description || '',
                                            required_fields: (state.required_fields || []).join(', '),
                                            guideline_actions: (state.guideline_actions || []).join('\n'),
                                        })}
                                    >
                                        Edit
                                    </button>
                                    <button className="btn btn-danger" disabled={busy === `journey-state-${state.state_id}`} onClick={() => onDeleteJourneyState(state)}>Delete</button>
                                </div>
                            </div>
                        </div>
                    ))}
                </div>
            </Card>

            <Card title="Journey Transitions">
                <div className="form-group">
                    <label className="form-label">From State</label>
                    <select className="form-input" value={journeyTransitionForm.from_state_id} onChange={(event) => setJourneyTransitionForm((prev) => ({ ...prev, from_state_id: event.target.value }))}>
                        <option value="">Select state</option>
                        {journeyStates.map((state) => <option key={state.state_id} value={state.state_id}>{state.name}</option>)}
                    </select>
                </div>
                <div className="form-group">
                    <label className="form-label">To State</label>
                    <select className="form-input" value={journeyTransitionForm.to_state_id} onChange={(event) => setJourneyTransitionForm((prev) => ({ ...prev, to_state_id: event.target.value }))}>
                        <option value="">Select state</option>
                        {journeyStates.map((state) => <option key={state.state_id} value={state.state_id}>{state.name}</option>)}
                    </select>
                </div>
                <div className="form-group">
                    <label className="form-label">Transition Type</label>
                    <select className="form-input" value={journeyTransitionForm.transition_type} onChange={(event) => setJourneyTransitionForm((prev) => ({ ...prev, transition_type: event.target.value }))}>
                        <option value="auto">auto</option>
                        <option value="manual">manual</option>
                        <option value="observation">observation</option>
                        <option value="handoff">handoff</option>
                        <option value="complete">complete</option>
                    </select>
                </div>
                <div className="form-group">
                    <label className="form-label">Condition Config JSON</label>
                    <textarea className="form-textarea" value={journeyTransitionForm.condition_config} onChange={(event) => setJourneyTransitionForm((prev) => ({ ...prev, condition_config: event.target.value }))} />
                </div>
                <div style={{ display: 'flex', gap: '8px', marginBottom: '16px' }}>
                    <button
                        className="btn btn-primary"
                        disabled={!journeyTransitionForm.from_state_id || !journeyTransitionForm.to_state_id || busy === 'journey-transition'}
                        onClick={onSaveJourneyTransition}
                    >
                        {busy === 'journey-transition' ? 'Saving...' : journeyTransitionForm.id ? 'Update' : 'Create'}
                    </button>
                    {journeyTransitionForm.id && (
                        <button className="btn btn-secondary" onClick={() => setJourneyTransitionForm({ id: '', from_state_id: '', to_state_id: '', transition_type: 'auto', condition_config: '{}' })}>
                            Cancel
                        </button>
                    )}
                </div>
                <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                    {journeyTransitions.filter((item) => matchesSearch(item, searchText)).map((transition) => (
                        <div key={transition.transition_id} style={{ border: '1px solid var(--border-subtle)', borderRadius: '10px', padding: '10px 12px' }}>
                            <div style={{ display: 'flex', justifyContent: 'space-between', gap: '8px' }}>
                                <div style={{ minWidth: 0 }}>
                                    <div style={{ fontSize: '13px', fontWeight: 600 }}>{transition.from_state_id} → {transition.to_state_id}</div>
                                    <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{transition.transition_type}</div>
                                    <pre style={{ marginTop: '8px', fontSize: '11px', color: 'var(--text-secondary)', whiteSpace: 'pre-wrap' }}>{pretty(transition.condition_config || {})}</pre>
                                </div>
                                <div style={{ display: 'flex', gap: '8px' }}>
                                    <button
                                        className="btn btn-secondary"
                                        onClick={() => setJourneyTransitionForm({
                                            id: transition.transition_id,
                                            from_state_id: transition.from_state_id,
                                            to_state_id: transition.to_state_id,
                                            transition_type: transition.transition_type,
                                            condition_config: pretty(transition.condition_config || {}),
                                        })}
                                    >
                                        Edit
                                    </button>
                                    <button className="btn btn-danger" disabled={busy === `journey-transition-${transition.transition_id}`} onClick={() => onDeleteJourneyTransition(transition)}>Delete</button>
                                </div>
                            </div>
                        </div>
                    ))}
                </div>
            </Card>
        </div>
    );
}
