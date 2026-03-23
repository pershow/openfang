import { Routes, Route, Navigate } from 'react-router-dom';
import { useAuthStore } from './stores';
import { useEffect, useState } from 'react';
import { authApi } from './services/api';
import Login from './pages/Login';
import CompanySetup from './pages/CompanySetup';
import Layout from './pages/Layout';
import Dashboard from './pages/Dashboard';
import Plaza from './pages/Plaza';
import AgentDetail from './pages/AgentDetail';
import AgentCreate from './pages/AgentCreate';
import AgentChatRedirect from './pages/AgentChatRedirect';
import Messages from './pages/Messages';
import EnterpriseSettings from './pages/EnterpriseSettings';
import InvitationCodes from './pages/InvitationCodes';
import AdminCompanies from './pages/AdminCompanies';
import Overview from './pages/Overview';
import Analytics from './pages/Analytics';
import Sessions from './pages/Sessions';
import Approvals from './pages/Approvals';
import Comms from './pages/Comms';
import Workflows from './pages/Workflows';
import Scheduler from './pages/Scheduler';
import Channels from './pages/Channels';
import Skills from './pages/Skills';
import Hands from './pages/Hands';
import Control from './pages/Control';
import Runtime from './pages/Runtime';
import Settings from './pages/Settings';
import Logs from './pages/Logs';

function ProtectedRoute({ children }: { children: React.ReactNode }) {
    const token = useAuthStore((s) => s.token);
    const user = useAuthStore((s) => s.user);
    if (!token) return <Navigate to="/login" replace />;
    // Force company setup for users without a tenant
    if (user && !user.tenant_id) return <Navigate to="/setup-company" replace />;
    return <>{children}</>;
}

/* ─── Notification Bar ─── */
function NotificationBar() {
    const [config, setConfig] = useState<{ enabled: boolean; text: string } | null>(null);
    const [dismissed, setDismissed] = useState(false);

    useEffect(() => {
        fetch('/api/enterprise/system-settings/notification_bar/public')
            .then(r => r.ok ? r.json() : null)
            .then(d => { if (d) setConfig(d); })
            .catch(() => { });
    }, []);

    // Check sessionStorage for dismissal (keyed by text so new messages re-show)
    useEffect(() => {
        if (config?.text) {
            const key = `notification_bar_dismissed_${btoa(encodeURIComponent(config.text))}`;
            if (sessionStorage.getItem(key)) setDismissed(true);
        }
    }, [config?.text]);

    // Manage body class: add when visible, remove when hidden or dismissed
    const isVisible = !!config?.enabled && !!config?.text && !dismissed;
    useEffect(() => {
        if (isVisible) {
            document.body.classList.add('has-notification-bar');
        } else {
            document.body.classList.remove('has-notification-bar');
        }
        return () => { document.body.classList.remove('has-notification-bar'); };
    }, [isVisible]);

    if (!isVisible) return null;

    const handleDismiss = () => {
        const key = `notification_bar_dismissed_${btoa(encodeURIComponent(config!.text))}`;
        sessionStorage.setItem(key, '1');
        setDismissed(true);
    };

    return (
        <div className="notification-bar">
            <span className="notification-bar-text">{config!.text}</span>
            <button className="notification-bar-close" onClick={handleDismiss} aria-label="Close">✕</button>
        </div>
    );
}

export default function App() {
    const { token, setAuth, user } = useAuthStore();
    const [loading, setLoading] = useState(true);

    useEffect(() => {
        // Initialize theme on app mount (ensures login page gets correct theme)
        const savedTheme = localStorage.getItem('theme') || 'dark';
        document.documentElement.setAttribute('data-theme', savedTheme);

        if (token && !user) {
            authApi.me()
                .then((u) => setAuth(u, token))
                .catch(() => useAuthStore.getState().logout())
                .finally(() => setLoading(false));
        } else {
            setLoading(false);
        }
    }, []);


    if (loading) {
        return (
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '100vh', color: 'var(--text-tertiary)' }}>
                加载中...
            </div>
        );
    }

    return (
        <>
            <NotificationBar />
            <Routes>
                <Route path="/login" element={<Login />} />
                <Route path="/setup-company" element={<CompanySetup />} />
                <Route path="/" element={<ProtectedRoute><Layout /></ProtectedRoute>}>
                    <Route index element={<Navigate to="/plaza" replace />} />
                    <Route path="overview" element={<Overview />} />
                    <Route path="dashboard" element={<Dashboard />} />
                    <Route path="analytics" element={<Analytics />} />
                    <Route path="plaza" element={<Plaza />} />
                    <Route path="agents/new" element={<AgentCreate />} />
                    <Route path="agents/:id" element={<AgentDetail />} />
                    <Route path="agents/:id/chat" element={<AgentChatRedirect />} />
                    <Route path="sessions" element={<Sessions />} />
                    <Route path="approvals" element={<Approvals />} />
                    <Route path="comms" element={<Comms />} />
                    <Route path="workflows" element={<Workflows />} />
                    <Route path="scheduler" element={<Scheduler />} />
                    <Route path="channels" element={<Channels />} />
                    <Route path="skills" element={<Skills />} />
                    <Route path="hands" element={<Hands />} />
                    <Route path="control" element={<Control />} />
                    <Route path="runtime" element={<Runtime />} />
                    <Route path="settings" element={<Settings />} />
                    <Route path="logs" element={<Logs />} />
                    <Route path="wizard" element={<Navigate to="/agents/new" replace />} />
                    <Route path="messages" element={<Messages />} />
                    <Route path="enterprise" element={<EnterpriseSettings />} />
                    <Route path="invitations" element={<InvitationCodes />} />
                    <Route path="admin/platform-settings" element={<AdminCompanies />} />
                </Route>
            </Routes>
        </>
    );
}
