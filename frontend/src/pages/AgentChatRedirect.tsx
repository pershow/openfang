import { useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';

export default function AgentChatRedirect() {
    const navigate = useNavigate();
    const { id } = useParams<{ id: string }>();

    useEffect(() => {
        if (id) {
            navigate(`/agents/${id}#chat`, { replace: true });
        } else {
            navigate('/', { replace: true });
        }
    }, [id, navigate]);

    return null;
}
