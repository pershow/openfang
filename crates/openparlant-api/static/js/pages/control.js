// OpenParlant Control Plane — Policies / Journeys / Knowledge / Releases / Debug
'use strict';

function controlPage() {
  return {
    // ── Tab routing ──────────────────────────────────────────────────────────
    tab: 'observations',  // 'debug' | 'observations' | 'guidelines' | 'journeys' | 'knowledge' | 'toolgate' | 'releases' | 'handoff'

    // ── Scope selection ──────────────────────────────────────────────────────
    scopes: [],
    selectedScope: '',
    newScopeName: '',
    creatingScope: false,

    // ── Debug / compile-turn ─────────────────────────────────────────────────
    debugAgentId: '',
    debugSessionId: '',
    debugMessage: 'Hello, what can you help me with?',
    debugResult: null,
    debugLoading: false,
    debugError: '',

    // ── Observations ─────────────────────────────────────────────────────────
    observations: [],
    obsLoading: false,
    obsForm: { name: '', matcher_type: 'keyword', priority: 0, enabled: true, matcher_config: '{}' },
    obsCreating: false,

    // ── Guidelines ───────────────────────────────────────────────────────────
    guidelines: [],
    glLoading: false,
    glForm: { name: '', condition_ref: '', action_text: '', composition_mode: 'append', priority: 0, enabled: true },
    glCreating: false,
    guidelineRelationships: [],
    relLoading: false,
    relForm: { from_guideline_id: '', to_guideline_id: '', relation_type: 'prioritizes_over' },
    relCreating: false,

    // ── Journeys ─────────────────────────────────────────────────────────────
    journeys: [],
    jrLoading: false,
    journeyView: 'list',
    jrForm: { name: '', trigger_config: '{}', completion_rule: '', enabled: true },
    jrCreating: false,
    expandedJourney: null,
    journeyStates: {},
    journeyTransitions: {},
    stateForm: { name: '', description: '', required_fields: '' },
    stateCreating: false,
    transitionForm: { from_state_id: '', to_state_id: '', transition_type: 'auto', condition_config: '{}' },
    transitionCreating: false,

    // ── Knowledge ────────────────────────────────────────────────────────────
    retrievers: [],
    retrieversLoading: false,
    retrieverBindings: [],
    bindingsLoading: false,
    glossaryTerms: [],
    glossaryLoading: false,
    contextVariables: [],
    contextVariablesLoading: false,
    cannedResponses: [],
    cannedResponsesLoading: false,
    retrieverForm: { name: '', retriever_type: 'static', config_json: '{"items":[]}' },
    retrieverCreating: false,
    bindingForm: { retriever_id: '', bind_type: 'guideline', bind_ref: '' },
    bindingCreating: false,
    glossaryForm: { name: '', description: '', synonyms: '', alwaysInclude: false },
    glossaryCreating: false,
    varForm: { name: '', value_source_type: 'static', value_source_config: '{"value":""}', visibility_rule: '' },
    varCreating: false,
    cannedForm: { name: '', template_text: '', trigger_rule: '', priority: 0 },
    cannedCreating: false,

    // ── Tool Gate ─────────────────────────────────────────────────────────────
    toolPolicies: [],
    tpLoading: false,
    tpForm: {
      tool_name: '', skill_ref: '', observation_ref: '', journey_state_ref: '',
      guideline_ref: '', approval_mode: 'none', enabled: true,
    },
    tpCreating: false,

    // ── Releases ─────────────────────────────────────────────────────────────
    releases: [],
    releasesLoading: false,
    releaseForm: { version: '', published_by: 'system' },
    releasePublishing: false,
    releaseRollback: false,

    // ── Handoff / Manual Mode ────────────────────────────────────────────────
    handoffSessionId: '',
    handoffReason: '',
    handoffSummary: '',
    handoffCreating: false,
    handoffResult: null,
    handoffList: [],
    handoffListLoading: false,
    manualModeSessionId: '',
    manualModeOp: 'enable',  // 'enable' | 'disable'
    manualModeResult: null,
    manualModeLoading: false,

    // ────────────────────────────────────────────────────────────────────────

    async init() {
      await this.loadScopes();
      // Pre-fill agent/session from URL hash params if present
      var hash = window.location.hash.replace('#control', '');
      var params = new URLSearchParams(hash.replace(/^[?&]/, '?'));
      if (params.get('agent')) this.debugAgentId = params.get('agent');
      if (params.get('session')) this.debugSessionId = params.get('session');
    },

    // ── Scopes ───────────────────────────────────────────────────────────────

    /** Label shown in the scope dropdown; uses option[label] instead of x-text for reliable rendering. */
    scopeSelectLabel(s) {
      var id = String(s && s.scope_id != null ? s.scope_id : '');
      var raw = s && s.name != null ? String(s.name) : '';
      var nm = raw.trim();
      if (!nm) nm = id ? id.slice(0, 8) + '…' : '—';
      var short = id ? id.slice(0, 8) : '';
      return short ? nm + ' (' + short + '…)' : nm;
    },

    async loadScopes() {
      try {
        var data = await SiliCrewAPI.get('/api/control/scopes');
        this.scopes = data || [];
        if (!this.selectedScope && this.scopes.length > 0) {
          this.selectedScope = this.scopes[0].scope_id;
          await this.onScopeChange();
        }
      } catch (e) {
        console.warn('control: loadScopes failed', e);
      }
    },

    async createScope() {
      if (!this.newScopeName.trim()) return;
      this.creatingScope = true;
      try {
        var s = await SiliCrewAPI.post('/api/control/scopes', { name: this.newScopeName.trim() });
        this.scopes.push(s);
        this.selectedScope = s.scope_id;
        this.newScopeName = '';
        await this.onScopeChange();
        this.toastSuccess('Scope "' + (s.name || 'new scope') + '" created');
      } catch (e) {
        this.toastError('Failed to create scope: ' + e.message);
      } finally {
        this.creatingScope = false;
      }
    },

    async onScopeChange() {
      if (!this.selectedScope) return;
      window.__silicrewControlScope = this.selectedScope || '';
      window.dispatchEvent(new CustomEvent('control-scope-changed', {
        detail: { scopeId: this.selectedScope || '' }
      }));
      await Promise.all([
        this.loadObservations(),
        this.loadGuidelines(),
        this.loadGuidelineRelationships(),
        this.loadJourneys(),
        this.loadToolPolicies(),
        this.loadRetrievers(),
        this.loadRetrieverBindings(),
        this.loadGlossaryTerms(),
        this.loadContextVariables(),
        this.loadCannedResponses(),
        this.loadReleases()
      ]);
    },

    // ── Compile-turn Debug ───────────────────────────────────────────────────

    async runCompileTurn() {
      if (!this.debugAgentId.trim() || !this.debugSessionId.trim()) {
        this.debugError = 'Agent ID and Session ID are required';
        return;
      }
      this.debugLoading = true;
      this.debugError = '';
      this.debugResult = null;
      try {
        var result = await SiliCrewAPI.post('/api/control/test/compile-turn', {
          scope_id: this.selectedScope || this.debugAgentId,
          agent_id: this.debugAgentId.trim(),
          session_id: this.debugSessionId.trim(),
          message: this.debugMessage,
          channel_type: 'web',
        });
        this.debugResult = result;
      } catch (e) {
        this.debugError = e.message || String(e);
      } finally {
        this.debugLoading = false;
      }
    },

    // Count helper
    countItems(arr) { return Array.isArray(arr) ? arr.length : 0; },

    badgeClass(n) {
      if (n === 0) return 'badge badge-muted';
      return 'badge badge-success';
    },

    toast(level, message) {
      if (typeof SiliCrewToast !== 'undefined' && SiliCrewToast[level]) {
        SiliCrewToast[level](message);
        return;
      }
      if (level === 'error') console.error(message);
      else console.log(message);
    },

    toastSuccess(message) {
      this.toast('success', message);
    },

    toastError(message) {
      this.toast('error', message);
    },

    selectedScopeRecord() {
      for (var i = 0; i < this.scopes.length; i++) {
        if (this.scopes[i].scope_id === this.selectedScope) return this.scopes[i];
      }
      return null;
    },

    switchTab(nextTab, journeyView) {
      this.tab = nextTab;
      if (nextTab === 'journeys' && journeyView) this.journeyView = journeyView;
      if (nextTab === 'observations') this.loadObservations();
      if (nextTab === 'guidelines') {
        this.loadGuidelines();
        this.loadGuidelineRelationships();
      }
      if (nextTab === 'journeys') this.loadJourneys();
      if (nextTab === 'knowledge') {
        this.loadRetrievers();
        this.loadRetrieverBindings();
        this.loadGlossaryTerms();
        this.loadContextVariables();
        this.loadCannedResponses();
      }
      if (nextTab === 'toolgate') this.loadToolPolicies();
      if (nextTab === 'releases') this.loadReleases();
    },

    openJourneyBuilder() {
      this.switchTab('journeys', 'builder');
      window.dispatchEvent(new CustomEvent('control-journey-builder-import', { detail: null }));
    },

    async inspectJourneyInBuilder(journey) {
      if (!journey || !journey.journey_id) return;
      this.switchTab('journeys', 'builder');
      try {
        var full = await SiliCrewAPI.get('/api/control/journeys/' + journey.journey_id);
        var states = await SiliCrewAPI.get('/api/control/journeys/' + journey.journey_id + '/states');
        var transitions = await SiliCrewAPI.get('/api/control/journeys/' + journey.journey_id + '/transitions');
        setTimeout(function() {
          window.dispatchEvent(new CustomEvent('control-journey-builder-import', {
            detail: {
              journey: full,
              states: states || [],
              transitions: transitions || [],
            }
          }));
        }, 0);
      } catch (e) {
        this.toastError('Failed to open journey graph: ' + (e.message || e));
      }
    },

    // ── Observations ─────────────────────────────────────────────────────────

    async loadObservations() {
      if (!this.selectedScope) return;
      this.obsLoading = true;
      try {
        this.observations = await SiliCrewAPI.get('/api/control/scopes/' + this.selectedScope + '/observations') || [];
      } catch (e) {
        this.observations = [];
      } finally {
        this.obsLoading = false;
      }
    },

    async createObservation() {
      if (!this.obsForm.name.trim() || !this.selectedScope) return;
      this.obsCreating = true;
      try {
        var cfg = {};
        try { cfg = JSON.parse(this.obsForm.matcher_config || '{}'); } catch (_) {}
        var obs = await SiliCrewAPI.post('/api/control/observations', {
          scope_id: this.selectedScope,
          name: this.obsForm.name,
          matcher_type: this.obsForm.matcher_type,
          matcher_config: cfg,
          priority: Number(this.obsForm.priority) || 0,
          enabled: this.obsForm.enabled,
        });
        this.observations.push(obs);
        this.obsForm = { name: '', matcher_type: 'keyword', priority: 0, enabled: true, matcher_config: '{}' };
        this.toastSuccess('Observation "' + obs.name + '" created');
      } catch (e) {
        this.toastError('Failed to create observation: ' + e.message);
      } finally {
        this.obsCreating = false;
      }
    },

    // ── Guidelines ───────────────────────────────────────────────────────────

    async loadGuidelines() {
      if (!this.selectedScope) return;
      this.glLoading = true;
      try {
        this.guidelines = await SiliCrewAPI.get('/api/control/scopes/' + this.selectedScope + '/guidelines') || [];
      } catch (e) {
        this.guidelines = [];
      } finally {
        this.glLoading = false;
      }
    },

    async createGuideline() {
      if (!this.glForm.name.trim() || !this.glForm.action_text.trim() || !this.selectedScope) return;
      this.glCreating = true;
      try {
        var g = await SiliCrewAPI.post('/api/control/guidelines', {
          scope_id: this.selectedScope,
          name: this.glForm.name,
          condition_ref: this.glForm.condition_ref,
          action_text: this.glForm.action_text,
          composition_mode: this.glForm.composition_mode,
          priority: Number(this.glForm.priority) || 0,
          enabled: this.glForm.enabled,
        });
        this.guidelines.push(g);
        this.glForm = { name: '', condition_ref: '', action_text: '', composition_mode: 'append', priority: 0, enabled: true };
        this.toastSuccess('Guideline "' + g.name + '" created');
      } catch (e) {
        this.toastError('Failed to create guideline: ' + e.message);
      } finally {
        this.glCreating = false;
      }
    },

    async loadGuidelineRelationships() {
      if (!this.selectedScope) return;
      this.relLoading = true;
      try {
        this.guidelineRelationships = await SiliCrewAPI.get('/api/control/scopes/' + this.selectedScope + '/guideline-relationships') || [];
      } catch (e) {
        this.guidelineRelationships = [];
      } finally {
        this.relLoading = false;
      }
    },

    async createGuidelineRelationship() {
      if (!this.selectedScope || !this.relForm.from_guideline_id || !this.relForm.to_guideline_id) return;
      if (this.relForm.from_guideline_id === this.relForm.to_guideline_id) {
        this.toastError('Relationship endpoints must be different guidelines');
        return;
      }
      this.relCreating = true;
      try {
        await SiliCrewAPI.post('/api/control/guideline-relationships', {
          scope_id: this.selectedScope,
          from_guideline_id: this.relForm.from_guideline_id,
          to_guideline_id: this.relForm.to_guideline_id,
          relation_type: this.relForm.relation_type,
        });
        this.relForm = { from_guideline_id: '', to_guideline_id: '', relation_type: 'prioritizes_over' };
        await this.loadGuidelineRelationships();
        this.toastSuccess('Guideline relationship created');
      } catch (e) {
        this.toastError('Failed to create relationship: ' + e.message);
      } finally {
        this.relCreating = false;
      }
    },

    guidelineNameById(id) {
      for (var i = 0; i < this.guidelines.length; i++) {
        if (this.guidelines[i].guideline_id === id) return this.guidelines[i].name;
      }
      return id || 'Unknown guideline';
    },

    shortId(id) {
      if (!id) return '—';
      var value = String(id);
      return value.length > 12 ? value.substring(0, 12) + '…' : value;
    },

    journeyRecordById(jid) {
      for (var i = 0; i < this.journeys.length; i++) {
        if (this.journeys[i].journey_id === jid) return this.journeys[i];
      }
      return null;
    },

    updateJourneyRecord(jid, patch) {
      var next = [];
      for (var i = 0; i < this.journeys.length; i++) {
        if (this.journeys[i].journey_id === jid) next.push(Object.assign({}, this.journeys[i], patch));
        else next.push(this.journeys[i]);
      }
      this.journeys = next;
    },

    journeyFallbackEntryStateId(jid) {
      var states = this.journeyStates[jid] || [];
      if (!states.length) return '';
      var inbound = {};
      var transitions = this.journeyTransitions[jid] || [];
      for (var i = 0; i < transitions.length; i++) inbound[transitions[i].to_state_id] = true;
      for (var s = 0; s < states.length; s++) {
        if (!inbound[states[s].state_id]) return states[s].state_id;
      }
      return states[0].state_id || '';
    },

    journeyEffectiveEntryStateId(journey) {
      if (!journey) return '';
      if (journey.entry_state_id) return journey.entry_state_id;
      return this.journeyFallbackEntryStateId(journey.journey_id);
    },

    journeyEntryStateLabel(journey) {
      if (!journey) return 'entry: auto';
      var entryStateId = this.journeyEffectiveEntryStateId(journey);
      if (!entryStateId) return 'entry: auto';
      var states = this.journeyStates[journey.journey_id] || [];
      for (var i = 0; i < states.length; i++) {
        if (states[i].state_id === entryStateId) {
          return 'entry: ' + (states[i].name || this.shortId(states[i].state_id));
        }
      }
      return 'entry: ' + this.shortId(entryStateId);
    },

    isJourneyEntryState(jid, stateId) {
      var journey = this.journeyRecordById(jid);
      return !!(journey && this.journeyEffectiveEntryStateId(journey) === stateId);
    },

    // ── Journeys ─────────────────────────────────────────────────────────────

    async loadJourneys() {
      if (!this.selectedScope) return;
      this.jrLoading = true;
      try {
        this.journeys = await SiliCrewAPI.get('/api/control/scopes/' + this.selectedScope + '/journeys') || [];
      } catch (e) {
        this.journeys = [];
      } finally {
        this.jrLoading = false;
      }
    },

    async createJourney() {
      if (!this.jrForm.name.trim() || !this.selectedScope) return;
      this.jrCreating = true;
      try {
        var cfg = {};
        try { cfg = JSON.parse(this.jrForm.trigger_config || '{}'); } catch (_) {}
        var j = await SiliCrewAPI.post('/api/control/journeys', {
          scope_id: this.selectedScope,
          name: this.jrForm.name,
          trigger_config: cfg,
          completion_rule: this.jrForm.completion_rule || null,
          enabled: this.jrForm.enabled,
        });
        this.journeys.push(j);
        this.jrForm = { name: '', trigger_config: '{}', completion_rule: '', enabled: true };
        this.toastSuccess('Journey "' + j.name + '" created');
      } catch (e) {
        this.toastError('Failed to create journey: ' + e.message);
      } finally {
        this.jrCreating = false;
      }
    },

    async toggleJourneyExpand(jid) {
      if (this.expandedJourney === jid) {
        this.expandedJourney = null;
        return;
      }
      this.expandedJourney = jid;
      await this.loadJourneyStates(jid);
      await this.loadJourneyTransitions(jid);
    },

    async loadJourneyStates(jid) {
      try {
        var data = await SiliCrewAPI.get('/api/control/journeys/' + jid + '/states');
        this.journeyStates = Object.assign({}, this.journeyStates, { [jid]: data || [] });
      } catch (e) {
        this.journeyStates = Object.assign({}, this.journeyStates, { [jid]: [] });
      }
    },

    async loadJourneyTransitions(jid) {
      try {
        var data = await SiliCrewAPI.get('/api/control/journeys/' + jid + '/transitions');
        this.journeyTransitions = Object.assign({}, this.journeyTransitions, { [jid]: data || [] });
      } catch (e) {
        this.journeyTransitions = Object.assign({}, this.journeyTransitions, { [jid]: [] });
      }
    },

    async createJourneyState(jid) {
      if (!this.stateForm.name.trim()) return;
      this.stateCreating = true;
      try {
        var fields = this.stateForm.required_fields
          ? this.stateForm.required_fields.split(',').map(function(s) { return s.trim(); }).filter(Boolean)
          : [];
        var s = await SiliCrewAPI.post('/api/control/journeys/' + jid + '/states', {
          name: this.stateForm.name,
          description: this.stateForm.description || null,
          required_fields: fields,
        });
        var cur = (this.journeyStates[jid] || []).slice();
        cur.push(s);
        this.journeyStates = Object.assign({}, this.journeyStates, { [jid]: cur });
        var journey = this.journeyRecordById(jid);
        if (journey && !journey.entry_state_id) {
          this.updateJourneyRecord(jid, { entry_state_id: s.state_id });
        }
        this.stateForm = { name: '', description: '', required_fields: '' };
        this.toastSuccess('Journey state "' + s.name + '" created');
      } catch (e) {
        this.toastError('Failed to create state: ' + e.message);
      } finally {
        this.stateCreating = false;
      }
    },

    async setJourneyEntryState(jid, stateId) {
      if (!jid || !stateId) return;
      try {
        await SiliCrewAPI.post('/api/control/journeys/' + jid + '/entry-state', {
          state_id: stateId,
        });
        this.updateJourneyRecord(jid, { entry_state_id: stateId });
        this.toastSuccess('Journey entry state updated');
      } catch (e) {
        this.toastError('Failed to set entry state: ' + e.message);
      }
    },

    async createJourneyTransition(jid) {
      if (!this.transitionForm.from_state_id || !this.transitionForm.to_state_id) return;
      this.transitionCreating = true;
      try {
        var cfg = {};
        try { cfg = JSON.parse(this.transitionForm.condition_config || '{}'); } catch (_) {}
        var t = await SiliCrewAPI.post('/api/control/journeys/' + jid + '/transitions', {
          from_state_id: this.transitionForm.from_state_id,
          to_state_id: this.transitionForm.to_state_id,
          transition_type: this.transitionForm.transition_type,
          condition_config: cfg,
        });
        var cur = (this.journeyTransitions[jid] || []).slice();
        cur.push(t);
        this.journeyTransitions = Object.assign({}, this.journeyTransitions, { [jid]: cur });
        this.transitionForm = { from_state_id: '', to_state_id: '', transition_type: 'auto', condition_config: '{}' };
        this.toastSuccess('Journey transition added');
      } catch (e) {
        this.toastError('Failed to create transition: ' + e.message);
      } finally {
        this.transitionCreating = false;
      }
    },

    // ── Knowledge ────────────────────────────────────────────────────────────

    async loadRetrievers() {
      if (!this.selectedScope) return;
      this.retrieversLoading = true;
      try {
        this.retrievers = await SiliCrewAPI.get('/api/control/scopes/' + this.selectedScope + '/retrievers') || [];
      } catch (e) {
        this.retrievers = [];
      } finally {
        this.retrieversLoading = false;
      }
    },

    async createRetriever() {
      if (!this.retrieverForm.name.trim() || !this.selectedScope) return;
      this.retrieverCreating = true;
      try {
        var cfg = {};
        try { cfg = JSON.parse(this.retrieverForm.config_json || '{}'); } catch (_) {}
        await SiliCrewAPI.post('/api/control/retrievers', {
          scope_id: this.selectedScope,
          name: this.retrieverForm.name.trim(),
          retriever_type: this.retrieverForm.retriever_type || 'static',
          config_json: cfg,
          enabled: true
        });
        this.retrieverForm = { name: '', retriever_type: 'static', config_json: '{"items":[]}' };
        await this.loadRetrievers();
        this.toastSuccess('Retriever created');
      } catch (e) {
        this.toastError('Failed to create retriever: ' + e.message);
      } finally {
        this.retrieverCreating = false;
      }
    },

    async loadRetrieverBindings() {
      if (!this.selectedScope) return;
      this.bindingsLoading = true;
      try {
        this.retrieverBindings = await SiliCrewAPI.get('/api/control/scopes/' + this.selectedScope + '/retriever-bindings') || [];
      } catch (e) {
        this.retrieverBindings = [];
      } finally {
        this.bindingsLoading = false;
      }
    },

    async loadGlossaryTerms() {
      if (!this.selectedScope) return;
      this.glossaryLoading = true;
      try {
        this.glossaryTerms = await SiliCrewAPI.get('/api/control/scopes/' + this.selectedScope + '/glossary-terms') || [];
      } catch (e) {
        this.glossaryTerms = [];
      } finally {
        this.glossaryLoading = false;
      }
    },

    async loadContextVariables() {
      if (!this.selectedScope) return;
      this.contextVariablesLoading = true;
      try {
        this.contextVariables = await SiliCrewAPI.get('/api/control/scopes/' + this.selectedScope + '/context-variables') || [];
      } catch (e) {
        this.contextVariables = [];
      } finally {
        this.contextVariablesLoading = false;
      }
    },

    async loadCannedResponses() {
      if (!this.selectedScope) return;
      this.cannedResponsesLoading = true;
      try {
        this.cannedResponses = await SiliCrewAPI.get('/api/control/scopes/' + this.selectedScope + '/canned-responses') || [];
      } catch (e) {
        this.cannedResponses = [];
      } finally {
        this.cannedResponsesLoading = false;
      }
    },

    async createRetrieverBinding() {
      if (!this.bindingForm.retriever_id || !this.bindingForm.bind_ref.trim() || !this.selectedScope) return;
      this.bindingCreating = true;
      try {
        await SiliCrewAPI.post('/api/control/retriever-bindings', {
          scope_id: this.selectedScope,
          retriever_id: this.bindingForm.retriever_id,
          bind_type: this.bindingForm.bind_type,
          bind_ref: this.bindingForm.bind_ref.trim()
        });
        this.bindingForm = { retriever_id: '', bind_type: 'guideline', bind_ref: '' };
        await this.loadRetrieverBindings();
        this.toastSuccess('Retriever binding created');
      } catch (e) {
        this.toastError('Failed to create binding: ' + e.message);
      } finally {
        this.bindingCreating = false;
      }
    },

    async deleteRetrieverBinding(id) {
      if (!id || !confirm('Delete this retriever binding?')) return;
      try {
        await SiliCrewAPI.delete('/api/control/retriever-bindings/' + encodeURIComponent(id));
        await this.loadRetrieverBindings();
        this.toastSuccess('Binding deleted');
      } catch (e) {
        this.toastError('Failed to delete: ' + e.message);
      }
    },

    async createGlossaryTerm() {
      if (!this.glossaryForm.name.trim() || !this.selectedScope) return;
      this.glossaryCreating = true;
      try {
        var syns = this.glossaryForm.synonyms
          ? this.glossaryForm.synonyms.split(',').map(function(s) { return s.trim(); }).filter(Boolean)
          : [];
        await SiliCrewAPI.post('/api/control/glossary-terms', {
          scope_id: this.selectedScope,
          name: this.glossaryForm.name,
          description: this.glossaryForm.description,
          synonyms: syns,
          always_include: !!this.glossaryForm.alwaysInclude
        });
        this.glossaryForm = { name: '', description: '', synonyms: '', alwaysInclude: false };
        await this.loadGlossaryTerms();
        this.toastSuccess('Glossary term created');
      } catch (e) {
        this.toastError('Failed to create glossary term: ' + e.message);
      } finally {
        this.glossaryCreating = false;
      }
    },

    async createContextVariable() {
      if (!this.varForm.name.trim() || !this.selectedScope) return;
      this.varCreating = true;
      try {
        var cfg = {};
        try { cfg = JSON.parse(this.varForm.value_source_config || '{}'); } catch (_) {}
        await SiliCrewAPI.post('/api/control/context-variables', {
          scope_id: this.selectedScope,
          name: this.varForm.name,
          value_source_type: this.varForm.value_source_type,
          value_source_config: cfg,
          visibility_rule: this.varForm.visibility_rule || null,
        });
        this.varForm = { name: '', value_source_type: 'static', value_source_config: '{"value":""}', visibility_rule: '' };
        await this.loadContextVariables();
        this.toastSuccess('Context variable created');
      } catch (e) {
        this.toastError('Failed to create context variable: ' + e.message);
      } finally {
        this.varCreating = false;
      }
    },

    async createCannedResponse() {
      if (!this.cannedForm.name.trim() || !this.cannedForm.template_text.trim() || !this.selectedScope) return;
      this.cannedCreating = true;
      try {
        await SiliCrewAPI.post('/api/control/canned-responses', {
          scope_id: this.selectedScope,
          name: this.cannedForm.name,
          template_text: this.cannedForm.template_text,
          trigger_rule: this.cannedForm.trigger_rule || null,
          priority: Number(this.cannedForm.priority) || 0,
        });
        this.cannedForm = { name: '', template_text: '', trigger_rule: '', priority: 0 };
        await this.loadCannedResponses();
        this.toastSuccess('Canned response created');
      } catch (e) {
        this.toastError('Failed to create canned response: ' + e.message);
      } finally {
        this.cannedCreating = false;
      }
    },

    // ── Tool Policies ─────────────────────────────────────────────────────────

    async loadToolPolicies() {
      if (!this.selectedScope) return;
      this.tpLoading = true;
      try {
        this.toolPolicies = await SiliCrewAPI.get('/api/control/scopes/' + this.selectedScope + '/tool-policies') || [];
      } catch (e) {
        this.toolPolicies = [];
      } finally {
        this.tpLoading = false;
      }
    },

    async createToolPolicy() {
      if (!this.tpForm.tool_name.trim() || !this.selectedScope) return;
      this.tpCreating = true;
      try {
        var p = await SiliCrewAPI.post('/api/control/tool-policies', {
          scope_id: this.selectedScope,
          tool_name: this.tpForm.tool_name,
          skill_ref: this.tpForm.skill_ref || null,
          observation_ref: this.tpForm.observation_ref || null,
          journey_state_ref: this.tpForm.journey_state_ref || null,
          guideline_ref: this.tpForm.guideline_ref || null,
          approval_mode: this.tpForm.approval_mode,
          enabled: this.tpForm.enabled,
        });
        this.toolPolicies.push(p);
        this.tpForm = { tool_name: '', skill_ref: '', observation_ref: '', journey_state_ref: '', guideline_ref: '', approval_mode: 'none', enabled: true };
        this.toastSuccess('Tool policy for "' + p.tool_name + '" created');
      } catch (e) {
        this.toastError('Failed to create tool policy: ' + e.message);
      } finally {
        this.tpCreating = false;
      }
    },

    approvalModeBadge(mode) {
      var map = { 'none': 'badge-success', 'conditional': 'badge-warn', 'required': 'badge-error' };
      return 'badge ' + (map[mode] || 'badge-muted');
    },

    // ── Releases ─────────────────────────────────────────────────────────────

    async loadReleases() {
      if (!this.selectedScope) return;
      this.releasesLoading = true;
      try {
        this.releases = await SiliCrewAPI.get('/api/control/scopes/' + this.selectedScope + '/releases') || [];
      } catch (e) {
        this.releases = [];
      } finally {
        this.releasesLoading = false;
      }
    },

    async publishRelease() {
      if (!this.selectedScope || !this.releaseForm.version.trim()) return;
      this.releasePublishing = true;
      try {
        await SiliCrewAPI.post('/api/control/releases/publish', {
          scope_id: this.selectedScope,
          version: this.releaseForm.version.trim(),
          published_by: (this.releaseForm.published_by || 'system').trim() || 'system',
        });
        await this.loadReleases();
        this.toastSuccess('Release published');
      } catch (e) {
        this.toastError('Failed to publish release: ' + e.message);
      } finally {
        this.releasePublishing = false;
      }
    },

    async rollbackRelease() {
      if (!this.selectedScope) return;
      this.releaseRollback = true;
      try {
        await SiliCrewAPI.post('/api/control/releases/rollback', { scope_id: this.selectedScope });
        await this.loadReleases();
        this.toastSuccess('Release rolled back');
      } catch (e) {
        this.toastError('Failed to roll back release: ' + e.message);
      } finally {
        this.releaseRollback = false;
      }
    },

    currentPublishedRelease() {
      for (var i = 0; i < this.releases.length; i++) {
        if (this.releases[i].status === 'published') return this.releases[i];
      }
      return null;
    },

    releaseStatusBadge(status) {
      var map = {
        'published': 'badge badge-ok',
        'superseded': 'badge badge-muted',
        'rolled_back': 'badge badge-warn'
      };
      return map[status] || 'badge badge-muted';
    },

    // ── Handoff / Manual Mode ─────────────────────────────────────────────────

    async doHandoff() {
      if (!this.handoffSessionId.trim() || !this.handoffReason.trim()) return;
      this.handoffCreating = true;
      this.handoffResult = null;
      try {
        this.handoffResult = await SiliCrewAPI.post(
          '/api/sessions/' + this.handoffSessionId + '/handoff',
          { reason: this.handoffReason, summary: this.handoffSummary || null }
        );
        this.toastSuccess('Handoff created');
      } catch (e) {
        this.toastError('Failed to create handoff: ' + e.message);
      } finally {
        this.handoffCreating = false;
      }
    },

    async loadHandoffs() {
      if (!this.handoffSessionId.trim()) return;
      this.handoffListLoading = true;
      try {
        this.handoffList = await SiliCrewAPI.get('/api/sessions/' + this.handoffSessionId + '/handoffs') || [];
      } catch (e) {
        this.handoffList = [];
      } finally {
        this.handoffListLoading = false;
      }
    },

    async setManualMode() {
      if (!this.manualModeSessionId.trim()) return;
      this.manualModeLoading = true;
      this.manualModeResult = null;
      var endpoint = this.manualModeOp === 'enable'
        ? '/api/sessions/' + this.manualModeSessionId + '/manual-mode'
        : '/api/sessions/' + this.manualModeSessionId + '/resume-ai';
      try {
        this.manualModeResult = await SiliCrewAPI.post(endpoint, {});
        this.toastSuccess(this.manualModeOp === 'enable' ? 'Manual mode enabled' : 'AI resumed');
      } catch (e) {
        this.toastError('Failed to update manual mode: ' + e.message);
      } finally {
        this.manualModeLoading = false;
      }
    },

    async updateHandoffStatus(handoffId, status) {
      try {
        await SiliCrewAPI.patch('/api/control/handoffs/' + handoffId + '/status', { status });
        await this.loadHandoffs();
        this.toastSuccess('Handoff status updated');
      } catch (e) {
        this.toastError('Failed to update handoff status: ' + e.message);
      }
    },

    // ── Helpers ───────────────────────────────────────────────────────────────

    formatJson(obj) {
      try { return JSON.stringify(obj, null, 2); } catch (_) { return String(obj); }
    },

    responseModeBadge(mode) {
      var map = {
        'freeform': 'badge badge-success',
        'guided': 'badge badge-info',
        'strict': 'badge badge-warn',
        'canned_only': 'badge badge-error'
      };
      return map[mode] || 'badge badge-muted';
    },
  };
}
