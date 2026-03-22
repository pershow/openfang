import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { request } from '../services/api';

type HandSetup = {
    id: string;
    name: string;
    settings: any[];
    requirements: any[];
};

export default function Hands() {
    const queryClient = useQueryClient();
    const [tab, setTab] = useState<'available' | 'active'>('available');
    const [setupHand, setSetupHand] = useState<HandSetup | null>(null);
    const [settingsValues, setSettingsValues] = useState<Record<string, string>>({});

    const { data, isLoading, error } = useQuery({
        queryKey: ['hands-console'],
        queryFn: async () => {
            const [handsRes, activeRes] = await Promise.all([
                request<any>('/hands').catch(() => ({ hands: [] })),
                request<any>('/hands/active').catch(() => ({ instances: [] })),
            ]);
            return {
                hands: Array.isArray(handsRes?.hands) ? handsRes.hands : [],
                active: Array.isArray(activeRes?.instances) ? activeRes.instances : [],
            };
        },
        refetchInterval: 15000,
    });

    const activateMutation = useMutation({
        mutationFn: ({ handId, config }: { handId: string; config: Record<string, string> }) =>
            request(`/hands/${handId}/activate`, { method: 'POST', body: JSON.stringify({ config }) }),
        onSuccess: () => {
            setSetupHand(null);
            setSettingsValues({});
            queryClient.invalidateQueries({ queryKey: ['hands-console'] });
        },
    });

    const pauseMutation = useMutation({
        mutationFn: (id: string) => request(`/hands/instances/${id}/pause`, { method: 'POST', body: JSON.stringify({}) }),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: ['hands-console'] }),
    });

    const resumeMutation = useMutation({
        mutationFn: (id: string) => request(`/hands/instances/${id}/resume`, { method: 'POST', body: JSON.stringify({}) }),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: ['hands-console'] }),
    });

    const deactivateMutation = useMutation({
        mutationFn: (id: string) => request(`/hands/instances/${id}`, { method: 'DELETE' }),
        onSuccess: () => queryClient.invalidateQueries({ queryKey: ['hands-console'] }),
    });

    const openSetup = async (handId: string) => {
        const detail = await request<any>(`/hands/${handId}`);
        const values: Record<string, string> = {};
        (detail.settings || []).forEach((setting: any) => {
            values[setting.key] = String(setting.default ?? '');
        });
        setSettingsValues(values);
        setSetupHand({
            id: handId,
            name: detail.display_name || detail.name || handId,
            settings: Array.isArray(detail.settings) ? detail.settings : [],
            requirements: Array.isArray(detail.requirements) ? detail.requirements : [],
        });
    };

    const hands = data?.hands || [];
    const activeInstances = data?.active || [];

    return (
        <div>
            <div className="page-header">
                <div>
                    <h1 className="page-title">Hands</h1>
                    <div className="page-subtitle">补齐静态版 Hands 页面，用当前前端风格展示可安装能力包和正在运行的实例。</div>
                </div>
                <div style={{ display: 'flex', gap: '8px' }}>
                    <button className={tab === 'available' ? 'btn btn-primary' : 'btn btn-secondary'} onClick={() => setTab('available')}>Available</button>
                    <button className={tab === 'active' ? 'btn btn-primary' : 'btn btn-secondary'} onClick={() => setTab('active')}>Active</button>
                </div>
            </div>

            {isLoading && <div className="card" style={{ padding: '24px', color: 'var(--text-tertiary)' }}>加载 hands 中...</div>}
            {!isLoading && error && <div className="card" style={{ padding: '24px', color: 'var(--error)' }}>{error instanceof Error ? error.message : 'Failed to load hands'}</div>}

            {!isLoading && !error && tab === 'available' && (
                <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))', gap: '16px' }}>
                    {hands.length === 0 && <div className="card" style={{ padding: '32px', color: 'var(--text-tertiary)', textAlign: 'center' }}>No hands available.</div>}
                    {hands.map((hand: any) => (
                        <div key={hand.id} className="card" style={{ padding: '18px' }}>
                            <div style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', alignItems: 'start', marginBottom: '10px' }}>
                                <div>
                                    <div style={{ fontSize: '16px', fontWeight: 700 }}>{hand.display_name || hand.name || hand.id}</div>
                                    <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{hand.id}</div>
                                </div>
                                <span className="badge badge-info">{hand.category || 'hand'}</span>
                            </div>
                            <div style={{ fontSize: '12px', color: 'var(--text-secondary)', lineHeight: 1.7, marginBottom: '14px' }}>
                                {hand.description || 'No description'}
                            </div>
                            <div style={{ display: 'flex', justifyContent: 'space-between', gap: '8px', alignItems: 'center' }}>
                                <div style={{ fontSize: '11px', color: 'var(--text-tertiary)' }}>
                                    {(hand.requirements || []).length} requirements
                                </div>
                                <button className="btn btn-primary" onClick={() => openSetup(hand.id)}>Activate</button>
                            </div>
                        </div>
                    ))}
                </div>
            )}

            {!isLoading && !error && tab === 'active' && (
                <div className="card" style={{ padding: '0', overflow: 'hidden' }}>
                    <div style={{ display: 'grid', gridTemplateColumns: '1.2fr 0.8fr 0.8fr 1fr', gap: '12px', padding: '12px 16px', borderBottom: '1px solid var(--border-subtle)', fontSize: '11px', textTransform: 'uppercase', color: 'var(--text-tertiary)', letterSpacing: '0.05em' }}>
                        <span>Instance</span>
                        <span>Hand</span>
                        <span>Status</span>
                        <span style={{ textAlign: 'right' }}>Actions</span>
                    </div>
                    {activeInstances.length === 0 && (
                        <div style={{ padding: '32px 16px', color: 'var(--text-tertiary)', textAlign: 'center' }}>没有正在运行的 hand 实例。</div>
                    )}
                    {activeInstances.map((instance: any) => (
                        <div key={instance.instance_id || instance.id} style={{ display: 'grid', gridTemplateColumns: '1.2fr 0.8fr 0.8fr 1fr', gap: '12px', alignItems: 'center', padding: '16px', borderBottom: '1px solid var(--border-subtle)' }}>
                            <div style={{ minWidth: 0 }}>
                                <div style={{ fontSize: '13px', fontWeight: 600 }}>{instance.agent_name || instance.instance_id || instance.id}</div>
                                <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '6px' }}>{instance.instance_id || instance.id}</div>
                            </div>
                            <div style={{ fontSize: '13px', fontWeight: 500 }}>{instance.hand_id || instance.hand_name || '-'}</div>
                            <div>
                                <span className={`badge ${instance.status === 'Active' || instance.status === 'Running' ? 'badge-success' : 'badge-warning'}`}>
                                    {instance.status || 'unknown'}
                                </span>
                            </div>
                            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '8px' }}>
                                {instance.status === 'Paused' ? (
                                    <button className="btn btn-secondary" onClick={() => resumeMutation.mutate(instance.instance_id || instance.id)} disabled={resumeMutation.isPending}>Resume</button>
                                ) : (
                                    <button className="btn btn-ghost" onClick={() => pauseMutation.mutate(instance.instance_id || instance.id)} disabled={pauseMutation.isPending}>Pause</button>
                                )}
                                <button
                                    className="btn btn-ghost"
                                    style={{ color: 'var(--error)' }}
                                    disabled={deactivateMutation.isPending}
                                    onClick={() => {
                                        if (window.confirm(`确定停用实例 ${instance.agent_name || instance.instance_id} 吗？`)) {
                                            deactivateMutation.mutate(instance.instance_id || instance.id);
                                        }
                                    }}
                                >
                                    Deactivate
                                </button>
                            </div>
                        </div>
                    ))}
                </div>
            )}

            {setupHand && (
                <div style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.55)', zIndex: 1200, display: 'flex', alignItems: 'center', justifyContent: 'center', padding: '24px' }} onClick={() => setSetupHand(null)}>
                    <div className="card" style={{ width: 'min(720px, 100%)', maxHeight: '88vh', overflow: 'auto', padding: '20px', boxShadow: 'var(--shadow-lg)' }} onClick={(event) => event.stopPropagation()}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
                            <div>
                                <div style={{ fontSize: '18px', fontWeight: 700 }}>{setupHand.name}</div>
                                <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginTop: '4px' }}>填写设置后激活 hand 实例。</div>
                            </div>
                            <button className="btn btn-ghost" onClick={() => setSetupHand(null)}>关闭</button>
                        </div>

                        {(setupHand.requirements || []).length > 0 && (
                            <div className="card" style={{ padding: '16px', marginBottom: '16px', background: 'var(--bg-secondary)' }}>
                                <div style={{ fontSize: '14px', fontWeight: 600, marginBottom: '10px' }}>Requirements</div>
                                <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
                                    {setupHand.requirements.map((requirement: any) => (
                                        <div key={requirement.key || requirement.name} style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', paddingBottom: '8px', borderBottom: '1px solid var(--border-subtle)' }}>
                                            <div>
                                                <div style={{ fontSize: '13px', fontWeight: 500 }}>{requirement.label || requirement.name || requirement.key}</div>
                                                <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{requirement.help_text || requirement.type || 'Requirement'}</div>
                                            </div>
                                            <span className={`badge ${requirement.satisfied ? 'badge-success' : 'badge-warning'}`}>{requirement.satisfied ? 'met' : 'required'}</span>
                                        </div>
                                    ))}
                                </div>
                            </div>
                        )}

                        <div style={{ display: 'grid', gap: '12px', marginBottom: '16px' }}>
                            {(setupHand.settings || []).map((setting: any) => (
                                <div key={setting.key}>
                                    <label className="form-label">{setting.label || setting.key}</label>
                                    {setting.setting_type === 'toggle' ? (
                                        <select className="form-input" value={settingsValues[setting.key] || 'false'} onChange={(event) => setSettingsValues({ ...settingsValues, [setting.key]: event.target.value })}>
                                            <option value="false">Disabled</option>
                                            <option value="true">Enabled</option>
                                        </select>
                                    ) : Array.isArray(setting.options) && setting.options.length > 0 ? (
                                        <select className="form-input" value={settingsValues[setting.key] || ''} onChange={(event) => setSettingsValues({ ...settingsValues, [setting.key]: event.target.value })}>
                                            {setting.options.map((option: any) => (
                                                <option key={option.value || option} value={option.value || option}>{option.label || option.value || option}</option>
                                            ))}
                                        </select>
                                    ) : (
                                        <input className="form-input" value={settingsValues[setting.key] || ''} onChange={(event) => setSettingsValues({ ...settingsValues, [setting.key]: event.target.value })} />
                                    )}
                                    {setting.help_text && <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{setting.help_text}</div>}
                                </div>
                            ))}
                            {(setupHand.settings || []).length === 0 && (
                                <div style={{ fontSize: '12px', color: 'var(--text-tertiary)' }}>该 hand 不需要额外配置，可以直接激活。</div>
                            )}
                        </div>

                        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '8px' }}>
                            <button className="btn btn-secondary" onClick={() => setSetupHand(null)}>Cancel</button>
                            <button className="btn btn-primary" onClick={() => activateMutation.mutate({ handId: setupHand.id, config: settingsValues })} disabled={activateMutation.isPending}>
                                {activateMutation.isPending ? 'Activating...' : 'Activate'}
                            </button>
                        </div>
                    </div>
                </div>
            )}
        </div>
    );
}
