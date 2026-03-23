import type { ReactNode } from 'react';

export default function Card({ title, subtitle, actions, children }: { title: string; subtitle?: string; actions?: ReactNode; children: ReactNode }) {
    return (
        <div className="card" style={{ padding: '20px' }}>
            <div style={{ marginBottom: '14px', display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: '12px' }}>
                <div>
                    <div style={{ fontSize: '15px', fontWeight: 600 }}>{title}</div>
                    {subtitle && <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{subtitle}</div>}
                </div>
                {actions}
            </div>
            {children}
        </div>
    );
}
