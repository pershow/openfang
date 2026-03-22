import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { request } from '../services/api';

const formatNumber = (value: number) => {
    if (!value) return '0';
    if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
    if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
    return String(value);
};

const formatMoney = (value: number) => {
    if (!value) return '$0.00';
    if (value < 0.01) return `$${value.toFixed(4)}`;
    return `$${value.toFixed(2)}`;
};

export default function Analytics() {
    const { data, isLoading, error } = useQuery({
        queryKey: ['analytics-console'],
        queryFn: async () => {
            const [summary, byModelRes, byAgentRes, dailyRes] = await Promise.all([
                request<any>('/usage/summary').catch(() => ({})),
                request<any>('/usage/by-model').catch(() => ({ models: [] })),
                request<any>('/usage').catch(() => ({ agents: [] })),
                request<any>('/usage/daily').catch(() => ({ days: [] })),
            ]);
            return {
                summary,
                byModel: Array.isArray(byModelRes?.models) ? byModelRes.models : [],
                byAgent: Array.isArray(byAgentRes?.agents) ? byAgentRes.agents : [],
                daily: Array.isArray(dailyRes?.days) ? dailyRes.days : [],
            };
        },
        refetchInterval: 30000,
    });

    const maxDailyCost = useMemo(() => {
        const values = (data?.daily || []).map((day: any) => day.cost_usd || 0);
        return Math.max(...values, 1);
    }, [data]);

    if (isLoading) {
        return <div style={{ padding: '48px 0', textAlign: 'center', color: 'var(--text-tertiary)' }}>加载分析数据中...</div>;
    }

    if (error || !data) {
        return (
            <div className="card" style={{ textAlign: 'center', padding: '36px' }}>
                <div style={{ fontSize: '16px', fontWeight: 600, marginBottom: '8px' }}>Analytics 不可用</div>
                <div style={{ color: 'var(--text-tertiary)' }}>{error instanceof Error ? error.message : 'Unknown error'}</div>
            </div>
        );
    }

    return (
        <div>
            <div className="page-header">
                <div>
                    <h1 className="page-title">Analytics</h1>
                    <div className="page-subtitle">对应静态版 Usage / Analytics 页面，集中看调用量、成本、模型和 Agent 消耗。</div>
                </div>
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, minmax(0, 1fr))', gap: '16px', marginBottom: '24px' }}>
                {[
                    { label: 'LLM Calls', value: String(data.summary?.call_count || 0), sub: 'total requests' },
                    { label: 'Token Volume', value: formatNumber((data.summary?.total_input_tokens || 0) + (data.summary?.total_output_tokens || 0)), sub: 'input + output' },
                    { label: 'Tool Calls', value: formatNumber(data.summary?.total_tool_calls || 0), sub: 'executed tools' },
                    { label: 'Cost', value: formatMoney(data.summary?.total_cost_usd || 0), sub: 'lifetime cost' },
                ].map((item) => (
                    <div key={item.label} className="card" style={{ padding: '18px 20px' }}>
                        <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginBottom: '8px' }}>{item.label}</div>
                        <div style={{ fontSize: '28px', fontWeight: 700, letterSpacing: '-0.03em' }}>{item.value}</div>
                        <div style={{ fontSize: '12px', color: 'var(--text-secondary)', marginTop: '6px' }}>{item.sub}</div>
                    </div>
                ))}
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: '1.1fr 1fr', gap: '16px', alignItems: 'start' }}>
                <div className="card" style={{ padding: '20px' }}>
                    <div style={{ fontSize: '15px', fontWeight: 600, marginBottom: '16px' }}>Daily Cost Trend</div>
                    <div style={{ display: 'flex', alignItems: 'end', gap: '10px', minHeight: '220px' }}>
                        {data.daily.length === 0 && (
                            <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>No daily cost samples yet.</div>
                        )}
                        {data.daily.map((day: any) => {
                            const height = Math.max(8, Math.round(((day.cost_usd || 0) / maxDailyCost) * 160));
                            return (
                                <div key={day.date} style={{ flex: 1, minWidth: 0, textAlign: 'center' }}>
                                    <div
                                        title={`${day.date}: ${formatMoney(day.cost_usd || 0)}`}
                                        style={{
                                            height: `${height}px`,
                                            borderRadius: '10px 10px 4px 4px',
                                            background: 'linear-gradient(180deg, rgba(225,225,232,0.95) 0%, rgba(225,225,232,0.2) 100%)',
                                            border: '1px solid var(--border-default)',
                                            marginBottom: '8px',
                                        }}
                                    />
                                    <div style={{ fontSize: '11px', color: 'var(--text-tertiary)' }}>{day.date?.slice(5) || '-'}</div>
                                </div>
                            );
                        })}
                    </div>
                </div>

                <div className="card" style={{ padding: '20px' }}>
                    <div style={{ fontSize: '15px', fontWeight: 600, marginBottom: '16px' }}>Model Cost Breakdown</div>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
                        {data.byModel.length === 0 && (
                            <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>No model usage data.</div>
                        )}
                        {data.byModel
                            .slice()
                            .sort((a: any, b: any) => (b.total_cost_usd || 0) - (a.total_cost_usd || 0))
                            .slice(0, 8)
                            .map((model: any) => {
                                const top = data.byModel[0]?.total_cost_usd || 1;
                                const width = Math.max(6, Math.round(((model.total_cost_usd || 0) / top) * 100));
                                return (
                                    <div key={model.model}>
                                        <div style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', marginBottom: '6px' }}>
                                            <div style={{ fontSize: '13px', fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{model.model}</div>
                                            <div style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>{formatMoney(model.total_cost_usd || 0)}</div>
                                        </div>
                                        <div style={{ height: '8px', background: 'var(--bg-tertiary)', borderRadius: '999px', overflow: 'hidden' }}>
                                            <div style={{ width: `${width}%`, height: '100%', background: 'var(--accent-primary)' }} />
                                        </div>
                                    </div>
                                );
                            })}
                    </div>
                </div>
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '16px', marginTop: '16px' }}>
                <div className="card" style={{ padding: '20px' }}>
                    <div style={{ fontSize: '15px', fontWeight: 600, marginBottom: '12px' }}>Top Models by Tokens</div>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                        {data.byModel.slice(0, 8).map((model: any) => (
                            <div key={`${model.model}-tokens`} style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', paddingBottom: '10px', borderBottom: '1px solid var(--border-subtle)' }}>
                                <div style={{ minWidth: 0 }}>
                                    <div style={{ fontSize: '13px', fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{model.model}</div>
                                    <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '2px' }}>
                                        {model.call_count || 0} calls
                                    </div>
                                </div>
                                <div style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>
                                    {formatNumber((model.total_input_tokens || 0) + (model.total_output_tokens || 0))}
                                </div>
                            </div>
                        ))}
                    </div>
                </div>

                <div className="card" style={{ padding: '20px' }}>
                    <div style={{ fontSize: '15px', fontWeight: 600, marginBottom: '12px' }}>Top Agents by Cost</div>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                        {data.byAgent.length === 0 && (
                            <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>No agent-level cost data.</div>
                        )}
                        {data.byAgent
                            .slice()
                            .sort((a: any, b: any) => (b.cost_usd || 0) - (a.cost_usd || 0))
                            .slice(0, 8)
                            .map((agent: any) => (
                                <div key={agent.agent_id || agent.id} style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', paddingBottom: '10px', borderBottom: '1px solid var(--border-subtle)' }}>
                                    <div style={{ minWidth: 0 }}>
                                        <div style={{ fontSize: '13px', fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                                            {agent.agent_name || agent.name || agent.agent_id || 'Unknown agent'}
                                        </div>
                                        <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '2px' }}>
                                            {formatNumber(agent.total_tokens || ((agent.input_tokens || 0) + (agent.output_tokens || 0)))} tokens
                                        </div>
                                    </div>
                                    <div style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>
                                        {formatMoney(agent.cost_usd || 0)}
                                    </div>
                                </div>
                            ))}
                    </div>
                </div>
            </div>
        </div>
    );
}
