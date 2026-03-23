import type { Scope } from './types';
import Card from './Card';

type ScopeSidebarProps = {
    scopes: Scope[];
    isLoading: boolean;
    selectedScope: string;
    newScopeName: string;
    busy: string | null;
    onNewScopeNameChange: (value: string) => void;
    onCreateScope: () => void;
    onSelectScope: (scopeId: string) => void;
};

export default function ScopeSidebar({
    scopes,
    isLoading,
    selectedScope,
    newScopeName,
    busy,
    onNewScopeNameChange,
    onCreateScope,
    onSelectScope,
}: ScopeSidebarProps) {
    return (
        <Card title="Scopes" subtitle="先选 scope，再在右侧维护资产。">
            <div style={{ display: 'grid', gap: '10px', marginBottom: '14px' }}>
                <input className="form-input" placeholder="新 scope 名称" value={newScopeName} onChange={(event) => onNewScopeNameChange(event.target.value)} />
                <button className="btn btn-primary" disabled={busy === 'scope' || !newScopeName.trim()} onClick={onCreateScope}>
                    {busy === 'scope' ? 'Creating...' : 'Create Scope'}
                </button>
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
                {isLoading && <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>加载 scopes 中...</div>}
                {!isLoading && scopes.length === 0 && <div style={{ color: 'var(--text-tertiary)', fontSize: '13px' }}>No scopes yet.</div>}
                {scopes.map((scope) => (
                    <button
                        key={scope.scope_id}
                        className={selectedScope === scope.scope_id ? 'btn btn-primary' : 'btn btn-secondary'}
                        style={{ justifyContent: 'space-between', width: '100%' }}
                        onClick={() => onSelectScope(scope.scope_id)}
                    >
                        <span>{scope.name || scope.scope_id.slice(0, 8)}</span>
                        <span style={{ fontSize: '11px', opacity: 0.8 }}>{scope.status || scope.scope_type || 'scope'}</span>
                    </button>
                ))}
            </div>
        </Card>
    );
}
