import { useEffect, useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { request } from '../services/api';

type QrState = {
    available: boolean;
    dataUrl: string;
    sessionId: string;
    connected: boolean;
    message: string;
    error: string;
};

const emptyQrState = (): QrState => ({
    available: false,
    dataUrl: '',
    sessionId: '',
    connected: false,
    message: '',
    error: '',
});

export default function Channels() {
    const queryClient = useQueryClient();
    const [search, setSearch] = useState('');
    const [selectedChannel, setSelectedChannel] = useState<any | null>(null);
    const [formValues, setFormValues] = useState<Record<string, string>>({});
    const [showAdvanced, setShowAdvanced] = useState(false);
    const [qrState, setQrState] = useState<QrState>(emptyQrState());

    const { data, isLoading, error } = useQuery({
        queryKey: ['channels-console'],
        queryFn: async () => {
            const response = await request<any>('/channels');
            return Array.isArray(response?.channels) ? response.channels : [];
        },
        refetchInterval: 15000,
    });

    const channels = useMemo(() => {
        const items = Array.isArray(data) ? data : [];
        const query = search.trim().toLowerCase();
        if (!query) return items;
        return items.filter((channel: any) => {
            const haystack = [channel.name, channel.display_name, channel.description, channel.category].filter(Boolean).join(' ').toLowerCase();
            return haystack.includes(query);
        });
    }, [data, search]);

    const saveMutation = useMutation({
        mutationFn: ({ name, fields }: { name: string; fields: Record<string, string> }) =>
            request(`/channels/${name}/configure`, { method: 'POST', body: JSON.stringify({ fields }) }),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: ['channels-console'] });
        },
    });

    const testMutation = useMutation({
        mutationFn: (name: string) => request<any>(`/channels/${name}/test`, { method: 'POST', body: JSON.stringify({}) }),
    });

    const removeMutation = useMutation({
        mutationFn: (name: string) => request(`/channels/${name}/configure`, { method: 'DELETE' }),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: ['channels-console'] });
            setSelectedChannel(null);
        },
    });

    const startQrMutation = useMutation({
        mutationFn: () => request<any>('/channels/whatsapp/qr/start', { method: 'POST', body: JSON.stringify({}) }),
        onSuccess: (result) => {
            setQrState({
                available: !!result.available,
                dataUrl: result.qr_data_url || '',
                sessionId: result.session_id || '',
                connected: !!result.connected,
                message: result.message || '',
                error: '',
            });
            queryClient.invalidateQueries({ queryKey: ['channels-console'] });
        },
        onError: (mutationError) => {
            setQrState((current) => ({
                ...current,
                error: mutationError instanceof Error ? mutationError.message : 'QR start failed',
            }));
        },
    });

    useEffect(() => {
        if (!selectedChannel) return;
        const values: Record<string, string> = {};
        (selectedChannel.fields || []).forEach((field: any) => {
            if (field.value !== undefined && field.value !== null && field.type !== 'secret') {
                values[field.key] = String(field.value);
            }
        });
        setFormValues(values);
        setShowAdvanced(false);
        setQrState(emptyQrState());
    }, [selectedChannel]);

    useEffect(() => {
        if (!selectedChannel || !qrState.sessionId || qrState.connected) return undefined;
        const timer = window.setInterval(async () => {
            try {
                const result = await request<any>(`/channels/whatsapp/qr/status?session_id=${encodeURIComponent(qrState.sessionId)}`);
                setQrState((current) => ({
                    ...current,
                    connected: !!result.connected,
                    message: result.message || current.message,
                }));
                if (result.connected) {
                    queryClient.invalidateQueries({ queryKey: ['channels-console'] });
                }
            } catch {
                // keep last state; retry on next interval
            }
        }, 3000);
        return () => window.clearInterval(timer);
    }, [qrState.connected, qrState.sessionId, queryClient, selectedChannel]);

    const visibleFields = (selectedChannel?.fields || []).filter((field: any) => showAdvanced || !field.advanced);

    return (
        <div>
            <div className="page-header">
                <div>
                    <h1 className="page-title">Channels</h1>
                    <div className="page-subtitle">补齐静态版 Channels 页面，统一查看全局渠道接入状态，并支持配置、测试与移除。</div>
                </div>
                <input
                    value={search}
                    onChange={(event) => setSearch(event.target.value)}
                    placeholder="搜索渠道 / 分类"
                    style={{ width: '240px' }}
                />
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))', gap: '16px' }}>
                {isLoading && <div className="card" style={{ padding: '24px', color: 'var(--text-tertiary)' }}>加载 channels 中...</div>}
                {!isLoading && error && <div className="card" style={{ padding: '24px', color: 'var(--error)' }}>{error instanceof Error ? error.message : 'Failed to load channels'}</div>}
                {!isLoading && !error && channels.length === 0 && (
                    <div className="card" style={{ padding: '32px', color: 'var(--text-tertiary)', textAlign: 'center' }}>没有匹配的渠道。</div>
                )}
                {!isLoading && !error && channels.map((channel: any) => {
                    const ready = channel.connected || (channel.configured && channel.has_token);
                    return (
                        <div key={channel.name} className="card card-clickable" style={{ padding: '18px' }} onClick={() => setSelectedChannel(channel)}>
                            <div style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', alignItems: 'start', marginBottom: '10px' }}>
                                <div>
                                    <div style={{ fontSize: '15px', fontWeight: 700 }}>{channel.display_name || channel.name}</div>
                                    <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{channel.category || 'channel'}</div>
                                </div>
                                <span className={`badge ${ready ? 'badge-success' : channel.configured ? 'badge-warning' : 'badge-info'}`}>
                                    {ready ? 'Ready' : channel.configured ? 'Configured' : 'Not set'}
                                </span>
                            </div>
                            <div style={{ fontSize: '12px', color: 'var(--text-secondary)', lineHeight: 1.7 }}>{channel.description || 'No description'}</div>
                        </div>
                    );
                })}
            </div>

            {selectedChannel && (
                <div style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.55)', zIndex: 1200, display: 'flex', alignItems: 'center', justifyContent: 'center', padding: '24px' }} onClick={() => setSelectedChannel(null)}>
                    <div className="card" style={{ width: 'min(760px, 100%)', maxHeight: '88vh', overflow: 'auto', padding: '20px', boxShadow: 'var(--shadow-lg)' }} onClick={(event) => event.stopPropagation()}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
                            <div>
                                <div style={{ fontSize: '18px', fontWeight: 700 }}>{selectedChannel.display_name || selectedChannel.name}</div>
                                <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{selectedChannel.description || 'Configure this channel connection'}</div>
                            </div>
                            <button className="btn btn-ghost" onClick={() => setSelectedChannel(null)}>关闭</button>
                        </div>

                        {selectedChannel.setup_type === 'qr' && (
                            <div className="card" style={{ padding: '16px', marginBottom: '16px', background: 'var(--bg-secondary)' }}>
                                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '12px' }}>
                                    <div style={{ fontSize: '14px', fontWeight: 600 }}>QR Login</div>
                                    <button className="btn btn-secondary" onClick={() => startQrMutation.mutate()} disabled={startQrMutation.isPending}>
                                        {startQrMutation.isPending ? 'Generating...' : 'Generate QR'}
                                    </button>
                                </div>
                                {qrState.available && qrState.dataUrl && (
                                    <div style={{ display: 'flex', gap: '16px', alignItems: 'center' }}>
                                        <img src={qrState.dataUrl} alt="QR code" style={{ width: '180px', height: '180px', borderRadius: '12px', background: '#fff', padding: '8px' }} />
                                        <div style={{ fontSize: '12px', color: 'var(--text-secondary)', lineHeight: 1.8 }}>
                                            <div>{qrState.connected ? '设备已连接。' : '扫描二维码完成绑定。'}</div>
                                            <div>{qrState.message || '等待扫码...'}</div>
                                            {qrState.error && <div style={{ color: 'var(--error)' }}>{qrState.error}</div>}
                                        </div>
                                    </div>
                                )}
                                {!qrState.available && !startQrMutation.isPending && (
                                    <div style={{ fontSize: '12px', color: 'var(--text-tertiary)' }}>该渠道使用二维码接入，可在这里生成二维码并轮询连接状态。</div>
                                )}
                            </div>
                        )}

                        {(selectedChannel.fields || []).length > 0 && (
                            <>
                                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '12px' }}>
                                    <div style={{ fontSize: '14px', fontWeight: 600 }}>Configuration</div>
                                    {(selectedChannel.fields || []).some((field: any) => field.advanced) && (
                                        <button className="btn btn-ghost" onClick={() => setShowAdvanced((current) => !current)}>
                                            {showAdvanced ? 'Hide advanced' : 'Show advanced'}
                                        </button>
                                    )}
                                </div>
                                <div style={{ display: 'grid', gap: '12px', marginBottom: '16px' }}>
                                    {visibleFields.map((field: any) => (
                                        <div key={field.key}>
                                            <label className="form-label">{field.label || field.key}</label>
                                            {field.type === 'textarea' ? (
                                                <textarea
                                                    className="form-textarea"
                                                    value={formValues[field.key] || ''}
                                                    placeholder={field.placeholder || ''}
                                                    onChange={(event) => setFormValues({ ...formValues, [field.key]: event.target.value })}
                                                />
                                            ) : (
                                                <input
                                                    className="form-input"
                                                    type={field.type === 'secret' ? 'password' : 'text'}
                                                    value={formValues[field.key] || ''}
                                                    placeholder={field.placeholder || ''}
                                                    onChange={(event) => setFormValues({ ...formValues, [field.key]: event.target.value })}
                                                />
                                            )}
                                            {field.help_text && <div style={{ fontSize: '11px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{field.help_text}</div>}
                                        </div>
                                    ))}
                                </div>
                            </>
                        )}

                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '12px' }}>
                            <div style={{ display: 'flex', gap: '8px' }}>
                                <button
                                    className="btn btn-primary"
                                    onClick={() => saveMutation.mutate({ name: selectedChannel.name, fields: formValues })}
                                    disabled={saveMutation.isPending}
                                >
                                    {saveMutation.isPending ? 'Saving...' : 'Save'}
                                </button>
                                <button
                                    className="btn btn-secondary"
                                    onClick={() => testMutation.mutate(selectedChannel.name)}
                                    disabled={testMutation.isPending}
                                >
                                    {testMutation.isPending ? 'Testing...' : 'Test'}
                                </button>
                            </div>
                            {selectedChannel.configured && (
                                <button
                                    className="btn btn-ghost"
                                    style={{ color: 'var(--error)' }}
                                    onClick={() => {
                                        if (window.confirm(`确定移除 ${selectedChannel.display_name || selectedChannel.name} 的配置吗？`)) {
                                            removeMutation.mutate(selectedChannel.name);
                                        }
                                    }}
                                    disabled={removeMutation.isPending}
                                >
                                    Remove
                                </button>
                            )}
                        </div>

                        {(testMutation.data || saveMutation.isSuccess) && (
                            <div style={{ marginTop: '14px', fontSize: '12px', color: 'var(--text-secondary)' }}>
                                {testMutation.data?.message || 'Configuration saved.'}
                            </div>
                        )}
                    </div>
                </div>
            )}
        </div>
    );
}
