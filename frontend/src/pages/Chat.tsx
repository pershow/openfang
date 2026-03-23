import { Link, useParams } from 'react-router-dom';

export default function Chat() {
    const { id } = useParams<{ id: string }>();

    return (
        <div className="card" style={{ maxWidth: '720px', margin: '32px auto', padding: '24px' }}>
            <h2 style={{ marginTop: 0, marginBottom: '12px' }}>Legacy Chat Page</h2>
            <p style={{ marginBottom: '12px', color: 'var(--text-secondary)' }}>
                This standalone chat page has been retired. Chat is now handled from the agent detail page so it can stay aligned with the current backend session APIs.
            </p>
            {id ? (
                <Link to={`/agents/${id}`} className="btn btn-primary">
                    Open Agent Detail
                </Link>
            ) : (
                <Link to="/dashboard" className="btn btn-primary">
                    Back to Dashboard
                </Link>
            )}
        </div>
    );
}
