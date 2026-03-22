import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import { agentApi, request } from '../services/api';

const formatNumber = (value: number) => {
    if (!value) return '0';
    if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
    if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
    return String(value);
};

const formatMoney = (value: number) => {
    if (!value) return '$0.00';
    if (value < 0.01) return `<$${value.toFixed(4)}`;
    return `$${value.toFixed(2)}`;
};

const timeAgo = (value?: string) => {
    if (!value) return '-';
    const diff = Math.floor((Date.now() - new Date(value).getTime()) / 1000);
    if (diff < 60) return `${diff}s ago`;
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
};

function MetricCard({ label, value, sub, tone = 'default' }: { label: string; value: string; sub: string; tone?: 'default' | 'success' | 'warning' | 'danger' }) {
    const color =
        tone === 'success' ? 'var(--success)' :
            tone === 'warning' ? 'var(--warning)' :
                tone === 'danger' ? 'var(--error)' :
                    'var(--text-primary)';

    return (
        <div className="card" style={{ padding: '18px 20px' }}>
            <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginBottom: '8px' }}>{label}</div>
            <div style={{ fontSize: '28px', fontWeight: 700, letterSpacing: '-0.03em', color }}>{value}</div>
            <div style={{ fontSize: '12px', color: 'var(--text-secondary)', marginTop: '6px' }}>{sub}</div>
        </div>
    );
}

export default function Overview() {
    const navigate = useNavigate();
    const tenantId = localStorage.getItem('current_tenant_id') || '';

    const { data, isLoading, error } = useQuery({
        queryKey: ['overview-console', tenantId],
        queryFn: async () => {
            const [
                health,
                status,
                version,
                providersRes,
                channelsRes,
                skillsRes,
                approvalsRes,
                auditRes,
                usageSummary,
                agents,
            ] = await Promise.all([
                request<any>('/health').catch(() => ({ status: 'down' })),
                request<any>('/status').catch(() => ({})),
                request<any>('/version').catch(() => ({})),
                request<any>('/providers').catch(() => ({ providers: [] })),
                request<any>('/channels').catch(() => ({ channels: [] })),
                request<any>('/skills').then((response) => Array.isArray(response?.skills) ? response.skills : []).catch(() => []),
                request<any>('/approvals').catch(() => ({ approvals: [] })),
                request<any>('/audit/recent?n=8').catch(() => ({ entries: [] })),
                request<any>('/usage/summary').catch(() => ({})),
                agentApi.list(tenantId || undefined).catch(() => []),
            ]);

            return {
                health,
                status,
                version,
                providers: Array.isArray(providersRes?.providers) ? providersRes.providers : [],
                channels: Array.isArray(channelsRes?.channels) ? channelsRes.channels : [],
                skills: Array.isArray(skillsRes) ? skillsRes : [],
                approvals: Array.isArray(approvalsRes?.approvals) ? approvalsRes.approvals : [],
                audit: Array.isArray(auditRes?.entries) ? auditRes.entries : [],
                usageSummary,
                agents,
            };
        },
        refetchInterval: 30000,
    });

    const derived = useMemo(() => {
        const providers = data?.providers || [];
        const channels = data?.channels || [];
        const approvals = data?.approvals || [];
        const agents = data?.agents || [];
        const configuredProviders = providers.filter((provider: any) => provider.auth_status === 'configured' || provider.auth_status === 'Configured');
        const readyChannels = channels.filter((channel: any) => channel.configured || channel.connected || channel.has_token);
        const pendingApprovals = approvals.filter((approval: any) => approval.status === 'pending');
        const runningAgents = agents.filter((agent: any) => ['running', 'idle'].includes(agent.status)).length;
        return {
            configuredProviders,
            readyChannels,
            pendingApprovals,
            runningAgents,
        };
    }, [data]);

    if (isLoading) {
        return <div style={{ padding: '48px 0', textAlign: 'center', color: 'var(--text-tertiary)' }}>加载总览中...</div>;
    }

    if (error || !data) {
        return (
            <div className="card" style={{ maxWidth: '760px', margin: '40px auto', textAlign: 'center', padding: '32px' }}>
                <div style={{ fontSize: '18px', fontWeight: 600, marginBottom: '8px' }}>总览页加载失败</div>
                <div style={{ color: 'var(--text-tertiary)', marginBottom: '18px' }}>{error instanceof Error ? error.message : 'Unknown error'}</div>
                <button className="btn btn-primary" onClick={() => window.location.reload()}>重新加载</button>
            </div>
        );
    }

    return (
        <div>
            <div className="page-header">
                <div>
                    <h1 className="page-title">Overview</h1>
                    <div className="page-subtitle">
                        用前端版风格整理系统、模型、渠道、审批和近期动态，作为静态版 Overview 的对应入口。
                    </div>
                </div>
                <div style={{ display: 'flex', gap: '8px' }}>
                    <button className="btn btn-secondary" onClick={() => navigate('/runtime')}>Runtime</button>
                    <button className="btn btn-primary" onClick={() => navigate('/analytics')}>Analytics</button>
                </div>
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, minmax(0, 1fr))', gap: '16px', marginBottom: '24px' }}>
                <MetricCard
                    label="System Health"
                    value={data.health?.status === 'ok' ? 'Healthy' : 'Attention'}
                    sub={`Version ${data.version?.version || data.status?.version || 'unknown'}`}
                    tone={data.health?.status === 'ok' ? 'success' : 'warning'}
                />
                <MetricCard
                    label="Running Agents"
                    value={String(derived.runningAgents)}
                    sub={`${data.agents.length} total agents`}
                    tone={derived.runningAgents > 0 ? 'success' : 'default'}
                />
                <MetricCard
                    label="Provider Coverage"
                    value={`${derived.configuredProviders.length}/${data.providers.length}`}
                    sub="configured providers"
                    tone={derived.configuredProviders.length > 0 ? 'success' : 'warning'}
                />
                <MetricCard
                    label="Pending Approvals"
                    value={String(derived.pendingApprovals.length)}
                    sub="sensitive actions waiting"
                    tone={derived.pendingApprovals.length > 0 ? 'warning' : 'default'}
                />
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: '1.4fr 1fr', gap: '16px', alignItems: 'start' }}>
                <div className="card" style={{ padding: '20px' }}>
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
                        <div>
                            <div style={{ fontSize: '15px', fontWeight: 600 }}>System Snapshot</div>
                            <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginTop: '4px' }}>
                                运行时状态、调用量和可用扩展的横截面。
                            </div>
                        </div>
                        <button className="btn btn-ghost" onClick={() => navigate('/settings')}>Settings</button>
                    </div>
                    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: '12px' }}>
                        {[
                            { label: 'Uptime', value: `${Math.floor((data.status?.uptime_seconds || 0) / 60)} min` },
                            { label: 'Tokens', value: formatNumber((data.usageSummary?.total_input_tokens || 0) + (data.usageSummary?.total_output_tokens || 0)) },
                            { label: 'Cost', value: formatMoney(data.usageSummary?.total_cost_usd || 0) },
                            { label: 'Skills', value: String(data.skills.length) },
                            { label: 'Channels Ready', value: String(derived.readyChannels.length) },
                            { label: 'Default Model', value: data.status?.default_model || '-' },
                        ].map((item) => (
                            <div key={item.label} style={{ padding: '12px 14px', borderRadius: '10px', border: '1px solid var(--border-subtle)', background: 'var(--bg-secondary)' }}>
                                <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginBottom: '6px' }}>{item.label}</div>
                                <div style={{ fontSize: '16px', fontWeight: 600 }}>{item.value}</div>
                            </div>
                        ))}
                    </div>
                </div>

                <div className="card" style={{ padding: '20px' }}>
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '12px' }}>
                        <div style={{ fontSize: '15px', fontWeight: 600 }}>Quick Actions</div>
                        <span className="badge badge-info">Console</span>
                    </div>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                        {[
                            { label: 'Review approvals', path: '/approvals', desc: `${derived.pendingApprovals.length} waiting` },
                            { label: 'Inspect workflows', path: '/workflows', desc: 'automation definitions' },
                            { label: 'Open channels', path: '/channels', desc: `${derived.readyChannels.length} configured` },
                            { label: 'Check hands', path: '/hands', desc: 'active capability packages' },
                        ].map((item) => (
                            <button
                                key={item.path}
                                className="btn btn-secondary"
                                onClick={() => navigate(item.path)}
                                style={{ justifyContent: 'space-between', width: '100%' }}
                            >
                                <span>{item.label}</span>
                                <span style={{ fontSize: '11px', color: 'var(--text-tertiary)' }}>{item.desc}</span>
                            </button>
                        ))}
                    </div>
                </div>
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '16px', marginTop: '16px' }}>
                <div className="card" style={{ padding: '20px' }}>
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '14px' }}>
                        <div style={{ fontSize: '15px', fontWeight: 600 }}>Providers</div>
                        <button className="btn btn-ghost" onClick={() => navigate('/settings')}>Manage</button>
                    </div>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                        {data.providers.length === 0 && (
                            <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>No providers found.</div>
                        )}
                        {data.providers.slice(0, 6).map((provider: any) => {
                            const ready = provider.auth_status === 'configured' || provider.auth_status === 'Configured';
                            return (
                                <div key={provider.id || provider.provider} style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', paddingBottom: '10px', borderBottom: '1px solid var(--border-subtle)' }}>
                                    <div>
                                        <div style={{ fontSize: '13px', fontWeight: 500 }}>{provider.display_name || provider.name || provider.id}</div>
                                        <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '2px' }}>{provider.base_url || provider.protocol || provider.id}</div>
                                    </div>
                                    <span className={`badge ${ready ? 'badge-success' : 'badge-warning'}`}>
                                        {ready ? 'Ready' : (provider.auth_status || 'Setup')}
                                    </span>
                                </div>
                            );
                        })}
                    </div>
                </div>

                <div className="card" style={{ padding: '20px' }}>
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '14px' }}>
                        <div style={{ fontSize: '15px', fontWeight: 600 }}>Recent Audit</div>
                        <button className="btn btn-ghost" onClick={() => navigate('/logs')}>Logs</button>
                    </div>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                        {data.audit.length === 0 && (
                            <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>No recent audit entries.</div>
                        )}
                        {data.audit.map((entry: any) => (
                            <div key={entry.id || `${entry.created_at}-${entry.action}`} style={{ paddingBottom: '10px', borderBottom: '1px solid var(--border-subtle)' }}>
                                <div style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', marginBottom: '4px' }}>
                                    <div style={{ fontSize: '13px', fontWeight: 500 }}>{entry.action || entry.summary || 'Audit event'}</div>
                                    <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', whiteSpace: 'nowrap' }}>{timeAgo(entry.created_at)}</div>
                                </div>
                                <div style={{ fontSize: '12px', color: 'var(--text-secondary)', lineHeight: 1.6 }}>
                                    {entry.summary || entry.detail || JSON.stringify(entry.details || entry.payload || {})}
                                </div>
                            </div>
                        ))}
                    </div>
                </div>
            </div>
        </div>
    );
}
