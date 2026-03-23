/** API service layer */

import type { Agent, TokenResponse, User, Task, ChatMessage } from '../types';

const API_BASE = '/api';

type RawAgent = Partial<Agent> & Record<string, any>;

function buildPath(path: string, params?: Record<string, string | null | undefined>): string {
    if (!params) return path;
    const search = new URLSearchParams();
    for (const [key, value] of Object.entries(params)) {
        if (value != null && value !== '') {
            search.set(key, value);
        }
    }
    const query = search.toString();
    return query ? `${path}?${query}` : path;
}

function normalizeAgentStatus(status: unknown): Agent['status'] {
    const value = String(status || '').trim().toLowerCase();
    if (value === 'running' || value === 'idle' || value === 'stopped' || value === 'error' || value === 'creating') {
        return value;
    }
    if (value === 'failed' || value === 'crashed') {
        return 'error';
    }
    return 'stopped';
}

function normalizeAgent(agent: RawAgent): Agent & Record<string, any> {
    const roleDescription =
        typeof agent.role_description === 'string' ? agent.role_description :
            typeof agent.description === 'string' ? agent.description :
                typeof agent.profile?.description === 'string' ? agent.profile.description :
                    '';

    const heartbeatIntervalMinutes =
        typeof agent.heartbeat_interval_minutes === 'number' ? agent.heartbeat_interval_minutes :
            typeof agent.heartbeat_interval_secs === 'number' ? Math.max(1, Math.round(agent.heartbeat_interval_secs / 60)) :
                0;

    return {
        ...agent,
        id: String(agent.id || ''),
        name: agent.name || 'Untitled Agent',
        avatar_url: agent.avatar_url || agent.identity?.avatar_url,
        role_description: roleDescription,
        bio: typeof agent.bio === 'string' ? agent.bio : '',
        status: normalizeAgentStatus(agent.status ?? agent.state),
        creator_id: agent.creator_id || agent.creator_user_id || '',
        primary_model_id: agent.primary_model_id,
        fallback_model_id: agent.fallback_model_id,
        autonomy_policy: agent.autonomy_policy || {},
        tokens_used_today: Number(agent.tokens_used_today || 0),
        tokens_used_month: Number(agent.tokens_used_month || 0),
        max_tokens_per_day: agent.max_tokens_per_day,
        max_tokens_per_month: agent.max_tokens_per_month,
        heartbeat_enabled: Boolean(agent.heartbeat_enabled),
        heartbeat_interval_minutes: heartbeatIntervalMinutes,
        heartbeat_active_hours: typeof agent.heartbeat_active_hours === 'string' ? agent.heartbeat_active_hours : '',
        last_heartbeat_at: agent.last_heartbeat_at,
        timezone: agent.timezone || agent.effective_timezone,
        context_window_size: typeof agent.context_window_size === 'number' ? agent.context_window_size : undefined,
        agent_type: agent.agent_type,
        openclaw_last_seen: agent.openclaw_last_seen,
        created_at: agent.created_at || '',
        last_active_at: agent.last_active_at || agent.last_active,
        state: agent.state || normalizeAgentStatus(agent.status ?? agent.state),
        description: agent.description || roleDescription,
        creator_user_id: agent.creator_user_id || agent.creator_id || '',
        access_level: agent.access_level,
    };
}

export async function request<T>(url: string, options: RequestInit = {}): Promise<T> {
    const token = localStorage.getItem('token');
    const headers: Record<string, string> = {
        'Content-Type': 'application/json',
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
    };

    const res = await fetch(`${API_BASE}${url}`, { ...options, headers });

    if (!res.ok) {
        // Auto-logout on expired/invalid token (but not on auth endpoints — let them show errors)
        const isAuthEndpoint = url.startsWith('/auth/login') || url.startsWith('/auth/register');
        if (res.status === 401 && !isAuthEndpoint) {
            localStorage.removeItem('token');
            localStorage.removeItem('user');
            window.location.href = '/login';
            throw new Error('Session expired');
        }
        const error = await res.json().catch(() => ({ detail: 'Request failed' }));
        // Pydantic validation errors return detail as an array of objects
        const fieldLabels: Record<string, string> = {
            name: '名称',
            role_description: '角色描述',
            agent_type: '智能体类型',
            primary_model_id: '主模型',
            max_tokens_per_day: '每日 Token 上限',
            max_tokens_per_month: '每月 Token 上限',
        };
        let message = '';
        if (Array.isArray(error.detail)) {
            message = error.detail
                .map((e: any) => {
                    const field = e.loc?.slice(-1)[0] || '';
                    const label = fieldLabels[field] || field;
                    return label ? `${label}: ${e.msg}` : e.msg;
                })
                .join('; ');
        } else {
            message = error.detail || error.error || error.message || `HTTP ${res.status}`;
        }
        throw new Error(message);
    }

    if (res.status === 204) return undefined as T;
    return res.json();
}

async function uploadFile(url: string, file: File, extraFields?: Record<string, string>): Promise<any> {
    const token = localStorage.getItem('token');
    const formData = new FormData();
    formData.append('file', file);
    if (extraFields) {
        for (const [k, v] of Object.entries(extraFields)) {
            formData.append(k, v);
        }
    }
    const res = await fetch(`${API_BASE}${url}`, {
        method: 'POST',
        headers: token ? { Authorization: `Bearer ${token}` } : {},
        body: formData,
    });
    if (!res.ok) {
        const error = await res.json().catch(() => ({ detail: 'Upload failed' }));
        throw new Error(error.detail || `HTTP ${res.status}`);
    }
    return res.json();
}

async function uploadBinaryFile(url: string, file: File): Promise<any> {
    const token = localStorage.getItem('token');
    const res = await fetch(`${API_BASE}${url}`, {
        method: 'POST',
        headers: {
            ...(token ? { Authorization: `Bearer ${token}` } : {}),
            'Content-Type': file.type || 'application/octet-stream',
            'X-Filename': file.name,
        },
        body: file,
    });
    if (!res.ok) {
        const error = await res.json().catch(() => ({ detail: 'Upload failed' }));
        throw new Error(error.detail || error.error || `HTTP ${res.status}`);
    }
    return res.json();
}

// Upload with progress tracking via XMLHttpRequest.
// Returns { promise, abort } — call abort() to cancel the upload.
// Progress callback: 0-100 = upload phase, 101 = processing phase (server is parsing the file).
export function uploadFileWithProgress(
    url: string,
    file: File,
    onProgress?: (percent: number) => void,
    extraFields?: Record<string, string>,
    timeoutMs: number = 120_000,
): { promise: Promise<any>; abort: () => void } {
    const xhr = new XMLHttpRequest();
    const promise = new Promise<any>((resolve, reject) => {
        const token = localStorage.getItem('token');
        const formData = new FormData();
        formData.append('file', file);
        if (extraFields) {
            for (const [k, v] of Object.entries(extraFields)) {
                formData.append(k, v);
            }
        }
        xhr.open('POST', `${API_BASE}${url}`);
        if (token) xhr.setRequestHeader('Authorization', `Bearer ${token}`);

        // Upload phase: 0-100%
        xhr.upload.onprogress = (e) => {
            if (e.lengthComputable && onProgress) {
                onProgress(Math.round((e.loaded / e.total) * 100));
            }
        };
        // Upload bytes finished → enter processing phase
        xhr.upload.onload = () => {
            if (onProgress) onProgress(101); // 101 = "processing" sentinel
        };

        xhr.onload = () => {
            if (xhr.status >= 200 && xhr.status < 300) {
                try { resolve(JSON.parse(xhr.responseText)); } catch { resolve(undefined); }
            } else {
                try {
                    const err = JSON.parse(xhr.responseText);
                    reject(new Error(err.detail || `HTTP ${xhr.status}`));
                } catch { reject(new Error(`HTTP ${xhr.status}`)); }
            }
        };
        xhr.onerror = () => reject(new Error('Network error'));
        xhr.ontimeout = () => reject(new Error('Upload timed out'));
        xhr.onabort = () => reject(new Error('Upload cancelled'));
        xhr.timeout = timeoutMs;
        xhr.send(formData);
    });
    return { promise, abort: () => xhr.abort() };
}

export function uploadBinaryFileWithProgress(
    url: string,
    file: File,
    onProgress?: (percent: number) => void,
    timeoutMs: number = 120_000,
): { promise: Promise<any>; abort: () => void } {
    const xhr = new XMLHttpRequest();
    const promise = new Promise<any>((resolve, reject) => {
        const token = localStorage.getItem('token');
        xhr.open('POST', `${API_BASE}${url}`);
        if (token) xhr.setRequestHeader('Authorization', `Bearer ${token}`);
        xhr.setRequestHeader('Content-Type', file.type || 'application/octet-stream');
        xhr.setRequestHeader('X-Filename', file.name);

        xhr.upload.onprogress = (e) => {
            if (e.lengthComputable && onProgress) {
                onProgress(Math.round((e.loaded / e.total) * 100));
            }
        };
        xhr.upload.onload = () => {
            if (onProgress) onProgress(101);
        };

        xhr.onload = () => {
            if (xhr.status >= 200 && xhr.status < 300) {
                try { resolve(JSON.parse(xhr.responseText)); } catch { resolve(undefined); }
            } else {
                try {
                    const err = JSON.parse(xhr.responseText);
                    reject(new Error(err.detail || err.error || `HTTP ${xhr.status}`));
                } catch { reject(new Error(`HTTP ${xhr.status}`)); }
            }
        };
        xhr.onerror = () => reject(new Error('Network error'));
        xhr.ontimeout = () => reject(new Error('Upload timed out'));
        xhr.onabort = () => reject(new Error('Upload cancelled'));
        xhr.timeout = timeoutMs;
        xhr.send(file);
    });
    return { promise, abort: () => xhr.abort() };
}

// ─── Auth ─────────────────────────────────────────────
export const authApi = {
    register: (data: { username: string; email: string; password: string; display_name: string }) =>
        request<TokenResponse>('/auth/register', { method: 'POST', body: JSON.stringify(data) }),

    login: (data: { username: string; password: string }) =>
        request<TokenResponse>('/auth/login', { method: 'POST', body: JSON.stringify(data) }),

    me: () => request<User>('/auth/me'),

    updateMe: (data: Partial<User>) =>
        request<User>('/auth/me', { method: 'PATCH', body: JSON.stringify(data) }),
};

// ─── Tenants ──────────────────────────────────────────
export const tenantApi = {
    selfCreate: (data: { name: string }) =>
        request<any>('/tenants/self-create', { method: 'POST', body: JSON.stringify(data) }),

    join: (invitationCode: string) =>
        request<any>('/tenants/join', { method: 'POST', body: JSON.stringify({ invitation_code: invitationCode }) }),

    registrationConfig: () =>
        request<{ allow_self_create_company: boolean }>('/tenants/registration-config'),
};

export const adminApi = {
    listCompanies: () =>
        request<any[]>('/admin/companies'),

    createCompany: (data: { name: string }) =>
        request<any>('/admin/companies', { method: 'POST', body: JSON.stringify(data) }),

    toggleCompany: (id: string) =>
        request<any>(`/admin/companies/${id}/toggle`, { method: 'PUT' }),

    getPlatformSettings: () =>
        request<any>('/admin/platform-settings'),

    updatePlatformSettings: (data: any) =>
        request<any>('/admin/platform-settings', { method: 'PUT', body: JSON.stringify(data) }),
};

// ─── Agents ───────────────────────────────────────────
export const agentApi = {
    list: (tenantId?: string) =>
        request<RawAgent[]>(buildPath('/agents', { tenant_id: tenantId }))
            .then(items => Array.isArray(items) ? items.map(normalizeAgent) : []),

    get: (id: string) =>
        request<RawAgent>(`/agents/${id}`).then(normalizeAgent),

    create: (data: any) =>
        request<any>('/agents', { method: 'POST', body: JSON.stringify(data) }),

    update: (id: string, data: Partial<Agent>) =>
        request<any>(`/agents/${id}`, { method: 'PATCH', body: JSON.stringify(data) })
            .then((result) => result && typeof result === 'object' && 'id' in result ? normalizeAgent(result as RawAgent) : result),

    delete: (id: string) =>
        request<void>(`/agents/${id}`, { method: 'DELETE' }),

    start: (id: string) =>
        request<Agent>(`/agents/${id}/start`, { method: 'POST' }),

    stop: (id: string) =>
        request<Agent>(`/agents/${id}/stop`, { method: 'POST' }),

    metrics: (id: string) =>
        request<any>(`/agents/${id}/metrics`),

    collaborators: (id: string) =>
        Promise.reject(new Error('Current backend does not expose agent collaborator APIs yet.')),

    templates: () =>
        request<any>('/templates').then(r => r.templates || []),

    // OpenClaw gateway
    generateApiKey: (id: string) =>
        Promise.reject(new Error('Current backend does not expose OpenClaw gateway APIs yet.')),

    gatewayMessages: (id: string) =>
        Promise.reject(new Error('Current backend does not expose OpenClaw gateway APIs yet.')),
};

// ─── Tasks ────────────────────────────────────────────
function unsupportedTaskApiError(): Error {
    return new Error('Current backend does not expose agent task APIs yet.');
}

export const taskApi = {
    list: (agentId: string, status?: string, type?: string) =>
        Promise.resolve([] as Task[]),

    create: (agentId: string, data: any) =>
        Promise.reject(unsupportedTaskApiError()),

    update: (agentId: string, taskId: string, data: Partial<Task>) =>
        Promise.reject(unsupportedTaskApiError()),

    getLogs: (agentId: string, taskId: string) =>
        Promise.resolve([] as { id: string; task_id: string; content: string; created_at: string }[]),

    trigger: (agentId: string, taskId: string) =>
        Promise.reject(unsupportedTaskApiError()),
};

// ─── Files ────────────────────────────────────────────
export const fileApi = {
    list: (agentId: string, path: string = '') =>
        request<any[]>(`/agents/${agentId}/files?path=${encodeURIComponent(path)}`),

    read: (agentId: string, path: string) =>
        request<{ path: string; content: string }>(`/agents/${agentId}/files/content?path=${encodeURIComponent(path)}`),

    write: (agentId: string, path: string, content: string) =>
        request(`/agents/${agentId}/files/content?path=${encodeURIComponent(path)}`, {
            method: 'PUT',
            body: JSON.stringify({ content }),
        }),

    delete: (agentId: string, path: string) =>
        request(`/agents/${agentId}/files/content?path=${encodeURIComponent(path)}`, {
            method: 'DELETE',
        }),

    upload: (agentId: string, file: File, path: string = 'workspace/knowledge_base', onProgress?: (pct: number) => void) =>
        onProgress
            ? uploadBinaryFileWithProgress(`/agents/${agentId}/upload?path=${encodeURIComponent(path)}`, file, onProgress).promise
            : uploadBinaryFile(`/agents/${agentId}/upload?path=${encodeURIComponent(path)}`, file),

    importSkill: (agentId: string, skillId: string) =>
        request<any>(`/agents/${agentId}/files/import-skill`, {
            method: 'POST',
            body: JSON.stringify({ skill_id: skillId }),
        }),

    downloadUrl: (agentId: string, path: string) => {
        const token = localStorage.getItem('token');
        return `${API_BASE}/agents/${agentId}/files/download?path=${encodeURIComponent(path)}&token=${token}`;
    },
};

// ─── Channel Config ───────────────────────────────────
export const channelApi = {
    get: (_agentId: string) =>
        request<any>('/channels').then(r => (r.channels || []).find((c: any) => c.name === 'feishu') || null).catch(() => null),

    create: (_agentId: string, data: any) =>
        request<any>(`/channels/${data.channel_type || 'feishu'}/configure`, { method: 'POST', body: JSON.stringify({ fields: data }) }),

    update: (_agentId: string, data: any) =>
        request<any>(`/channels/${data.channel_type || 'feishu'}/configure`, { method: 'POST', body: JSON.stringify({ fields: data }) }),

    delete: (_agentId: string) =>
        request<void>('/channels/feishu/configure', { method: 'DELETE' }),

    webhookUrl: (agentId: string) =>
        Promise.resolve({ webhook_url: '' }),
};

// ─── Enterprise ───────────────────────────────────────
export const enterpriseApi = {
    llmModels: () => {
        const tid = localStorage.getItem('current_tenant_id');
        return request<any[]>(`/enterprise/llm-models${tid ? `?tenant_id=${tid}` : ''}`);
    },
    templates: () => request<any>('/templates').then(r => r.templates || []),

    // Enterprise Knowledge Base
    kbFiles: (path: string = '') =>
        Promise.reject(new Error('Current backend does not expose enterprise knowledge base APIs yet.')),

    kbUpload: (file: File, subPath: string = '') =>
        Promise.reject(new Error('Current backend does not expose enterprise knowledge base APIs yet.')),

    kbRead: (path: string) =>
        Promise.reject(new Error('Current backend does not expose enterprise knowledge base APIs yet.')),

    kbWrite: (path: string, content: string) =>
        Promise.reject(new Error('Current backend does not expose enterprise knowledge base APIs yet.')),

    kbDelete: (path: string) =>
        Promise.reject(new Error('Current backend does not expose enterprise knowledge base APIs yet.')),
};

// ─── Activity Logs ────────────────────────────────────
export const activityApi = {
    list: (agentId: string, limit = 50) =>
        request<any[]>(`/agents/${agentId}/activity?limit=${limit}`),
};

// ─── Messages ─────────────────────────────────────────
export const messageApi = {
    inbox: (limit = 50) =>
        request<any[]>(`/messages/inbox?limit=${limit}`),

    unreadCount: () =>
        request<{ unread_count: number }>('/messages/unread-count'),

    markRead: (messageId: string) =>
        request<void>(`/messages/${messageId}/read`, { method: 'PUT' }),

    markAllRead: () =>
        request<void>('/messages/read-all', { method: 'PUT' }),
};

// ─── Schedules ────────────────────────────────────────
export const scheduleApi = {
    list: (agentId: string) =>
        request<any>('/schedules').then(r => (r.schedules || []).filter((s: any) => s.agent_id === agentId)),

    create: (agentId: string, data: { name: string; instruction: string; cron_expr: string }) =>
        request<any>('/schedules', {
            method: 'POST',
            body: JSON.stringify({ name: data.name, cron: data.cron_expr, agent_id: agentId, message: data.instruction, enabled: true }),
        }),

    update: (agentId: string, scheduleId: string, data: any) =>
        request<any>(`/schedules/${scheduleId}`, {
            method: 'PUT',
            body: JSON.stringify({
                enabled: data.is_enabled,
                name: data.name,
                cron: data.cron_expr,
                agent_id: agentId,
                message: data.instruction,
            }),
        }),

    delete: (agentId: string, scheduleId: string) =>
        request<void>(`/schedules/${scheduleId}`, { method: 'DELETE' }),

    trigger: (agentId: string, scheduleId: string) =>
        request<any>(`/schedules/${scheduleId}/run`, { method: 'POST' }),

    history: (agentId: string, scheduleId: string) =>
        Promise.resolve([] as any[]),
};

// ─── Skills ───────────────────────────────────────────
export const skillApi = {
    list: () => request<any>('/skills').then(r => r.skills || []),
    get: (id: string) => request<any>('/skills').then(r => (r.skills || []).find((s: any) => s.name === id || s.id === id)),
    create: (data: any) =>
        request<any>('/skills/create', { method: 'POST', body: JSON.stringify(data) }),
    update: (id: string, data: any) =>
        request<any>(`/skills/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
    delete: (id: string) =>
        request<void>(`/skills/${id}`, { method: 'DELETE' }),
    // Path-based browse for FileBrowser
    browse: {
        list: (path: string) => request<any[]>(`/skills/browse/list?path=${encodeURIComponent(path)}`),
        read: (path: string) => request<{ content: string }>(`/skills/browse/read?path=${encodeURIComponent(path)}`),
        write: (path: string, content: string) =>
            request<any>('/skills/browse/write', { method: 'PUT', body: JSON.stringify({ path, content }) }),
        delete: (path: string) =>
            request<any>(`/skills/browse/delete?path=${encodeURIComponent(path)}`, { method: 'DELETE' }),
    },
    // ClawHub marketplace integration
    clawhub: {
        search: (q: string) => request<any>(`/clawhub/search?q=${encodeURIComponent(q)}`).then(r => r.items || []),
        detail: (slug: string) => request<any>(`/clawhub/skill/${slug}`),
        install: (slug: string) => request<any>('/clawhub/install', { method: 'POST', body: JSON.stringify({ slug }) }),
    },
    importFromUrl: (url: string) =>
        request<any>('/skills/import-from-url', { method: 'POST', body: JSON.stringify({ url }) }),
    previewUrl: (url: string) =>
        request<any>('/skills/import-from-url/preview', { method: 'POST', body: JSON.stringify({ url }) }),
    // Tenant-level settings
    settings: {
        getToken: () => request<{ configured: boolean; source: string; masked: string; clawhub_configured: boolean; clawhub_masked: string }>('/skills/settings/token'),
        setToken: (github_token: string) =>
            request<any>('/skills/settings/token', { method: 'PUT', body: JSON.stringify({ github_token }) }),
        setClawhubKey: (clawhub_key: string) =>
            request<any>('/skills/settings/token', { method: 'PUT', body: JSON.stringify({ clawhub_key }) }),
    },
    // Agent-level import (writes to agent workspace)
    agentImport: {
        fromClawhub: (agentId: string, slug: string) =>
            request<any>(`/agents/${agentId}/files/import-from-clawhub`, { method: 'POST', body: JSON.stringify({ slug }) }),
        fromUrl: (agentId: string, url: string) =>
            request<any>(`/agents/${agentId}/files/import-from-url`, { method: 'POST', body: JSON.stringify({ url }) }),
    },
};

// ─── Triggers (Aware Engine) ──────────────────────────
export const triggerApi = {
    list: (agentId: string) =>
        request<any[]>(`/triggers?agent_id=${encodeURIComponent(agentId)}`),

    update: (agentId: string, triggerId: string, data: any) =>
        request<any>(`/triggers/${triggerId}`, { method: 'PUT', body: JSON.stringify({ enabled: data.is_enabled ?? data.enabled }) }),

    delete: (agentId: string, triggerId: string) =>
        request<void>(`/triggers/${triggerId}`, { method: 'DELETE' }),
};
