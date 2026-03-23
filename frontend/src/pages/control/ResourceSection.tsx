import type { Field, ResourceConfig, ScopeDetails } from './types';
import Card from './Card';
import { matchesSearch } from './utils';

type ResourceSectionProps = {
    resource: ResourceConfig;
    details?: ScopeDetails;
    forms: Record<string, any>;
    searchText: string;
    selectedScope: string;
    busy: string | null;
    collapsed: Record<string, boolean>;
    onToggleCollapse: (key: string) => void;
    onUpdateForm: (key: string, patch: Record<string, any>) => void;
    onSaveResource: (resource: ResourceConfig) => void;
    onResetForm: (key: string) => void;
    onDeleteResource: (resource: ResourceConfig, item: any) => void;
    onSelectJourney: (id: string) => void;
    onSelectVariable: (id: string) => void;
    stacked?: boolean;
};

function renderField(resource: ResourceConfig, form: Record<string, any>, field: Field, onUpdateForm: (key: string, patch: Record<string, any>) => void) {
    const value = form[field.key];

    if (field.kind === 'checkbox') {
        return (
            <label key={field.key} style={{ display: 'flex', gap: '8px', marginBottom: '12px', fontSize: '13px' }}>
                <input type="checkbox" checked={!!value} onChange={(event) => onUpdateForm(resource.key, { [field.key]: event.target.checked })} />
                {field.label}
            </label>
        );
    }

    if (field.kind === 'select') {
        return (
            <div key={field.key} className="form-group">
                <label className="form-label">{field.label}</label>
                <select className="form-input" value={value} onChange={(event) => onUpdateForm(resource.key, { [field.key]: event.target.value })}>
                    {field.options?.map((option) => <option key={option} value={option}>{option}</option>)}
                </select>
            </div>
        );
    }

    if (field.kind === 'textarea') {
        return (
            <div key={field.key} className="form-group">
                <label className="form-label">{field.label}</label>
                <textarea
                    className="form-textarea"
                    value={value}
                    onChange={(event) => onUpdateForm(resource.key, { [field.key]: event.target.value })}
                    placeholder={field.placeholder}
                />
            </div>
        );
    }

    return (
        <div key={field.key} className="form-group">
            <label className="form-label">{field.label}</label>
            <input
                className="form-input"
                type={field.kind === 'number' ? 'number' : 'text'}
                value={value}
                onChange={(event) => onUpdateForm(resource.key, { [field.key]: field.kind === 'number' ? Number(event.target.value) : event.target.value })}
                placeholder={field.placeholder}
            />
        </div>
    );
}

export default function ResourceSection({
    resource,
    details,
    forms,
    searchText,
    selectedScope,
    busy,
    collapsed,
    onToggleCollapse,
    onUpdateForm,
    onSaveResource,
    onResetForm,
    onDeleteResource,
    onSelectJourney,
    onSelectVariable,
    stacked = false,
}: ResourceSectionProps) {
    const allItems = (details?.[resource.listKey] as any[]) || [];
    const items = allItems.filter((item) => matchesSearch(item, searchText));
    const form = forms[resource.key] ?? resource.empty;
    const singularTitle = resource.title.endsWith('s') ? resource.title.slice(0, -1) : resource.title;
    const formCollapsed = !!collapsed[`${resource.key}-form`];
    const listCollapsed = !!collapsed[`${resource.key}-list`];

    return (
        <div style={{ display: 'grid', gridTemplateColumns: stacked ? '1fr' : '360px 1fr', gap: '16px' }}>
            <Card
                title={form.id ? `Edit ${singularTitle}` : `New ${singularTitle}`}
                actions={<button className="btn btn-ghost" style={{ padding: '4px 8px' }} onClick={() => onToggleCollapse(`${resource.key}-form`)}>{formCollapsed ? 'Expand' : 'Collapse'}</button>}
            >
                {!formCollapsed && (
                    <>
                        {resource.fields.map((field) => renderField(resource, form, field, onUpdateForm))}
                        <div style={{ display: 'flex', gap: '8px' }}>
                            <button
                                className="btn btn-primary"
                                disabled={!selectedScope || !String(form.name || '').trim() || busy === resource.key}
                                onClick={() => onSaveResource(resource)}
                            >
                                {busy === resource.key ? 'Saving...' : form.id ? 'Update' : 'Create'}
                            </button>
                            {form.id && <button className="btn btn-secondary" onClick={() => onResetForm(resource.key)}>Cancel</button>}
                        </div>
                    </>
                )}
            </Card>
            <Card
                title={resource.title}
                subtitle={resource.description}
                actions={<button className="btn btn-ghost" style={{ padding: '4px 8px' }} onClick={() => onToggleCollapse(`${resource.key}-list`)}>{listCollapsed ? 'Expand' : 'Collapse'}</button>}
            >
                {!listCollapsed && (
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                        {items.map((item) => (
                            <div key={item[resource.idField]} style={{ border: '1px solid var(--border-subtle)', borderRadius: '10px', padding: '12px 14px' }}>
                                <div style={{ display: 'flex', justifyContent: 'space-between', gap: '12px' }}>
                                    <div style={{ minWidth: 0 }}>
                                        <div style={{ fontSize: '14px', fontWeight: 600 }}>{item.name || item[resource.idField]}</div>
                                        {resource.summary(item)}
                                    </div>
                                    <div style={{ display: 'flex', gap: '8px', flexShrink: 0 }}>
                                        {(resource.key === 'journeys' || resource.key === 'variables') && (
                                            <button
                                                className="btn btn-secondary"
                                                onClick={() => resource.key === 'journeys' ? onSelectJourney(item.journey_id) : onSelectVariable(item.variable_id)}
                                            >
                                                Select
                                            </button>
                                        )}
                                        <button className="btn btn-secondary" onClick={() => onUpdateForm(resource.key, resource.decode(item))}>Edit</button>
                                        <button className="btn btn-danger" disabled={busy === `${resource.key}-${item[resource.idField]}`} onClick={() => onDeleteResource(resource, item)}>Delete</button>
                                    </div>
                                </div>
                            </div>
                        ))}
                        {!items.length && <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>{allItems.length ? 'No matches.' : `No ${resource.title.toLowerCase()} yet.`}</div>}
                    </div>
                )}
            </Card>
        </div>
    );
}
