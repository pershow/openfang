import type { Dispatch, ReactNode, SetStateAction } from 'react';

export type Scope = { scope_id: string; name: string; scope_type?: string; status?: string };

export type ScopeDetails = {
    observations: any[];
    guidelines: any[];
    journeys: any[];
    glossaryTerms: any[];
    contextVariables: any[];
    cannedResponses: any[];
    guidelineRelationships: any[];
    toolPolicies: any[];
    retrievers: any[];
    retrieverBindings: any[];
    releases: any[];
};

export type TabKey = 'observations' | 'guidelines' | 'journeys' | 'knowledge' | 'governance';

export type Feedback = { type: 'success' | 'error'; message: string };

export type Field = {
    key: string;
    label: string;
    kind: 'text' | 'textarea' | 'number' | 'checkbox' | 'select';
    options?: string[];
    placeholder?: string;
};

export type ResourceConfig = {
    key: string;
    title: string;
    listKey: keyof ScopeDetails;
    tab: TabKey;
    idField: string;
    createPath: string;
    itemPath: (id: string) => string;
    empty: Record<string, any>;
    fields: Field[];
    encode: (form: Record<string, any>, scopeId: string) => any;
    decode: (item: any) => Record<string, any>;
    summary: (item: any) => ReactNode;
    description?: string;
};

export type JourneyStateForm = {
    id: string;
    name: string;
    description: string;
    required_fields: string;
    guideline_actions: string;
};

export type JourneyTransitionForm = {
    id: string;
    from_state_id: string;
    to_state_id: string;
    transition_type: string;
    condition_config: string;
};

export type VariableValueForm = {
    key: string;
    data: string;
};

export type RelationshipForm = {
    from_guideline_id: string;
    to_guideline_id: string;
    relation_type: string;
};

export type ToolPolicyForm = {
    id: string;
    tool_name: string;
    skill_ref: string;
    observation_ref: string;
    journey_state_ref: string;
    guideline_ref: string;
    approval_mode: string;
    enabled: boolean;
};

export type RetrieverForm = {
    id: string;
    name: string;
    retriever_type: string;
    config_json: string;
    enabled: boolean;
};

export type BindingForm = {
    retriever_id: string;
    bind_type: string;
    bind_ref: string;
};

export type StateSetter<T> = Dispatch<SetStateAction<T>>;
