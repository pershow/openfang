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

export default function Comms() {
    const queryClient = useQueryClient();
    const [sendForm, setSendForm] = useState({ from_agent_id: '', to_agent_id: '', message: '' });
    const [taskForm, setTaskForm] = useState({ title: '', description: '', assigned_to: '' });

    const { data, isLoading, error } = useQuery({
        queryKey: ['comms-console'],
        queryFn: async () => {
            const [topology, events] = await Promise.all([
                request<any>('/comms/topology').catch(() => ({ nodes: [], edges: [] })),
                request<any[]>('/comms/events?limit=100').catch(() => []),
            ]);
            return {
                topology: {
                    nodes: Array.isArray(topology?.nodes) ? topology.nodes : [],
                    edges: Array.isArray(topology?.edges) ? topology.edges : [],
                },
                events: Array.isArray(events) ? events : [],
            };
        },
        refetchInterval: 10000,
    });

    const sendMutation = useMutation({
        mutationFn: () => request('/comms/send', { method: 'POST', body: JSON.stringify(sendForm) }),
        onSuccess: () => {
            setSendForm({ from_agent_id: '', to_agent_id: '', message: '' });
            queryClient.invalidateQueries({ queryKey: ['comms-console'] });
        },
    });

    const taskMutation = useMutation({
        mutationFn: () => request('/comms/task', { method: 'POST', body: JSON.stringify(taskForm) }),
        onSuccess: () => {
            setTaskForm({ title: '', description: '', assigned_to: '' });
            queryClient.invalidateQueries({ queryKey: ['comms-console'] });
        },
    });

    const nodes = data?.topology?.nodes || [];
    const edges = data?.topology?.edges || [];

    const peerMap = useMemo(() => {
        const map = new Map<string, string[]>();
        edges.forEach((edge: any) => {
            const from = edge.from || edge.source;
            const to = edge.to || edge.target;
            if (!from || !to) return;
            if (!map.has(from)) map.set(from, []);
            map.get(from)!.push(to);
        });
        return map;
    }, [edges]);

    return (
        <div>
            <div className="page-header">
                <div>
                    <h1 className="page-title">Comms</h1>
                    <div className="page-subtitle">补齐静态版 Comms 页面，查看 Agent 拓扑、互相通信的事件流，以及手动发消息/派任务。</div>
                </div>
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: '1.1fr 0.9fr', gap: '16px', alignItems: 'start' }}>
                <div className="card" style={{ padding: '20px' }}>
                    <div style={{ fontSize: '15px', fontWeight: 600, marginBottom: '14px' }}>Agent Topology</div>
                    {isLoading && <div style={{ color: 'var(--text-tertiary)' }}>加载拓扑中...</div>}
                    {!isLoading && error && <div style={{ color: 'var(--error)' }}>{error instanceof Error ? error.message : 'Failed to load topology'}</div>}
                    {!isLoading && !error && nodes.length === 0 && (
                        <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>No communication nodes found.</div>
                    )}
                    {!isLoading && !error && (
                        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))', gap: '12px' }}>
                            {nodes.map((node: any) => (
                                <div key={node.id} style={{ border: '1px solid var(--border-subtle)', borderRadius: '12px', padding: '14px', background: 'var(--bg-secondary)' }}>
                                    <div style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', marginBottom: '8px' }}>
                                        <div style={{ fontSize: '13px', fontWeight: 600 }}>{node.name || node.id}</div>
                                        <span className={`badge ${node.state === 'Running' || node.status === 'connected' ? 'badge-success' : 'badge-warning'}`}>
                                            {node.state || node.status || 'unknown'}
                                        </span>
                                    </div>
                                    <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginBottom: '10px' }}>{node.id}</div>
                                    <div style={{ fontSize: '12px', color: 'var(--text-secondary)', lineHeight: 1.6 }}>
                                        {(peerMap.get(node.id) || []).slice(0, 4).map((peerId) => (
                                            <div key={peerId}>→ {peerId}</div>
                                        ))}
                                        {(peerMap.get(node.id) || []).length === 0 && <div>No outgoing edges</div>}
                                    </div>
                                </div>
                            ))}
                        </div>
                    )}
                </div>

                <div style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
                    <div className="card" style={{ padding: '20px' }}>
                        <div style={{ fontSize: '15px', fontWeight: 600, marginBottom: '12px' }}>Send Agent Message</div>
                        <div style={{ display: 'grid', gap: '10px' }}>
                            <input className="form-input" placeholder="From agent id" value={sendForm.from_agent_id} onChange={(event) => setSendForm({ ...sendForm, from_agent_id: event.target.value })} />
                            <input className="form-input" placeholder="To agent id" value={sendForm.to_agent_id} onChange={(event) => setSendForm({ ...sendForm, to_agent_id: event.target.value })} />
                            <textarea className="form-textarea" placeholder="Message" value={sendForm.message} onChange={(event) => setSendForm({ ...sendForm, message: event.target.value })} />
                            <button className="btn btn-primary" onClick={() => sendMutation.mutate()} disabled={sendMutation.isPending || !sendForm.from_agent_id || !sendForm.to_agent_id || !sendForm.message}>
                                {sendMutation.isPending ? 'Sending...' : 'Send'}
                            </button>
                        </div>
                    </div>

                    <div className="card" style={{ padding: '20px' }}>
                        <div style={{ fontSize: '15px', fontWeight: 600, marginBottom: '12px' }}>Post Shared Task</div>
                        <div style={{ display: 'grid', gap: '10px' }}>
                            <input className="form-input" placeholder="Task title" value={taskForm.title} onChange={(event) => setTaskForm({ ...taskForm, title: event.target.value })} />
                            <input className="form-input" placeholder="Assigned agent id (optional)" value={taskForm.assigned_to} onChange={(event) => setTaskForm({ ...taskForm, assigned_to: event.target.value })} />
                            <textarea className="form-textarea" placeholder="Task description" value={taskForm.description} onChange={(event) => setTaskForm({ ...taskForm, description: event.target.value })} />
                            <button className="btn btn-secondary" onClick={() => taskMutation.mutate()} disabled={taskMutation.isPending || !taskForm.title}>
                                {taskMutation.isPending ? 'Posting...' : 'Post Task'}
                            </button>
                        </div>
                    </div>
                </div>
            </div>

            <div className="card" style={{ padding: '20px', marginTop: '16px' }}>
                <div style={{ fontSize: '15px', fontWeight: 600, marginBottom: '14px' }}>Recent Events</div>
                {isLoading && <div style={{ color: 'var(--text-tertiary)' }}>加载事件流中...</div>}
                {!isLoading && !error && (data?.events || []).length === 0 && (
                    <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>No communication events yet.</div>
                )}
                {!isLoading && !error && (
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                        {(data?.events || []).map((event: any, index: number) => (
                            <div key={event.id || `${event.kind}-${index}`} style={{ borderBottom: '1px solid var(--border-subtle)', paddingBottom: '10px' }}>
                                <div style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', marginBottom: '6px' }}>
                                    <span className={`badge ${event.kind?.includes('terminated') ? 'badge-error' : event.kind?.includes('spawned') ? 'badge-success' : 'badge-info'}`}>{event.kind || 'event'}</span>
                                    <span style={{ fontSize: '11px', color: 'var(--text-tertiary)' }}>{timeAgo(event.created_at || event.timestamp)}</span>
                                </div>
                                <div style={{ fontSize: '12px', color: 'var(--text-secondary)', lineHeight: 1.6 }}>
                                    {event.message || event.summary || JSON.stringify(event)}
                                </div>
                            </div>
                        ))}
                    </div>
                )}
            </div>
        </div>
    );
}
