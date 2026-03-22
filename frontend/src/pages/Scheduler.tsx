import { useMemo } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import { agentApi, scheduleApi } from '../services/api';

const timeAgo = (value?: string) => {
    if (!value) return '-';
    const diff = Math.floor((Date.now() - new Date(value).getTime()) / 1000);
    if (diff < 60) return `${diff}s ago`;
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
};

export default function Scheduler() {
    const navigate = useNavigate();
    const queryClient = useQueryClient();
    const tenantId = localStorage.getItem('current_tenant_id') || '';

    const { data, isLoading, error } = useQuery({
        queryKey: ['scheduler-console', tenantId],
        queryFn: async () => {
            const agents = await agentApi.list(tenantId || undefined);
            const results = await Promise.all(
                agents.map(async (agent: any) => ({
                    agent,
                    schedules: await scheduleApi.list(agent.id).catch(() => []),
                })),
            );
            return results.flatMap(({ agent, schedules }) =>
                (schedules || []).map((schedule: any) => ({
                    ...schedule,
                    agent_id: agent.id,
                    agent_name: agent.name,
                })),
            );
        },
        refetchInterval: 15000,
    });

    const triggerMutation = useMutation({
        mutationFn: ({ agentId, scheduleId }: { agentId: string; scheduleId: string }) => scheduleApi.trigger(agentId, scheduleId),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: ['scheduler-console'] });
        },
    });

    const toggleMutation = useMutation({
        mutationFn: ({ agentId, scheduleId, enabled }: { agentId: string; scheduleId: string; enabled: boolean }) =>
            scheduleApi.update(agentId, scheduleId, { is_enabled: enabled }),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: ['scheduler-console'] });
        },
    });

    const scheduleList = useMemo(() => Array.isArray(data) ? data : [], [data]);

    return (
        <div>
            <div className="page-header">
                <div>
                    <h1 className="page-title">Scheduler</h1>
                    <div className="page-subtitle">补齐静态版 Scheduler 页面，统一查看所有 Agent 的定时任务并支持一键触发。</div>
                </div>
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', gap: '16px', marginBottom: '20px' }}>
                <div className="card" style={{ padding: '18px 20px' }}>
                    <div style={{ fontSize: '12px', color: 'var(--text-tertiary)' }}>Schedules</div>
                    <div style={{ fontSize: '30px', fontWeight: 700, marginTop: '8px' }}>{scheduleList.length}</div>
                    <div style={{ fontSize: '12px', color: 'var(--text-secondary)', marginTop: '6px' }}>Total configured schedules</div>
                </div>
                <div className="card" style={{ padding: '18px 20px' }}>
                    <div style={{ fontSize: '12px', color: 'var(--text-tertiary)' }}>Enabled</div>
                    <div style={{ fontSize: '30px', fontWeight: 700, marginTop: '8px' }}>{scheduleList.filter((item: any) => item.is_enabled !== false).length}</div>
                    <div style={{ fontSize: '12px', color: 'var(--text-secondary)', marginTop: '6px' }}>Actively polling</div>
                </div>
                <div className="card" style={{ padding: '18px 20px' }}>
                    <div style={{ fontSize: '12px', color: 'var(--text-tertiary)' }}>Needs Attention</div>
                    <div style={{ fontSize: '30px', fontWeight: 700, marginTop: '8px', color: 'var(--warning)' }}>{scheduleList.filter((item: any) => item.is_enabled === false).length}</div>
                    <div style={{ fontSize: '12px', color: 'var(--text-secondary)', marginTop: '6px' }}>Currently paused</div>
                </div>
            </div>

            <div className="card" style={{ padding: '0', overflow: 'hidden' }}>
                <div style={{ display: 'grid', gridTemplateColumns: '1.2fr 1fr 0.8fr 0.8fr 1fr', gap: '12px', padding: '12px 16px', borderBottom: '1px solid var(--border-subtle)', fontSize: '11px', textTransform: 'uppercase', color: 'var(--text-tertiary)', letterSpacing: '0.05em' }}>
                    <span>Schedule</span>
                    <span>Agent</span>
                    <span>Cron</span>
                    <span>Last Run</span>
                    <span style={{ textAlign: 'right' }}>Actions</span>
                </div>
                {isLoading && <div style={{ padding: '24px 16px', color: 'var(--text-tertiary)' }}>加载调度任务中...</div>}
                {!isLoading && error && <div style={{ padding: '24px 16px', color: 'var(--error)' }}>{error instanceof Error ? error.message : 'Failed to load schedules'}</div>}
                {!isLoading && !error && scheduleList.length === 0 && (
                    <div style={{ padding: '32px 16px', textAlign: 'center', color: 'var(--text-tertiary)' }}>还没有任何定时任务。</div>
                )}
                {!isLoading && !error && scheduleList.map((schedule: any) => (
                    <div key={schedule.id} style={{ display: 'grid', gridTemplateColumns: '1.2fr 1fr 0.8fr 0.8fr 1fr', gap: '12px', alignItems: 'center', padding: '16px', borderBottom: '1px solid var(--border-subtle)' }}>
                        <div style={{ minWidth: 0 }}>
                            <div style={{ fontSize: '13px', fontWeight: 600 }}>{schedule.name}</div>
                            <div style={{ fontSize: '12px', color: 'var(--text-secondary)', marginTop: '6px', lineHeight: 1.6 }}>
                                {schedule.instruction || 'No instruction'}
                            </div>
                        </div>
                        <div style={{ minWidth: 0 }}>
                            <div style={{ fontSize: '13px', fontWeight: 500 }}>{schedule.agent_name}</div>
                            <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '6px' }}>{schedule.agent_id}</div>
                        </div>
                        <div style={{ fontSize: '12px', fontFamily: 'var(--font-mono)', color: 'var(--text-secondary)' }}>{schedule.cron_expr || '-'}</div>
                        <div style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>{timeAgo(schedule.last_run_at || schedule.updated_at)}</div>
                        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '8px' }}>
                            <button
                                className="btn btn-ghost"
                                onClick={() => toggleMutation.mutate({ agentId: schedule.agent_id, scheduleId: schedule.id, enabled: schedule.is_enabled === false })}
                                disabled={toggleMutation.isPending}
                            >
                                {schedule.is_enabled === false ? 'Enable' : 'Pause'}
                            </button>
                            <button
                                className="btn btn-secondary"
                                onClick={() => triggerMutation.mutate({ agentId: schedule.agent_id, scheduleId: schedule.id })}
                                disabled={triggerMutation.isPending}
                            >
                                Run Now
                            </button>
                            <button className="btn btn-ghost" onClick={() => navigate(`/agents/${schedule.agent_id}#settings`)}>Agent</button>
                        </div>
                    </div>
                ))}
            </div>
        </div>
    );
}
