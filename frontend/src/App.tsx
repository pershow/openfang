import { Routes, Route, Navigate } from 'react-router-dom';
import { useAuthStore } from './stores';
import { lazy, Suspense, useEffect, useState, type ReactNode } from 'react';
import { authApi } from './services/api';

const Login = lazy(() => import('./pages/Login'));
const CompanySetup = lazy(() => import('./pages/CompanySetup'));
const Layout = lazy(() => import('./pages/Layout'));
const Dashboard = lazy(() => import('./pages/Dashboard'));
const Plaza = lazy(() => import('./pages/Plaza'));
const AgentDetail = lazy(() => import('./pages/AgentDetail'));
const AgentCreate = lazy(() => import('./pages/AgentCreate'));
const AgentChatRedirect = lazy(() => import('./pages/AgentChatRedirect'));
const Messages = lazy(() => import('./pages/Messages'));
const EnterpriseSettings = lazy(() => import('./pages/EnterpriseSettings'));
const InvitationCodes = lazy(() => import('./pages/InvitationCodes'));
const AdminCompanies = lazy(() => import('./pages/AdminCompanies'));
const Overview = lazy(() => import('./pages/Overview'));
const Analytics = lazy(() => import('./pages/Analytics'));
const Sessions = lazy(() => import('./pages/Sessions'));
const Approvals = lazy(() => import('./pages/Approvals'));
const Comms = lazy(() => import('./pages/Comms'));
const Workflows = lazy(() => import('./pages/Workflows'));
const Scheduler = lazy(() => import('./pages/Scheduler'));
const Channels = lazy(() => import('./pages/Channels'));
const Skills = lazy(() => import('./pages/Skills'));
const Hands = lazy(() => import('./pages/Hands'));
const Control = lazy(() => import('./pages/Control'));
const Runtime = lazy(() => import('./pages/Runtime'));
const Settings = lazy(() => import('./pages/Settings'));
const Logs = lazy(() => import('./pages/Logs'));

function ProtectedRoute({ children }: { children: ReactNode }) {
    const token = useAuthStore((s) => s.token);
    const user = useAuthStore((s) => s.user);
    if (!token) return <Navigate to="/login" replace />;
    // Force company setup for users without a tenant
    if (user && !user.tenant_id) return <Navigate to="/setup-company" replace />;
    return <>{children}</>;
}

function RouteFallback() {
    return (
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', minHeight: '50vh', color: 'var(--text-tertiary)' }}>
            加载中...
        </div>
    );
}

function withSuspense(element: ReactNode) {
    return <Suspense fallback={<RouteFallback />}>{element}</Suspense>;
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

        if ('serviceWorker' in navigator) {
            navigator.serviceWorker.getRegistrations()
                .then((registrations) => Promise.all(registrations.map((registration) => registration.unregister())))
                .catch(() => { });
        }

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
                <Route path="/login" element={withSuspense(<Login />)} />
                <Route path="/setup-company" element={withSuspense(<CompanySetup />)} />
                <Route path="/" element={<ProtectedRoute>{withSuspense(<Layout />)}</ProtectedRoute>}>
                    <Route index element={<Navigate to="/plaza" replace />} />
                    <Route path="overview" element={withSuspense(<Overview />)} />
                    <Route path="dashboard" element={withSuspense(<Dashboard />)} />
                    <Route path="analytics" element={withSuspense(<Analytics />)} />
                    <Route path="plaza" element={withSuspense(<Plaza />)} />
                    <Route path="agents/new" element={withSuspense(<AgentCreate />)} />
                    <Route path="agents/:id" element={withSuspense(<AgentDetail />)} />
                    <Route path="agents/:id/chat" element={withSuspense(<AgentChatRedirect />)} />
                    <Route path="sessions" element={withSuspense(<Sessions />)} />
                    <Route path="approvals" element={withSuspense(<Approvals />)} />
                    <Route path="comms" element={withSuspense(<Comms />)} />
                    <Route path="workflows" element={withSuspense(<Workflows />)} />
                    <Route path="scheduler" element={withSuspense(<Scheduler />)} />
                    <Route path="channels" element={withSuspense(<Channels />)} />
                    <Route path="skills" element={withSuspense(<Skills />)} />
                    <Route path="hands" element={withSuspense(<Hands />)} />
                    <Route path="control" element={withSuspense(<Control />)} />
                    <Route path="runtime" element={withSuspense(<Runtime />)} />
                    <Route path="settings" element={withSuspense(<Settings />)} />
                    <Route path="logs" element={withSuspense(<Logs />)} />
                    <Route path="wizard" element={<Navigate to="/agents/new" replace />} />
                    <Route path="messages" element={withSuspense(<Messages />)} />
                    <Route path="enterprise" element={withSuspense(<EnterpriseSettings />)} />
                    <Route path="invitations" element={withSuspense(<InvitationCodes />)} />
                    <Route path="admin/platform-settings" element={withSuspense(<AdminCompanies />)} />
                </Route>
            </Routes>
        </>
    );
}
