import { useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { request } from '../services/api';

type WorkflowStep = {
    name: string;
    agent_name: string;
    mode: string;
    prompt: string;
};

type WorkflowForm = {
    id?: string;
    name: string;
    description: string;
    steps: WorkflowStep[];
};

const emptyStep = (): WorkflowStep => ({
    name: '',
    agent_name: '',
    mode: 'sequential',
    prompt: '{{input}}',
});

const emptyWorkflow = (): WorkflowForm => ({
    name: '',
    description: '',
    steps: [emptyStep()],
});

function WorkflowEditor({
    form,
    setForm,
    onClose,
    onSubmit,
    saving,
}: {
    form: WorkflowForm;
    setForm: (form: WorkflowForm) => void;
    onClose: () => void;
    onSubmit: () => void;
    saving: boolean;
}) {
    return (
        <div style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.55)', zIndex: 1200, display: 'flex', alignItems: 'center', justifyContent: 'center', padding: '24px' }} onClick={onClose}>
            <div className="card" style={{ width: 'min(860px, 100%)', maxHeight: '88vh', overflow: 'auto', padding: '20px', boxShadow: 'var(--shadow-lg)' }} onClick={(event) => event.stopPropagation()}>
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
                    <div>
                        <div style={{ fontSize: '18px', fontWeight: 700 }}>{form.id ? 'Edit Workflow' : 'Create Workflow'}</div>
                        <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginTop: '4px' }}>沿用静态版的步骤式定义，但用当前前端的卡片布局表达。</div>
                    </div>
                    <button className="btn btn-ghost" onClick={onClose}>关闭</button>
                </div>

                <div className="form-group">
                    <label className="form-label">Workflow Name</label>
                    <input className="form-input" value={form.name} onChange={(event) => setForm({ ...form, name: event.target.value })} />
                </div>

                <div className="form-group">
                    <label className="form-label">Description</label>
                    <textarea className="form-textarea" value={form.description} onChange={(event) => setForm({ ...form, description: event.target.value })} />
                </div>

                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '12px' }}>
                    <div style={{ fontSize: '14px', fontWeight: 600 }}>Steps</div>
                    <button className="btn btn-secondary" onClick={() => setForm({ ...form, steps: [...form.steps, emptyStep()] })}>Add Step</button>
                </div>

                <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
                    {form.steps.map((step, index) => (
                        <div key={`${step.name}-${index}`} style={{ border: '1px solid var(--border-subtle)', borderRadius: '12px', padding: '14px', background: 'var(--bg-secondary)' }}>
                            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '12px' }}>
                                <div style={{ fontSize: '13px', fontWeight: 600 }}>Step {index + 1}</div>
                                {form.steps.length > 1 && (
                                    <button
                                        className="btn btn-ghost"
                                        style={{ color: 'var(--error)' }}
                                        onClick={() => setForm({ ...form, steps: form.steps.filter((_, stepIndex) => stepIndex !== index) })}
                                    >
                                        Remove
                                    </button>
                                )}
                            </div>
                            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 180px', gap: '12px', marginBottom: '12px' }}>
                                <div>
                                    <label className="form-label">Step Name</label>
                                    <input className="form-input" value={step.name} onChange={(event) => {
                                        const steps = [...form.steps];
                                        steps[index] = { ...steps[index], name: event.target.value };
                                        setForm({ ...form, steps });
                                    }} />
                                </div>
                                <div>
                                    <label className="form-label">Agent Name</label>
                                    <input className="form-input" value={step.agent_name} onChange={(event) => {
                                        const steps = [...form.steps];
                                        steps[index] = { ...steps[index], agent_name: event.target.value };
                                        setForm({ ...form, steps });
                                    }} />
                                </div>
                                <div>
                                    <label className="form-label">Mode</label>
                                    <select className="form-input" value={step.mode} onChange={(event) => {
                                        const steps = [...form.steps];
                                        steps[index] = { ...steps[index], mode: event.target.value };
                                        setForm({ ...form, steps });
                                    }}>
                                        <option value="sequential">sequential</option>
                                        <option value="parallel">parallel</option>
                                        <option value="conditional">conditional</option>
                                    </select>
                                </div>
                            </div>
                            <div>
                                <label className="form-label">Prompt Template</label>
                                <textarea className="form-textarea" value={step.prompt} onChange={(event) => {
                                    const steps = [...form.steps];
                                    steps[index] = { ...steps[index], prompt: event.target.value };
                                    setForm({ ...form, steps });
                                }} />
                            </div>
                        </div>
                    ))}
                </div>

                <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '8px', marginTop: '18px' }}>
                    <button className="btn btn-secondary" onClick={onClose}>Cancel</button>
                    <button className="btn btn-primary" onClick={onSubmit} disabled={saving}>{saving ? 'Saving...' : 'Save Workflow'}</button>
                </div>
            </div>
        </div>
    );
}

export default function Workflows() {
    const queryClient = useQueryClient();
    const [editorOpen, setEditorOpen] = useState(false);
    const [workflowForm, setWorkflowForm] = useState<WorkflowForm>(emptyWorkflow());
    const [runner, setRunner] = useState<{ id: string; name: string } | null>(null);
    const [runInput, setRunInput] = useState('');
    const [runOutput, setRunOutput] = useState('');
    const [runHistory, setRunHistory] = useState<any[] | null>(null);

    const { data, isLoading, error } = useQuery({
        queryKey: ['workflows-console'],
        queryFn: () => request<any[]>('/workflows'),
        refetchInterval: 15000,
    });

    const workflowList = useMemo(() => Array.isArray(data) ? data : [], [data]);

    const saveMutation = useMutation({
        mutationFn: async (form: WorkflowForm) => {
            const payload = {
                name: form.name,
                description: form.description,
                steps: form.steps.map((step) => ({
                    name: step.name || 'step',
                    agent_name: step.agent_name,
                    mode: step.mode,
                    prompt: step.prompt || '{{input}}',
                })),
            };

            if (form.id) {
                return request(`/workflows/${form.id}`, { method: 'PUT', body: JSON.stringify(payload) });
            }

            return request('/workflows', { method: 'POST', body: JSON.stringify(payload) });
        },
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: ['workflows-console'] });
            setEditorOpen(false);
            setWorkflowForm(emptyWorkflow());
        },
    });

    const deleteMutation = useMutation({
        mutationFn: (workflowId: string) => request(`/workflows/${workflowId}`, { method: 'DELETE' }),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: ['workflows-console'] });
        },
    });

    const runMutation = useMutation({
        mutationFn: async ({ workflowId, input }: { workflowId: string; input: string }) => {
            return request<any>(`/workflows/${workflowId}/run`, { method: 'POST', body: JSON.stringify({ input }) });
        },
        onSuccess: (result) => {
            setRunOutput(result.output || JSON.stringify(result, null, 2));
            queryClient.invalidateQueries({ queryKey: ['workflows-console'] });
        },
    });

    return (
        <div>
            <div className="page-header">
                <div>
                    <h1 className="page-title">Workflows</h1>
                    <div className="page-subtitle">对应静态版 Workflows 页面，保留定义、运行和查看运行历史三类核心动作。</div>
                </div>
                <button
                    className="btn btn-primary"
                    onClick={() => {
                        setWorkflowForm(emptyWorkflow());
                        setEditorOpen(true);
                    }}
                >
                    New Workflow
                </button>
            </div>

            <div className="card" style={{ padding: '0', overflow: 'hidden' }}>
                <div style={{ display: 'grid', gridTemplateColumns: '1.4fr 0.8fr 0.8fr 1fr', gap: '12px', padding: '12px 16px', borderBottom: '1px solid var(--border-subtle)', fontSize: '11px', textTransform: 'uppercase', color: 'var(--text-tertiary)', letterSpacing: '0.05em' }}>
                    <span>Workflow</span>
                    <span>Steps</span>
                    <span>Status</span>
                    <span style={{ textAlign: 'right' }}>Actions</span>
                </div>
                {isLoading && <div style={{ padding: '24px 16px', color: 'var(--text-tertiary)' }}>加载 workflows 中...</div>}
                {!isLoading && error && <div style={{ padding: '24px 16px', color: 'var(--error)' }}>{error instanceof Error ? error.message : 'Failed to load workflows'}</div>}
                {!isLoading && !error && workflowList.length === 0 && (
                    <div style={{ padding: '32px 16px', color: 'var(--text-tertiary)', textAlign: 'center' }}>还没有 workflow，先创建一个试试。</div>
                )}
                {!isLoading && !error && workflowList.map((workflow: any) => (
                    <div key={workflow.id} style={{ display: 'grid', gridTemplateColumns: '1.4fr 0.8fr 0.8fr 1fr', gap: '12px', alignItems: 'center', padding: '16px', borderBottom: '1px solid var(--border-subtle)' }}>
                        <div style={{ minWidth: 0 }}>
                            <div style={{ fontSize: '13px', fontWeight: 600 }}>{workflow.name}</div>
                            <div style={{ fontSize: '12px', color: 'var(--text-secondary)', marginTop: '6px', lineHeight: 1.6 }}>
                                {workflow.description || 'No description'}
                            </div>
                        </div>
                        <div style={{ fontSize: '13px', color: 'var(--text-secondary)' }}>{workflow.steps || workflow.step_count || workflow.steps_count || 0}</div>
                        <div>
                            <span className={`badge ${workflow.enabled === false ? 'badge-warning' : 'badge-success'}`}>
                                {workflow.enabled === false ? 'disabled' : 'ready'}
                            </span>
                        </div>
                        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '8px' }}>
                            <button
                                className="btn btn-ghost"
                                onClick={async () => {
                                    const full = await request<any>(`/workflows/${workflow.id}`);
                                    setWorkflowForm({
                                        id: workflow.id,
                                        name: full.name || '',
                                        description: full.description || '',
                                        steps: Array.isArray(full.steps) && full.steps.length > 0
                                            ? full.steps.map((step: any) => ({
                                                name: step.name || '',
                                                agent_name: step.agent_name || step.agent?.name || '',
                                                mode: step.mode || 'sequential',
                                                prompt: step.prompt_template || step.prompt || '{{input}}',
                                            }))
                                            : [emptyStep()],
                                    });
                                    setEditorOpen(true);
                                }}
                            >
                                Edit
                            </button>
                            <button
                                className="btn btn-ghost"
                                onClick={async () => {
                                    const runs = await request<any[]>(`/workflows/${workflow.id}/runs`);
                                    setRunner({ id: workflow.id, name: workflow.name });
                                    setRunHistory(Array.isArray(runs) ? runs : []);
                                    setRunOutput('');
                                }}
                            >
                                Runs
                            </button>
                            <button
                                className="btn btn-secondary"
                                onClick={() => {
                                    setRunner({ id: workflow.id, name: workflow.name });
                                    setRunInput('');
                                    setRunOutput('');
                                    setRunHistory(null);
                                }}
                            >
                                Run
                            </button>
                            <button
                                className="btn btn-ghost"
                                style={{ color: 'var(--error)' }}
                                disabled={deleteMutation.isPending}
                                onClick={() => {
                                    if (window.confirm(`确定删除 workflow "${workflow.name}" 吗？`)) {
                                        deleteMutation.mutate(workflow.id);
                                    }
                                }}
                            >
                                Delete
                            </button>
                        </div>
                    </div>
                ))}
            </div>

            {editorOpen && (
                <WorkflowEditor
                    form={workflowForm}
                    setForm={setWorkflowForm}
                    onClose={() => setEditorOpen(false)}
                    onSubmit={() => saveMutation.mutate(workflowForm)}
                    saving={saveMutation.isPending}
                />
            )}

            {runner && (
                <div style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.55)', zIndex: 1200, display: 'flex', alignItems: 'center', justifyContent: 'center', padding: '24px' }} onClick={() => setRunner(null)}>
                    <div className="card" style={{ width: 'min(840px, 100%)', maxHeight: '88vh', overflow: 'auto', padding: '20px', boxShadow: 'var(--shadow-lg)' }} onClick={(event) => event.stopPropagation()}>
                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
                            <div>
                                <div style={{ fontSize: '18px', fontWeight: 700 }}>{runner.name}</div>
                                <div style={{ fontSize: '12px', color: 'var(--text-tertiary)', marginTop: '4px' }}>{runHistory ? 'Run history' : 'Execute workflow with custom input'}</div>
                            </div>
                            <button className="btn btn-ghost" onClick={() => setRunner(null)}>关闭</button>
                        </div>

                        {!runHistory && (
                            <>
                                <div className="form-group">
                                    <label className="form-label">Input</label>
                                    <textarea className="form-textarea" value={runInput} onChange={(event) => setRunInput(event.target.value)} placeholder="输入 workflow 的运行内容..." />
                                </div>
                                <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: '16px' }}>
                                    <button className="btn btn-primary" onClick={() => runMutation.mutate({ workflowId: runner.id, input: runInput })} disabled={runMutation.isPending}>
                                        {runMutation.isPending ? 'Running...' : 'Run Workflow'}
                                    </button>
                                </div>
                                <div style={{ border: '1px solid var(--border-subtle)', borderRadius: '12px', background: 'var(--bg-secondary)', padding: '14px' }}>
                                    <div style={{ fontSize: '13px', fontWeight: 600, marginBottom: '10px' }}>Output</div>
                                    <pre style={{ margin: 0, whiteSpace: 'pre-wrap', wordBreak: 'break-word', fontFamily: 'var(--font-mono)', fontSize: '12px', color: 'var(--text-secondary)' }}>
                                        {runOutput || '尚未执行。'}
                                    </pre>
                                </div>
                            </>
                        )}

                        {runHistory && (
                            <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                                {runHistory.length === 0 && <div style={{ color: 'var(--text-tertiary)' }}>No run history.</div>}
                                {runHistory.map((run: any) => (
                                    <div key={run.run_id || run.id} style={{ border: '1px solid var(--border-subtle)', borderRadius: '12px', padding: '12px 14px', background: 'var(--bg-secondary)' }}>
                                        <div style={{ display: 'flex', justifyContent: 'space-between', gap: '12px', marginBottom: '8px' }}>
                                            <span className={`badge ${run.status === 'completed' ? 'badge-success' : run.status === 'failed' ? 'badge-error' : 'badge-info'}`}>{run.status || 'unknown'}</span>
                                            <span style={{ fontSize: '11px', color: 'var(--text-tertiary)' }}>{run.created_at ? new Date(run.created_at).toLocaleString() : ''}</span>
                                        </div>
                                        <div style={{ fontSize: '12px', color: 'var(--text-secondary)', whiteSpace: 'pre-wrap' }}>
                                            {run.output || run.result || JSON.stringify(run, null, 2)}
                                        </div>
                                    </div>
                                ))}
                            </div>
                        )}
                    </div>
                </div>
            )}
        </div>
    );
}
