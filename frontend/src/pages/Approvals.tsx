import { useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { request } from '../services/api';

const timeAgo = (value?: string) => {
    if (!value) return '-';
    const diff = Math.floor((Date.now() - new Date(value).getTime()) / 1000);
    if (diff < 60) return `${diff}s ago`;
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
};

export default function Approvals() {
    const queryClient = useQueryClient();
    const [filter, setFilter] = useState<'all' | 'pending' | 'approved' | 'rejected'>('all');

    const { data, isLoading, error } = useQuery({
        queryKey: ['approvals-console'],
        queryFn: async () => {
            const response = await request<any>('/approvals');
            return Array.isArray(response?.approvals) ? response.approvals : [];
        },
        refetchInterval: 5000,
    });

    const resolveMutation = useMutation({
        mutationFn: async ({ id, action }: { id: string; action: 'approve' | 'reject' }) => {
            return request(`/approvals/${id}/${action}`, { method: 'POST' });
        },
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: ['approvals-console'] });
        },
    });

    const approvals = useMemo(() => {
        const items = data || [];
        if (filter === 'all') return items;
        return items.filter((approval: any) => approval.status === filter);
    }, [data, filter]);

    const pendingCount = (data || []).filter((approval: any) => approval.status === 'pending').length;

    return (
        <div>
            <div className="page-header">
                <div>
                    <h1 className="page-title">Approvals</h1>
                    <div className="page-subtitle">对应静态版全局审批队列，集中处理高风险工具调用和人工确认项。</div>
                </div>
                <div style={{ display: 'flex', gap: '8px' }}>
                    {(['all', 'pending', 'approved', 'rejected'] as const).map((value) => (
                        <button
                            key={value}
                            className={filter === value ? 'btn btn-primary' : 'btn btn-secondary'}
                            onClick={() => setFilter(value)}
                        >
                            {value === 'all' ? 'All' : value}
                        </button>
                    ))}
                </div>
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: '16px', marginBottom: '20px' }}>
                <div className="card" style={{ padding: '18px 20px' }}>
                    <div style={{ fontSize: '12px', color: 'var(--text-tertiary)' }}>Pending</div>
                    <div style={{ fontSize: '30px', fontWeight: 700, marginTop: '8px', color: pendingCount > 0 ? 'var(--warning)' : 'var(--text-primary)' }}>{pendingCount}</div>
                    <div style={{ fontSize: '12px', color: 'var(--text-secondary)', marginTop: '6px' }}>Need manual action</div>
                </div>
                <div className="card" style={{ padding: '18px 20px' }}>
                    <div style={{ fontSize: '12px', color: 'var(--text-tertiary)' }}>Resolved</div>
                    <div style={{ fontSize: '30px', fontWeight: 700, marginTop: '8px' }}>{(data || []).filter((item: any) => item.status !== 'pending').length}</div>
                    <div style={{ fontSize: '12px', color: 'var(--text-secondary)', marginTop: '6px' }}>Approved or rejected</div>
                </div>
                <div className="card" style={{ padding: '18px 20px' }}>
                    <div style={{ fontSize: '12px', color: 'var(--text-tertiary)' }}>Auto Refresh</div>
                    <div style={{ fontSize: '30px', fontWeight: 700, marginTop: '8px' }}>5s</div>
                    <div style={{ fontSize: '12px', color: 'var(--text-secondary)', marginTop: '6px' }}>Keeps queue current</div>
                </div>
            </div>

            <div className="card" style={{ padding: '0', overflow: 'hidden' }}>
                <div style={{ display: 'grid', gridTemplateColumns: '1.2fr 1fr 0.9fr 0.8fr 1fr', gap: '12px', padding: '12px 16px', borderBottom: '1px solid var(--border-subtle)', fontSize: '11px', textTransform: 'uppercase', color: 'var(--text-tertiary)', letterSpacing: '0.05em' }}>
                    <span>Request</span>
                    <span>Agent / Tool</span>
                    <span>Status</span>
                    <span>Created</span>
                    <span style={{ textAlign: 'right' }}>Actions</span>
                </div>
                {isLoading && <div style={{ padding: '24px 16px', color: 'var(--text-tertiary)' }}>加载审批中...</div>}
                {!isLoading && error && <div style={{ padding: '24px 16px', color: 'var(--error)' }}>{error instanceof Error ? error.message : 'Failed to load approvals'}</div>}
                {!isLoading && !error && approvals.length === 0 && (
                    <div style={{ padding: '32px 16px', textAlign: 'center', color: 'var(--text-tertiary)' }}>当前筛选条件下没有审批项。</div>
                )}
                {!isLoading && !error && approvals.map((approval: any) => (
                    <div key={approval.id} style={{ display: 'grid', gridTemplateColumns: '1.2fr 1fr 0.9fr 0.8fr 1fr', gap: '12px', alignItems: 'start', padding: '16px', borderBottom: '1px solid var(--border-subtle)' }}>
                        <div style={{ minWidth: 0 }}>
                            <div style={{ fontSize: '13px', fontWeight: 600 }}>{approval.title || approval.reason || approval.id}</div>
                            <div style={{ fontSize: '12px', color: 'var(--text-secondary)', marginTop: '6px', lineHeight: 1.6 }}>
                                {approval.description || approval.message || approval.prompt || 'No description'}
                            </div>
                        </div>
                        <div style={{ minWidth: 0 }}>
                            <div style={{ fontSize: '13px', fontWeight: 500 }}>{approval.agent_name || approval.agent_id || 'Unknown agent'}</div>
                            <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '6px' }}>{approval.tool_name || approval.tool || approval.action || '-'}</div>
                        </div>
                        <div>
                            <span className={`badge ${approval.status === 'approved' ? 'badge-success' : approval.status === 'rejected' ? 'badge-error' : 'badge-warning'}`}>
                                {approval.status || 'pending'}
                            </span>
                        </div>
                        <div style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>{timeAgo(approval.created_at)}</div>
                        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '8px' }}>
                            {approval.status === 'pending' ? (
                                <>
                                    <button
                                        className="btn btn-primary"
                                        disabled={resolveMutation.isPending}
                                        onClick={() => resolveMutation.mutate({ id: approval.id, action: 'approve' })}
                                    >
                                        Approve
                                    </button>
                                    <button
                                        className="btn btn-secondary"
                                        disabled={resolveMutation.isPending}
                                        onClick={() => {
                                            if (window.confirm('确认拒绝这个审批请求吗？')) {
                                                resolveMutation.mutate({ id: approval.id, action: 'reject' });
                                            }
                                        }}
                                    >
                                        Reject
                                    </button>
                                </>
                            ) : (
                                <span style={{ fontSize: '12px', color: 'var(--text-tertiary)' }}>
                                    {approval.resolved_at ? new Date(approval.resolved_at).toLocaleString() : 'Resolved'}
                                </span>
                            )}
                        </div>
                    </div>
                ))}
            </div>
        </div>
    );
}
