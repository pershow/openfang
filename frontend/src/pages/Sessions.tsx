import { useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import { agentApi, request } from '../services/api';

const timeAgo = (value?: string) => {
    if (!value) return '-';
    const diff = Math.floor((Date.now() - new Date(value).getTime()) / 1000);
    if (diff < 60) return `${diff}s ago`;
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
};

function ReplayModal({ data, loading, onClose }: { data: any; loading: boolean; onClose: () => void }) {
    return (
        <div style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.55)', zIndex: 1200, display: 'flex', alignItems: 'center', justifyContent: 'center', padding: '24px' }} onClick={onClose}>
            <div className="card" style={{ width: 'min(960px, 100%)', maxHeight: '88vh', overflow: 'auto', padding: '20px', boxShadow: 'var(--shadow-lg)' }} onClick={(event) => event.stopPropagation()}>
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
                    <div>
                        <div style={{ fontSize: '18px', fontWeight: 700 }}>Session Replay</div>
                        <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginTop: '4px' }}>查看完整对话、控制轨迹和消息上下文。</div>
                    </div>
                    <button className="btn btn-ghost" onClick={onClose}>关闭</button>
                </div>
                {loading && <div style={{ color: 'var(--text-tertiary)' }}>加载回放数据中...</div>}
                {!loading && !data && <div style={{ color: 'var(--text-tertiary)' }}>暂无回放内容。</div>}
                {!loading && data && (
                    <div style={{ display: 'grid', gridTemplateColumns: '1.2fr 1fr', gap: '16px' }}>
                        <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
                            <div style={{ fontSize: '14px', fontWeight: 600 }}>Transcript</div>
                            {(data.messages || data.transcript || []).map((message: any, index: number) => (
                                <div key={message.id || index} style={{ border: '1px solid var(--border-subtle)', borderRadius: '10px', padding: '12px 14px', background: 'var(--bg-secondary)' }}>
                                    <div style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', marginBottom: '8px' }}>
                                        <span className={`badge ${message.role === 'assistant' ? 'badge-info' : 'badge-success'}`}>{message.role || 'message'}</span>
                                        <span style={{ fontSize: '11px', color: 'var(--text-tertiary)' }}>{message.created_at ? new Date(message.created_at).toLocaleString() : ''}</span>
                                    </div>
                                    <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.7, fontSize: '13px' }}>{message.content || message.text || JSON.stringify(message, null, 2)}</div>
                                </div>
                            ))}
                        </div>
                        <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
                            <div style={{ fontSize: '14px', fontWeight: 600 }}>Control Trace</div>
                            <div style={{ border: '1px solid var(--border-subtle)', borderRadius: '10px', padding: '12px 14px', background: 'var(--bg-secondary)' }}>
                                <pre style={{ margin: 0, whiteSpace: 'pre-wrap', wordBreak: 'break-word', fontFamily: 'var(--font-mono)', fontSize: '12px', color: 'var(--text-secondary)' }}>
                                    {JSON.stringify(data.trace || data.control_trace || data.enriched_trace || {}, null, 2)}
                                </pre>
                            </div>
                        </div>
                    </div>
                )}
            </div>
        </div>
    );
}

export default function Sessions() {
    const navigate = useNavigate();
    const queryClient = useQueryClient();
    const tenantId = localStorage.getItem('current_tenant_id') || '';
    const [search, setSearch] = useState('');
    const [selectedSessionId, setSelectedSessionId] = useState<string | null>(null);

    const { data, isLoading, error } = useQuery({
        queryKey: ['sessions-console', tenantId],
        queryFn: async () => {
            const [sessionRes, agents] = await Promise.all([
                request<any>('/sessions').catch(() => ({ sessions: [] })),
                agentApi.list(tenantId || undefined).catch(() => []),
            ]);
            const sessionList = Array.isArray(sessionRes?.sessions) ? sessionRes.sessions : [];
            const agentMap = new Map((agents || []).map((agent: any) => [agent.id, agent]));
            return sessionList.map((session: any) => ({
                ...session,
                agent_name: session.agent_name || agentMap.get(session.agent_id)?.name || session.agent_id,
            }));
        },
        refetchInterval: 15000,
    });

    const replayQuery = useQuery({
        queryKey: ['session-replay', selectedSessionId],
        queryFn: () => request<any>(`/sessions/${selectedSessionId}/replay`),
        enabled: !!selectedSessionId,
    });

    const deleteMutation = useMutation({
        mutationFn: (sessionId: string) => request<void>(`/sessions/${sessionId}`, { method: 'DELETE' }),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: ['sessions-console'] });
        },
    });

    const sessions = useMemo(() => {
        const items = data || [];
        const query = search.trim().toLowerCase();
        if (!query) return items;
        return items.filter((session: any) => {
            const haystack = [
                session.agent_name,
                session.agent_id,
                session.username,
                session.source_channel,
                session.session_id,
            ]
                .filter(Boolean)
                .join(' ')
                .toLowerCase();
            return haystack.includes(query);
        });
    }, [data, search]);

    return (
        <div>
            <div className="page-header">
                <div>
                    <h1 className="page-title">Sessions</h1>
                    <div className="page-subtitle">对应静态版 Sessions 页面，按 Agent、来源和时间统一查看全局会话。</div>
                </div>
                <div style={{ display: 'flex', gap: '8px' }}>
                    <input
                        value={search}
                        onChange={(event) => setSearch(event.target.value)}
                        placeholder="搜索会话 / Agent / 用户"
                        style={{ width: '240px' }}
                    />
                </div>
            </div>

            <div className="card" style={{ padding: '0', overflow: 'hidden' }}>
                <div style={{ display: 'grid', gridTemplateColumns: '2fr 1.2fr 0.8fr 0.8fr 1fr', gap: '12px', padding: '12px 16px', borderBottom: '1px solid var(--border-subtle)', fontSize: '11px', textTransform: 'uppercase', color: 'var(--text-tertiary)', letterSpacing: '0.05em' }}>
                    <span>Session</span>
                    <span>Agent</span>
                    <span>Source</span>
                    <span>Updated</span>
                    <span style={{ textAlign: 'right' }}>Actions</span>
                </div>
                {isLoading && <div style={{ padding: '24px 16px', color: 'var(--text-tertiary)' }}>加载会话列表中...</div>}
                {!isLoading && error && <div style={{ padding: '24px 16px', color: 'var(--error)' }}>{error instanceof Error ? error.message : 'Failed to load sessions'}</div>}
                {!isLoading && !error && sessions.length === 0 && (
                    <div style={{ padding: '32px 16px', color: 'var(--text-tertiary)', textAlign: 'center' }}>没有找到匹配的会话。</div>
                )}
                {!isLoading && !error && sessions.map((session: any) => (
                    <div key={session.session_id || session.id} style={{ display: 'grid', gridTemplateColumns: '2fr 1.2fr 0.8fr 0.8fr 1fr', gap: '12px', alignItems: 'center', padding: '14px 16px', borderBottom: '1px solid var(--border-subtle)' }}>
                        <div style={{ minWidth: 0 }}>
                            <div style={{ fontSize: '13px', fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                                {session.label || session.username || session.session_id || 'Session'}
                            </div>
                            <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                                {session.session_id || session.id}
                            </div>
                        </div>
                        <div style={{ minWidth: 0 }}>
                            <div style={{ fontSize: '13px', fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{session.agent_name}</div>
                            <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{session.agent_id}</div>
                        </div>
                        <div>
                            <span className={`badge ${session.source_channel === 'agent' ? 'badge-info' : 'badge-success'}`}>
                                {session.source_channel || 'web'}
                            </span>
                        </div>
                        <div style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>
                            {timeAgo(session.updated_at || session.last_message_at || session.created_at)}
                        </div>
                        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '8px' }}>
                            <button className="btn btn-ghost" onClick={() => setSelectedSessionId(session.session_id || session.id)}>Replay</button>
                            <button className="btn btn-ghost" onClick={() => navigate(`/agents/${session.agent_id}#chat`)}>Agent</button>
                            <button
                                className="btn btn-ghost"
                                style={{ color: 'var(--error)' }}
                                disabled={deleteMutation.isPending}
                                onClick={() => {
                                    if (window.confirm(`确定删除会话 ${session.session_id || session.id} 吗？`)) {
                                        deleteMutation.mutate(session.session_id || session.id);
                                    }
                                }}
                            >
                                Delete
                            </button>
                        </div>
                    </div>
                ))}
            </div>

            {selectedSessionId && (
                <ReplayModal
                    data={replayQuery.data}
                    loading={replayQuery.isLoading}
                    onClose={() => setSelectedSessionId(null)}
                />
            )}
        </div>
    );
}
