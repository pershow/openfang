import type { ScopeDetails, StateSetter, VariableValueForm } from './types';
import Card from './Card';
import { matchesSearch, pretty } from './utils';

type VariableValuesSectionProps = {
    details?: ScopeDetails;
    selectedVariableId: string;
    variableValues: any[];
    searchText: string;
    busy: string | null;
    variableValueForm: VariableValueForm;
    setVariableValueForm: StateSetter<VariableValueForm>;
    onSaveVariableValue: () => void;
    onDeleteVariableValue: (value: any) => void;
};

export default function VariableValuesSection({
    details,
    selectedVariableId,
    variableValues,
    searchText,
    busy,
    variableValueForm,
    setVariableValueForm,
    onSaveVariableValue,
    onDeleteVariableValue,
}: VariableValuesSectionProps) {
    if (!selectedVariableId) return null;

    const selectedVariableName = details?.contextVariables.find((item) => item.variable_id === selectedVariableId)?.name || selectedVariableId;

    return (
        <Card title="Context Variable Values" subtitle={`当前变量: ${selectedVariableName}`}>
            <div className="form-group">
                <label className="form-label">Key</label>
                <input className="form-input" value={variableValueForm.key} onChange={(event) => setVariableValueForm((prev) => ({ ...prev, key: event.target.value }))} />
            </div>
            <div className="form-group">
                <label className="form-label">Value Data JSON</label>
                <textarea className="form-textarea" value={variableValueForm.data} onChange={(event) => setVariableValueForm((prev) => ({ ...prev, data: event.target.value }))} />
            </div>
            <div style={{ display: 'flex', gap: '8px', marginBottom: '16px' }}>
                <button className="btn btn-primary" disabled={!variableValueForm.key.trim() || busy === 'variable-value'} onClick={onSaveVariableValue}>
                    {busy === 'variable-value' ? 'Saving...' : 'Upsert Value'}
                </button>
                <button className="btn btn-secondary" onClick={() => setVariableValueForm({ key: '', data: '{\n  "value": ""\n}' })}>Reset</button>
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                {variableValues.filter((item) => matchesSearch(item, searchText)).map((value) => (
                    <div key={value.key} style={{ border: '1px solid var(--border-subtle)', borderRadius: '10px', padding: '10px 12px' }}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', gap: '8px' }}>
                            <div style={{ minWidth: 0 }}>
                                <div style={{ fontSize: '13px', fontWeight: 600 }}>{value.key}</div>
                                <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{new Date(value.updated_at).toLocaleString()}</div>
                                <pre style={{ marginTop: '8px', fontSize: '11px', color: 'var(--text-secondary)', whiteSpace: 'pre-wrap' }}>{pretty(value.data)}</pre>
                            </div>
                            <div style={{ display: 'flex', gap: '8px' }}>
                                <button className="btn btn-secondary" onClick={() => setVariableValueForm({ key: value.key, data: pretty(value.data) })}>Load</button>
                                <button className="btn btn-danger" disabled={busy === `variable-value-${value.key}`} onClick={() => onDeleteVariableValue(value)}>Delete</button>
                            </div>
                        </div>
                    </div>
                ))}
                {!variableValues.length && <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>No stored values yet.</div>}
            </div>
        </Card>
    );
}
