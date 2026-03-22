import { useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { request } from '../services/api';

type ControlScope = {
    scope_id: string;
    name: string;
    scope_type?: string;
    status?: string;
};

export default function Control() {
    const queryClient = useQueryClient();
    const [selectedScope, setSelectedScope] = useState('');
    const [newScopeName, setNewScopeName] = useState('');

    const scopesQuery = useQuery({
        queryKey: ['control-scopes'],
        queryFn: () => request<ControlScope[]>('/control/scopes'),
        refetchInterval: 15000,
    });

    useEffect(() => {
        if (!selectedScope && scopesQuery.data && scopesQuery.data.length > 0) {
            setSelectedScope(scopesQuery.data[0].scope_id);
        }
    }, [scopesQuery.data, selectedScope]);

    const createScopeMutation = useMutation({
        mutationFn: () => request('/control/scopes', { method: 'POST', body: JSON.stringify({ name: newScopeName.trim() }) }),
        onSuccess: () => {
            setNewScopeName('');
            queryClient.invalidateQueries({ queryKey: ['control-scopes'] });
        },
    });

    const detailsQuery = useQuery({
        queryKey: ['control-scope-details', selectedScope],
        queryFn: async () => {
            const [observations, guidelines, journeys, toolPolicies, releases] = await Promise.all([
                request<any[]>(`/control/scopes/${selectedScope}/observations`).catch(() => []),
                request<any[]>(`/control/scopes/${selectedScope}/guidelines`).catch(() => []),
                request<any[]>(`/control/scopes/${selectedScope}/journeys`).catch(() => []),
                request<any[]>(`/control/scopes/${selectedScope}/tool-policies`).catch(() => []),
                request<any[]>(`/control/scopes/${selectedScope}/releases`).catch(() => []),
            ]);
            return { observations, guidelines, journeys, toolPolicies, releases };
        },
        enabled: !!selectedScope,
        refetchInterval: 15000,
    });

    const scopes = scopesQuery.data || [];
    const details = detailsQuery.data;

    return (
        <div>
            <div className="page-header">
                <div>
                    <h1 className="page-title">Control Plane</h1>
                    <div className="page-subtitle">补齐静态版 Control 页面，先提供 scope 级概览与关键资产清单，便于继续深挖策略和 Journey。</div>
                </div>
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: '300px 1fr', gap: '16px', alignItems: 'start' }}>
                <div className="card" style={{ padding: '20px' }}>
                    <div style={{ fontSize: '15px', fontWeight: 600, marginBottom: '14px' }}>Scopes</div>
                    <div style={{ display: 'grid', gap: '10px', marginBottom: '14px' }}>
                        <input className="form-input" placeholder="新 scope 名称" value={newScopeName} onChange={(event) => setNewScopeName(event.target.value)} />
                        <button className="btn btn-primary" disabled={createScopeMutation.isPending || !newScopeName.trim()} onClick={() => createScopeMutation.mutate()}>
                            {createScopeMutation.isPending ? 'Creating...' : 'Create Scope'}
                        </button>
                    </div>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
                        {scopesQuery.isLoading && <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>加载 scopes 中...</div>}
                        {!scopesQuery.isLoading && scopes.length === 0 && <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>No scopes yet.</div>}
                        {scopes.map((scope) => (
                            <button
                                key={scope.scope_id}
                                className={selectedScope === scope.scope_id ? 'btn btn-primary' : 'btn btn-secondary'}
                                style={{ justifyContent: 'space-between', width: '100%' }}
                                onClick={() => setSelectedScope(scope.scope_id)}
                            >
                                <span>{scope.name || scope.scope_id.slice(0, 8)}</span>
                                <span style={{ fontSize: '11px', opacity: 0.8 }}>{scope.status || scope.scope_type || 'scope'}</span>
                            </button>
                        ))}
                    </div>
                </div>

                <div style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
                    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(5, minmax(0, 1fr))', gap: '16px' }}>
                        {[
                            { label: 'Observations', value: details?.observations.length || 0 },
                            { label: 'Guidelines', value: details?.guidelines.length || 0 },
                            { label: 'Journeys', value: details?.journeys.length || 0 },
                            { label: 'Tool Policies', value: details?.toolPolicies.length || 0 },
                            { label: 'Releases', value: details?.releases.length || 0 },
                        ].map((item) => (
                            <div key={item.label} className="card" style={{ padding: '18px 20px' }}>
                                <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginBottom: '8px' }}>{item.label}</div>
                                <div style={{ fontSize: '28px', fontWeight: 700 }}>{item.value}</div>
                            </div>
                        ))}
                    </div>

                    <div className="card" style={{ padding: '20px' }}>
                        <div style={{ fontSize: '15px', fontWeight: 600, marginBottom: '14px' }}>Scope Summary</div>
                        {!selectedScope && <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>选择一个 scope 查看详情。</div>}
                        {selectedScope && detailsQuery.isLoading && <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>加载详情中...</div>}
                        {selectedScope && !detailsQuery.isLoading && (
                            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '16px' }}>
                                {[
                                    { title: 'Observations', items: details?.observations || [], keyField: 'name' },
                                    { title: 'Guidelines', items: details?.guidelines || [], keyField: 'name' },
                                    { title: 'Journeys', items: details?.journeys || [], keyField: 'name' },
                                    { title: 'Tool Policies', items: details?.toolPolicies || [], keyField: 'tool_name' },
                                ].map((group) => (
                                    <div key={group.title} style={{ border: '1px solid var(--border-subtle)', borderRadius: '12px', padding: '14px', background: 'var(--bg-secondary)' }}>
                                        <div style={{ fontSize: '14px', fontWeight: 600, marginBottom: '10px' }}>{group.title}</div>
                                        <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
                                            {group.items.length === 0 && <div style={{ fontSize: '12px', color: 'var(--text-tertiary)' }}>No items</div>}
                                            {group.items.slice(0, 6).map((item: any) => (
                                                <div key={item.id || item.observation_id || item.guideline_id || item.journey_id || item.policy_id} style={{ paddingBottom: '8px', borderBottom: '1px solid var(--border-subtle)' }}>
                                                    <div style={{ fontSize: '13px', fontWeight: 500 }}>{item[group.keyField] || item.name || item.tool_name || 'Unnamed'}</div>
                                                    <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>
                                                        {item.matcher_type || item.composition_mode || item.approval_mode || item.status || 'configured'}
                                                    </div>
                                                </div>
                                            ))}
                                        </div>
                                    </div>
                                ))}
                            </div>
                        )}
                    </div>
                </div>
            </div>
        </div>
    );
}
