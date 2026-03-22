import { useQuery } from '@tanstack/react-query';
import { agentApi, request } from '../services/api';

const formatUptime = (seconds: number) => {
    if (!seconds) return '-';
    const days = Math.floor(seconds / 86400);
    const hours = Math.floor((seconds % 86400) / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    if (days > 0) return `${days}d ${hours}h`;
    if (hours > 0) return `${hours}h ${minutes}m`;
    return `${minutes}m`;
};

export default function Runtime() {
    const { data, isLoading, error } = useQuery({
        queryKey: ['runtime-console'],
        queryFn: async () => {
            const [status, version, providersRes, peersRes, securityRes, agents] = await Promise.all([
                request<any>('/status').catch(() => ({})),
                request<any>('/version').catch(() => ({})),
                request<any>('/providers').catch(() => ({ providers: [] })),
                request<any>('/peers').catch(() => ({ peers: [] })),
                request<any>('/security').catch(() => ({})),
                agentApi.list().catch(() => []),
            ]);
            return {
                status,
                version,
                providers: Array.isArray(providersRes?.providers) ? providersRes.providers : [],
                peers: Array.isArray(peersRes?.peers) ? peersRes.peers : Array.isArray(peersRes) ? peersRes : [],
                security: securityRes,
                agents,
            };
        },
        refetchInterval: 15000,
    });

    if (isLoading) {
        return <div style={{ padding: '48px 0', textAlign: 'center', color: 'var(--text-tertiary)' }}>加载运行时状态中...</div>;
    }

    if (error || !data) {
        return (
            <div className="card" style={{ textAlign: 'center', padding: '36px' }}>
                <div style={{ fontSize: '16px', fontWeight: 600, marginBottom: '8px' }}>Runtime 页面暂不可用</div>
                <div style={{ color: 'var(--text-tertiary)' }}>{error instanceof Error ? error.message : 'Unknown error'}</div>
            </div>
        );
    }

    return (
        <div>
            <div className="page-header">
                <div>
                    <h1 className="page-title">Runtime</h1>
                    <div className="page-subtitle">对应静态版 Runtime 页面，聚合版本、进程状态、Provider、Peer 和安全开关。</div>
                </div>
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, minmax(0, 1fr))', gap: '16px', marginBottom: '24px' }}>
                {[
                    { label: 'Version', value: data.version?.version || data.status?.version || '-', sub: `${data.version?.platform || 'unknown'} / ${data.version?.arch || 'unknown'}` },
                    { label: 'Uptime', value: formatUptime(data.status?.uptime_seconds || 0), sub: `listen ${data.status?.api_listen || data.status?.listen || '-'}` },
                    { label: 'Agents', value: String(data.agents.length), sub: `${data.status?.agent_count || data.agents.length} active in runtime` },
                    { label: 'Providers Ready', value: String(data.providers.filter((provider: any) => provider.auth_status === 'configured' || provider.auth_status === 'Configured').length), sub: `${data.providers.length} providers detected` },
                ].map((item) => (
                    <div key={item.label} className="card" style={{ padding: '18px 20px' }}>
                        <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginBottom: '8px' }}>{item.label}</div>
                        <div style={{ fontSize: '28px', fontWeight: 700, letterSpacing: '-0.03em' }}>{item.value}</div>
                        <div style={{ fontSize: '12px', color: 'var(--text-secondary)', marginTop: '6px' }}>{item.sub}</div>
                    </div>
                ))}
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: '1.05fr 0.95fr', gap: '16px', alignItems: 'start' }}>
                <div className="card" style={{ padding: '20px' }}>
                    <div style={{ fontSize: '15px', fontWeight: 600, marginBottom: '14px' }}>Runtime Details</div>
                    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: '12px' }}>
                        {[
                            { label: 'Default Model', value: data.status?.default_model || '-' },
                            { label: 'Default Provider', value: data.status?.default_provider || '-' },
                            { label: 'Home Dir', value: data.status?.home_dir || '-' },
                            { label: 'Log Level', value: data.status?.log_level || '-' },
                            { label: 'Network Enabled', value: data.status?.network_enabled ? 'Yes' : 'No' },
                            { label: 'Security Mode', value: data.security?.mode || data.security?.status || 'standard' },
                        ].map((item) => (
                            <div key={item.label} style={{ padding: '12px 14px', background: 'var(--bg-secondary)', border: '1px solid var(--border-subtle)', borderRadius: '10px' }}>
                                <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginBottom: '6px' }}>{item.label}</div>
                                <div style={{ fontSize: '13px', fontWeight: 600, wordBreak: 'break-word' }}>{item.value}</div>
                            </div>
                        ))}
                    </div>
                </div>

                <div className="card" style={{ padding: '20px' }}>
                    <div style={{ fontSize: '15px', fontWeight: 600, marginBottom: '14px' }}>Provider Health</div>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                        {data.providers.length === 0 && <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>No providers reported.</div>}
                        {data.providers.map((provider: any) => {
                            const ready = provider.auth_status === 'configured' || provider.auth_status === 'Configured';
                            return (
                                <div key={provider.id || provider.provider} style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', paddingBottom: '10px', borderBottom: '1px solid var(--border-subtle)' }}>
                                    <div>
                                        <div style={{ fontSize: '13px', fontWeight: 500 }}>{provider.display_name || provider.id}</div>
                                        <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{provider.base_url || provider.protocol || provider.provider}</div>
                                    </div>
                                    <span className={`badge ${ready ? 'badge-success' : 'badge-warning'}`}>
                                        {provider.health || provider.auth_status || 'unknown'}
                                    </span>
                                </div>
                            );
                        })}
                    </div>
                </div>
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '16px', marginTop: '16px' }}>
                <div className="card" style={{ padding: '20px' }}>
                    <div style={{ fontSize: '15px', fontWeight: 600, marginBottom: '14px' }}>Peer Connections</div>
                    {data.peers.length === 0 ? (
                        <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>No peers connected.</div>
                    ) : (
                        <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                            {data.peers.map((peer: any) => (
                                <div key={peer.id || peer.peer_id || peer.node_id || peer.name} style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', paddingBottom: '10px', borderBottom: '1px solid var(--border-subtle)' }}>
                                    <div>
                                        <div style={{ fontSize: '13px', fontWeight: 500 }}>{peer.name || peer.node_name || peer.peer_id || peer.node_id || peer.id}</div>
                                        <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{peer.address || peer.endpoint || '-'}</div>
                                    </div>
                                    <span className={`badge ${String(peer.status || peer.state || '').toLowerCase().includes('connected') ? 'badge-success' : 'badge-warning'}`}>
                                        {peer.status || peer.state || 'unknown'}
                                    </span>
                                </div>
                            ))}
                        </div>
                    )}
                </div>

                <div className="card" style={{ padding: '20px' }}>
                    <div style={{ fontSize: '15px', fontWeight: 600, marginBottom: '14px' }}>Security Snapshot</div>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                        {Object.entries(data.security || {}).length === 0 && (
                            <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>Security endpoint returned no details.</div>
                        )}
                        {Object.entries(data.security || {}).map(([key, value]) => (
                            <div key={key} style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', paddingBottom: '10px', borderBottom: '1px solid var(--border-subtle)' }}>
                                <span style={{ fontSize: '13px', color: 'var(--text-secondary)' }}>{key}</span>
                                <span style={{ fontSize: '12px', fontWeight: 600, color: 'var(--text-primary)', wordBreak: 'break-word', textAlign: 'right' }}>
                                    {typeof value === 'object' ? JSON.stringify(value) : String(value)}
                                </span>
                            </div>
                        ))}
                    </div>
                </div>
            </div>
        </div>
    );
}
