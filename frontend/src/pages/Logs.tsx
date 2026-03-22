import { useMemo, useState } from 'react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { request } from '../services/api';

const classifyLevel = (action: string) => {
    const value = (action || '').toLowerCase();
    if (value.includes('error') || value.includes('fail') || value.includes('reject')) return 'error';
    if (value.includes('warn') || value.includes('deny') || value.includes('limit')) return 'warn';
    return 'info';
};

const timeAgo = (value?: string) => {
    if (!value) return '-';
    const diff = Math.floor((Date.now() - new Date(value).getTime()) / 1000);
    if (diff < 60) return `${diff}s ago`;
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
};

export default function Logs() {
    const [level, setLevel] = useState<'all' | 'info' | 'warn' | 'error'>('all');
    const [search, setSearch] = useState('');

    const { data, isLoading, error } = useQuery({
        queryKey: ['logs-console'],
        queryFn: () => request<any>('/audit/recent?n=200'),
        refetchInterval: 5000,
    });

    const verifyMutation = useMutation({
        mutationFn: () => request<any>('/audit/verify'),
    });

    const entries = useMemo(() => {
        const items = Array.isArray(data?.entries) ? data.entries : [];
        const query = search.trim().toLowerCase();
        return items.filter((entry: any) => {
            if (level !== 'all' && classifyLevel(entry.action) !== level) return false;
            if (!query) return true;
            const haystack = [entry.action, entry.summary, entry.agent_id, JSON.stringify(entry.detail || entry.details || {})].join(' ').toLowerCase();
            return haystack.includes(query);
        });
    }, [data?.entries, level, search]);

    return (
        <div>
            <div className="page-header">
                <div>
                    <h1 className="page-title">Logs</h1>
                    <div className="page-subtitle">补齐静态版 Logs 页面，先提供高频轮询的实时审计视图，并支持校验审计链。</div>
                </div>
                <div style={{ display: 'flex', gap: '8px' }}>
                    <input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="搜索 action / agent / detail" style={{ width: '240px' }} />
                    <button className="btn btn-secondary" onClick={() => verifyMutation.mutate()} disabled={verifyMutation.isPending}>
                        {verifyMutation.isPending ? 'Verifying...' : 'Verify Chain'}
                    </button>
                </div>
            </div>

            <div style={{ display: 'flex', gap: '8px', marginBottom: '16px' }}>
                {(['all', 'info', 'warn', 'error'] as const).map((item) => (
                    <button key={item} className={level === item ? 'btn btn-primary' : 'btn btn-secondary'} onClick={() => setLevel(item)}>
                        {item}
                    </button>
                ))}
                {verifyMutation.data && (
                    <span className={`badge ${verifyMutation.data.valid ? 'badge-success' : 'badge-error'}`} style={{ alignSelf: 'center' }}>
                        {verifyMutation.data.valid ? 'Audit chain valid' : 'Audit chain invalid'}
                    </span>
                )}
            </div>

            <div className="card" style={{ padding: '0', overflow: 'hidden' }}>
                <div style={{ display: 'grid', gridTemplateColumns: '0.8fr 1fr 2fr 0.9fr', gap: '12px', padding: '12px 16px', borderBottom: '1px solid var(--border-subtle)', fontSize: '11px', textTransform: 'uppercase', color: 'var(--text-tertiary)', letterSpacing: '0.05em' }}>
                    <span>Level</span>
                    <span>Action</span>
                    <span>Summary</span>
                    <span>When</span>
                </div>
                {isLoading && <div style={{ padding: '24px 16px', color: 'var(--text-tertiary)' }}>加载日志中...</div>}
                {!isLoading && error && <div style={{ padding: '24px 16px', color: 'var(--error)' }}>{error instanceof Error ? error.message : 'Failed to load logs'}</div>}
                {!isLoading && !error && entries.length === 0 && (
                    <div style={{ padding: '32px 16px', color: 'var(--text-tertiary)', textAlign: 'center' }}>没有符合筛选条件的日志。</div>
                )}
                {!isLoading && !error && entries.map((entry: any) => {
                    const entryLevel = classifyLevel(entry.action);
                    return (
                        <div key={entry.id || `${entry.created_at}-${entry.action}`} style={{ display: 'grid', gridTemplateColumns: '0.8fr 1fr 2fr 0.9fr', gap: '12px', alignItems: 'start', padding: '16px', borderBottom: '1px solid var(--border-subtle)' }}>
                            <div>
                                <span className={`badge ${entryLevel === 'error' ? 'badge-error' : entryLevel === 'warn' ? 'badge-warning' : 'badge-info'}`}>{entryLevel}</span>
                            </div>
                            <div style={{ fontSize: '13px', fontWeight: 600 }}>{entry.action || 'event'}</div>
                            <div>
                                <div style={{ fontSize: '12px', color: 'var(--text-secondary)', lineHeight: 1.7, whiteSpace: 'pre-wrap' }}>
                                    {entry.summary || JSON.stringify(entry.detail || entry.details || {})}
                                </div>
                                {entry.agent_id && <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '6px' }}>agent: {entry.agent_id}</div>}
                            </div>
                            <div style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>{timeAgo(entry.created_at || entry.timestamp)}</div>
                        </div>
                    );
                })}
            </div>
        </div>
    );
}
