// SiliCrew Control Plane Journey Builder — visual graph editor for journeys
'use strict';

function controlJourneyBuilder() {
  return {
    scopeId: '',
    journeyId: '',
    builderMode: 'draft', // draft | clone | saved
    saving: false,
    builderError: '',

    journeyMeta: {
      name: '',
      completionRule: '',
      enabled: true,
      triggerMode: 'contains_any',
      triggerValue: '',
      triggerJsonText: '{}'
    },

    nodes: [],
    connections: [],
    selectedNodeId: '',
    selectedConnectionId: '',
    dragging: '',
    dragOffset: { x: 0, y: 0 },
    connectingFromId: '',
    connectPreview: null,
    canvasOffset: { x: 0, y: 0 },
    canvasDragging: false,
    canvasDragStart: { x: 0, y: 0 },
    zoom: 1,
    nextId: 1,

    _renderScheduled: false,
    _canvasEl: null,
    _importHandler: null,
    _scopeHandler: null,

    init: function() {
      var self = this;
      this.scopeId = window.__silicrewControlScope || '';
      this.resetDraft();

      this._importHandler = function(evt) {
        if (!evt || !evt.detail) {
          self.resetDraft();
          return;
        }
        self.importJourneyDraft(evt.detail);
      };
      this._scopeHandler = function(evt) {
        self.scopeId = evt && evt.detail ? (evt.detail.scopeId || '') : '';
      };

      window.addEventListener('control-journey-builder-import', this._importHandler);
      window.addEventListener('control-scope-changed', this._scopeHandler);
    },

    notify: function(level, message) {
      if (typeof SiliCrewToast !== 'undefined' && SiliCrewToast[level]) {
        SiliCrewToast[level](message);
        return;
      }
      if (level === 'error') console.error(message);
      else console.log(message);
    },

    scheduleRender: function() {
      if (this._renderScheduled) return;
      this._renderScheduled = true;
      var self = this;
      requestAnimationFrame(function() {
        self._renderScheduled = false;
        self.renderCanvas();
      });
    },

    renderCanvas: function() {
      var container = document.getElementById('cp-journey-render-group');
      if (!container) return;

      var SVG_NS = 'http://www.w3.org/2000/svg';
      var self = this;

      while (container.firstChild) container.removeChild(container.firstChild);

      for (var ci = 0; ci < this.connections.length; ci++) {
        var conn = this.connections[ci];
        var pathData = this.getConnectionPath(conn);
        if (!pathData) continue;

        var path = document.createElementNS(SVG_NS, 'path');
        path.setAttribute('d', pathData);
        path.setAttribute('fill', 'none');
        path.setAttribute('stroke', this.selectedConnectionId === conn.id ? 'var(--accent)' : 'var(--border-strong)');
        path.setAttribute('stroke-width', this.selectedConnectionId === conn.id ? '3' : '2');
        path.style.cursor = 'pointer';
        (function(connection) {
          path.addEventListener('click', function(e) {
            e.stopPropagation();
            self.selectedConnectionId = connection.id;
            self.selectedNodeId = '';
            self.scheduleRender();
          });
        })(conn);
        container.appendChild(path);

        var labelPos = this.getConnectionLabelPos(conn);
        var labelGroup = document.createElementNS(SVG_NS, 'g');
        labelGroup.style.cursor = 'pointer';
        (function(connection) {
          labelGroup.addEventListener('click', function(e) {
            e.stopPropagation();
            self.selectedConnectionId = connection.id;
            self.selectedNodeId = '';
            self.scheduleRender();
          });
        })(conn);

        var labelBg = document.createElementNS(SVG_NS, 'rect');
        labelBg.setAttribute('x', String(labelPos.x - 32));
        labelBg.setAttribute('y', String(labelPos.y - 11));
        labelBg.setAttribute('width', '64');
        labelBg.setAttribute('height', '22');
        labelBg.setAttribute('rx', '11');
        labelBg.setAttribute('fill', this.selectedConnectionId === conn.id ? 'var(--accent)' : 'var(--surface)');
        labelBg.setAttribute('stroke', this.selectedConnectionId === conn.id ? 'var(--accent)' : 'var(--border)');
        labelGroup.appendChild(labelBg);

        var label = document.createElementNS(SVG_NS, 'text');
        label.setAttribute('x', String(labelPos.x));
        label.setAttribute('y', String(labelPos.y + 4));
        label.setAttribute('text-anchor', 'middle');
        label.setAttribute('fill', this.selectedConnectionId === conn.id ? 'var(--bg-primary)' : 'var(--text-dim)');
        label.setAttribute('style', 'font-size:10px;font-weight:700;pointer-events:none');
        label.textContent = this.connectionLabel(conn);
        labelGroup.appendChild(label);

        container.appendChild(labelGroup);
      }

      if (this.connectingFromId && this.connectPreview) {
        var previewData = this.getPreviewPath();
        if (previewData) {
          var preview = document.createElementNS(SVG_NS, 'path');
          preview.setAttribute('d', previewData);
          preview.setAttribute('fill', 'none');
          preview.setAttribute('stroke', 'var(--accent)');
          preview.setAttribute('stroke-width', '2');
          preview.setAttribute('stroke-dasharray', '6,4');
          container.appendChild(preview);
        }
      }

      for (var ni = 0; ni < this.nodes.length; ni++) {
        var node = this.nodes[ni];
        var group = document.createElementNS(SVG_NS, 'g');
        group.classList.add('wf-node');
        group.setAttribute('transform', 'translate(' + node.x + ',' + node.y + ')');
        (function(currentNode) {
          group.addEventListener('mousedown', function(e) {
            self.onNodeMouseDown(currentNode, e);
          });
        })(node);

        var rect = document.createElementNS(SVG_NS, 'rect');
        rect.setAttribute('x', '0');
        rect.setAttribute('y', '0');
        rect.setAttribute('width', String(node.width));
        rect.setAttribute('height', String(node.height));
        rect.setAttribute('rx', '12');
        rect.setAttribute('ry', '12');
        rect.setAttribute('fill', this.selectedNodeId === node.id ? 'var(--surface2)' : 'var(--surface)');
        rect.setAttribute('stroke', this.selectedNodeId === node.id ? 'var(--accent)' : 'var(--border)');
        rect.setAttribute('stroke-width', this.selectedNodeId === node.id ? '2.5' : '1.5');
        group.appendChild(rect);

        var rail = document.createElementNS(SVG_NS, 'rect');
        rail.setAttribute('x', '0');
        rail.setAttribute('y', '0');
        rail.setAttribute('width', '8');
        rail.setAttribute('height', String(node.height));
        rail.setAttribute('rx', '12');
        rail.setAttribute('fill', node.isStart ? '#10b981' : '#60a5fa');
        group.appendChild(rail);

        var title = document.createElementNS(SVG_NS, 'text');
        title.setAttribute('x', '20');
        title.setAttribute('y', '28');
        title.setAttribute('fill', 'var(--text)');
        title.setAttribute('style', 'font-size:13px;font-weight:700;pointer-events:none');
        title.textContent = node.name || 'State';
        group.appendChild(title);

        var subtitle = document.createElementNS(SVG_NS, 'text');
        subtitle.setAttribute('x', '20');
        subtitle.setAttribute('y', '48');
        subtitle.setAttribute('fill', 'var(--text-dim)');
        subtitle.setAttribute('style', 'font-size:10px;pointer-events:none');
        subtitle.textContent = this.stateSubtitle(node);
        group.appendChild(subtitle);

        if (node.isStart) {
          var startBg = document.createElementNS(SVG_NS, 'rect');
          startBg.setAttribute('x', String(node.width - 58));
          startBg.setAttribute('y', '12');
          startBg.setAttribute('width', '46');
          startBg.setAttribute('height', '18');
          startBg.setAttribute('rx', '9');
          startBg.setAttribute('fill', 'rgba(16,185,129,0.18)');
          group.appendChild(startBg);

          var startLabel = document.createElementNS(SVG_NS, 'text');
          startLabel.setAttribute('x', String(node.width - 35));
          startLabel.setAttribute('y', '24');
          startLabel.setAttribute('text-anchor', 'middle');
          startLabel.setAttribute('fill', '#10b981');
          startLabel.setAttribute('style', 'font-size:10px;font-weight:700;pointer-events:none');
          startLabel.textContent = 'ENTRY';
          group.appendChild(startLabel);
        }

        var inPort = document.createElementNS(SVG_NS, 'circle');
        inPort.classList.add('wf-port', 'wf-port-in');
        inPort.setAttribute('cx', String(node.width / 2));
        inPort.setAttribute('cy', '0');
        inPort.setAttribute('r', '6');
        inPort.setAttribute('fill', 'var(--surface2)');
        inPort.setAttribute('stroke', 'var(--text-dim)');
        inPort.setAttribute('stroke-width', '2');
        (function(nodeId) {
          inPort.addEventListener('mouseup', function(e) {
            e.stopPropagation();
            self.endConnect(nodeId, e);
          });
        })(node.id);
        group.appendChild(inPort);

        var outPort = document.createElementNS(SVG_NS, 'circle');
        outPort.classList.add('wf-port', 'wf-port-out');
        outPort.setAttribute('cx', String(node.width / 2));
        outPort.setAttribute('cy', String(node.height));
        outPort.setAttribute('r', '6');
        outPort.setAttribute('fill', 'var(--surface2)');
        outPort.setAttribute('stroke', node.isStart ? '#10b981' : '#60a5fa');
        outPort.setAttribute('stroke-width', '2');
        (function(nodeId) {
          outPort.addEventListener('mousedown', function(e) {
            e.stopPropagation();
            self.startConnect(nodeId, e);
          });
        })(node.id);
        group.appendChild(outPort);

        container.appendChild(group);
      }
    },

    selectedNode: function() {
      return this.getNode(this.selectedNodeId);
    },

    selectedConnection: function() {
      return this.getConnection(this.selectedConnectionId);
    },

    stateSubtitle: function(node) {
      if (!node) return '';
      var requiredCount = this.parseCsv(node.requiredFieldsText).length;
      var actionsCount = this.parseLines(node.guidelineActionsText).length;
      if (requiredCount || actionsCount) {
        return requiredCount + ' required fields, ' + actionsCount + ' actions';
      }
      return node.description ? node.description : 'No state rules yet';
    },

    connectionLabel: function(conn) {
      if (!conn) return '';
      if (conn.transitionType === 'complete') return 'complete';
      if (conn.transitionType === 'manual') return 'manual';
      if (conn.conditionMode === 'tool_called') return 'tool';
      if (conn.conditionMode === 'response_contains') return 'response';
      if (conn.conditionMode === 'json') return 'custom';
      return conn.transitionType || 'auto';
    },

    resetDraft: function() {
      this.builderMode = 'draft';
      this.journeyId = '';
      this.builderError = '';
      this.selectedNodeId = '';
      this.selectedConnectionId = '';
      this.dragging = '';
      this.connectingFromId = '';
      this.connectPreview = null;
      this.canvasOffset = { x: 0, y: 0 };
      this.zoom = 1;
      this.nextId = 1;
      this.journeyMeta = {
        name: '',
        completionRule: '',
        enabled: true,
        triggerMode: 'contains_any',
        triggerValue: '',
        triggerJsonText: '{}'
      };
      this.nodes = [];
      this.connections = [];
      this.addStateNode(140, 100, 'Entry', true);
      this.scheduleRender();
    },

    addStateNode: function(x, y, name, isStart) {
      var node = {
        id: 'cp-state-' + this.nextId++,
        name: name || 'State ' + this.nextId,
        description: '',
        requiredFieldsText: '',
        guidelineActionsText: '',
        x: typeof x === 'number' ? x : 160,
        y: typeof y === 'number' ? y : 120,
        width: 220,
        height: 72,
        isStart: !!isStart
      };

      if (node.isStart) {
        for (var i = 0; i < this.nodes.length; i++) this.nodes[i].isStart = false;
      } else if (this.nodes.length === 0) {
        node.isStart = true;
      }

      this.nodes.push(node);
      this.selectedNodeId = node.id;
      this.selectedConnectionId = '';
      this.scheduleRender();
      return node;
    },

    onPaletteDragStart: function(e) {
      e.dataTransfer.setData('text/plain', 'state');
      e.dataTransfer.effectAllowed = 'copy';
    },

    onCanvasDrop: function(e) {
      e.preventDefault();
      if (e.dataTransfer.getData('text/plain') !== 'state') return;
      var rect = this.getCanvasRect();
      var x = (e.clientX - rect.left) / this.zoom - this.canvasOffset.x;
      var y = (e.clientY - rect.top) / this.zoom - this.canvasOffset.y;
      this.addStateNode(x - 110, y - 36, 'New State', false);
    },

    onCanvasDragOver: function(e) {
      e.preventDefault();
      e.dataTransfer.dropEffect = 'copy';
    },

    duplicateSelectedNode: function() {
      var node = this.selectedNode();
      if (!node) return;
      var copy = this.addStateNode(node.x + 36, node.y + 36, node.name + ' Copy', false);
      copy.description = node.description;
      copy.requiredFieldsText = node.requiredFieldsText;
      copy.guidelineActionsText = node.guidelineActionsText;
      this.scheduleRender();
    },

    setStartState: function(nodeId) {
      for (var i = 0; i < this.nodes.length; i++) {
        this.nodes[i].isStart = this.nodes[i].id === nodeId;
      }
      this.scheduleRender();
    },

    deleteSelectedConnection: function() {
      if (!this.selectedConnectionId) return;
      this.connections = this.connections.filter(function(conn) {
        return conn.id !== this.selectedConnectionId;
      }, this);
      this.selectedConnectionId = '';
      this.scheduleRender();
    },

    deleteSelectedNode: function() {
      if (!this.selectedNodeId) return;
      var deletedId = this.selectedNodeId;
      var deletedWasStart = !!(this.selectedNode() && this.selectedNode().isStart);

      this.nodes = this.nodes.filter(function(node) {
        return node.id !== deletedId;
      });
      this.connections = this.connections.filter(function(conn) {
        return conn.from !== deletedId && conn.to !== deletedId;
      });
      this.selectedNodeId = '';
      this.selectedConnectionId = '';

      if (deletedWasStart && this.nodes.length > 0) {
        this.nodes[0].isStart = true;
      }

      this.scheduleRender();
    },

    getNode: function(nodeId) {
      for (var i = 0; i < this.nodes.length; i++) {
        if (this.nodes[i].id === nodeId) return this.nodes[i];
      }
      return null;
    },

    getConnection: function(connectionId) {
      for (var i = 0; i < this.connections.length; i++) {
        if (this.connections[i].id === connectionId) return this.connections[i];
      }
      return null;
    },

    getInputPortPos: function(node) {
      return { x: node.x + (node.width / 2), y: node.y };
    },

    getOutputPortPos: function(node) {
      return { x: node.x + (node.width / 2), y: node.y + node.height };
    },

    startConnect: function(nodeId, e) {
      e.stopPropagation();
      this.connectingFromId = nodeId;
      var pos = this.getOutputPortPos(this.getNode(nodeId));
      this.connectPreview = { x: pos.x, y: pos.y };
      this.selectedNodeId = '';
      this.selectedConnectionId = '';
      this.scheduleRender();
    },

    endConnect: function(nodeId, e) {
      e.stopPropagation();
      if (!this.connectingFromId || this.connectingFromId === nodeId) {
        this.connectingFromId = '';
        this.connectPreview = null;
        return;
      }

      var exists = this.connections.some(function(conn) {
        return conn.from === this.connectingFromId && conn.to === nodeId;
      }, this);

      if (!exists) {
        var conn = {
          id: 'cp-conn-' + this.nextId++,
          from: this.connectingFromId,
          to: nodeId,
          transitionType: 'auto',
          conditionMode: 'always',
          conditionValue: '',
          conditionJsonText: '{}'
        };
        this.connections.push(conn);
        this.selectedConnectionId = conn.id;
        this.selectedNodeId = '';
      }

      this.connectingFromId = '';
      this.connectPreview = null;
      this.scheduleRender();
    },

    onNodeMouseDown: function(node, e) {
      e.stopPropagation();
      this.selectedNodeId = node.id;
      this.selectedConnectionId = '';
      this.dragging = node.id;
      var rect = this.getCanvasRect();
      this.dragOffset = {
        x: (e.clientX - rect.left) / this.zoom - this.canvasOffset.x - node.x,
        y: (e.clientY - rect.top) / this.zoom - this.canvasOffset.y - node.y
      };
    },

    onCanvasMouseDown: function(e) {
      if (e.target.closest('.wf-node') || e.target.closest('.wf-port')) return;
      this.selectedNodeId = '';
      this.selectedConnectionId = '';
      this.canvasDragging = true;
      this.canvasDragStart = {
        x: e.clientX - this.canvasOffset.x * this.zoom,
        y: e.clientY - this.canvasOffset.y * this.zoom
      };
    },

    onCanvasMouseMove: function(e) {
      var rect = this.getCanvasRect();
      if (this.dragging) {
        var node = this.getNode(this.dragging);
        if (node) {
          node.x = Math.max(20, (e.clientX - rect.left) / this.zoom - this.canvasOffset.x - this.dragOffset.x);
          node.y = Math.max(20, (e.clientY - rect.top) / this.zoom - this.canvasOffset.y - this.dragOffset.y);
          this.scheduleRender();
        }
        return;
      }

      if (this.connectingFromId) {
        this.connectPreview = {
          x: (e.clientX - rect.left) / this.zoom - this.canvasOffset.x,
          y: (e.clientY - rect.top) / this.zoom - this.canvasOffset.y
        };
        this.scheduleRender();
        return;
      }

      if (this.canvasDragging) {
        this.canvasOffset = {
          x: (e.clientX - this.canvasDragStart.x) / this.zoom,
          y: (e.clientY - this.canvasDragStart.y) / this.zoom
        };
      }
    },

    onCanvasMouseUp: function() {
      this.dragging = '';
      this.connectingFromId = '';
      this.connectPreview = null;
      this.canvasDragging = false;
      this.scheduleRender();
    },

    onCanvasWheel: function(e) {
      e.preventDefault();
      var delta = e.deltaY > 0 ? -0.06 : 0.06;
      this.zoom = Math.max(0.35, Math.min(2, this.zoom + delta));
    },

    zoomIn: function() {
      this.zoom = Math.min(2, this.zoom + 0.1);
    },

    zoomOut: function() {
      this.zoom = Math.max(0.35, this.zoom - 0.1);
    },

    zoomReset: function() {
      this.zoom = 1;
      this.canvasOffset = { x: 0, y: 0 };
    },

    getCanvasRect: function() {
      if (!this._canvasEl) this._canvasEl = document.getElementById('cp-journey-canvas');
      return this._canvasEl ? this._canvasEl.getBoundingClientRect() : { left: 0, top: 0 };
    },

    getConnectionPath: function(conn) {
      var fromNode = this.getNode(conn.from);
      var toNode = this.getNode(conn.to);
      if (!fromNode || !toNode) return '';
      var from = this.getOutputPortPos(fromNode);
      var to = this.getInputPortPos(toNode);
      var dy = Math.abs(to.y - from.y);
      var cp = Math.max(40, dy * 0.45);
      return 'M ' + from.x + ' ' + from.y + ' C ' + from.x + ' ' + (from.y + cp) + ' ' + to.x + ' ' + (to.y - cp) + ' ' + to.x + ' ' + to.y;
    },

    getPreviewPath: function() {
      if (!this.connectingFromId || !this.connectPreview) return '';
      var fromNode = this.getNode(this.connectingFromId);
      if (!fromNode) return '';
      var from = this.getOutputPortPos(fromNode);
      var to = this.connectPreview;
      var dy = Math.abs(to.y - from.y);
      var cp = Math.max(40, dy * 0.45);
      return 'M ' + from.x + ' ' + from.y + ' C ' + from.x + ' ' + (from.y + cp) + ' ' + to.x + ' ' + (to.y - cp) + ' ' + to.x + ' ' + to.y;
    },

    getConnectionLabelPos: function(conn) {
      var fromNode = this.getNode(conn.from);
      var toNode = this.getNode(conn.to);
      if (!fromNode || !toNode) return { x: 0, y: 0 };
      var from = this.getOutputPortPos(fromNode);
      var to = this.getInputPortPos(toNode);
      return {
        x: Math.round((from.x + to.x) / 2),
        y: Math.round((from.y + to.y) / 2)
      };
    },

    inferStartNodeId: function() {
      for (var i = 0; i < this.nodes.length; i++) {
        if (this.nodes[i].isStart) return this.nodes[i].id;
      }

      var inbound = {};
      for (var c = 0; c < this.connections.length; c++) inbound[this.connections[c].to] = true;
      for (var n = 0; n < this.nodes.length; n++) {
        if (!inbound[this.nodes[n].id]) return this.nodes[n].id;
      }
      return this.nodes.length ? this.nodes[0].id : '';
    },

    computeLevels: function() {
      var startId = this.inferStartNodeId();
      var levels = {};
      var queue = [];
      if (startId) {
        levels[startId] = 0;
        queue.push(startId);
      }

      while (queue.length) {
        var currentId = queue.shift();
        var baseLevel = levels[currentId] || 0;
        for (var i = 0; i < this.connections.length; i++) {
          var conn = this.connections[i];
          if (conn.from !== currentId) continue;
          var nextLevel = baseLevel + 1;
          if (typeof levels[conn.to] !== 'number' || nextLevel > levels[conn.to]) {
            levels[conn.to] = nextLevel;
            queue.push(conn.to);
          }
        }
      }

      for (var n = 0; n < this.nodes.length; n++) {
        if (typeof levels[this.nodes[n].id] !== 'number') levels[this.nodes[n].id] = 0;
      }

      return levels;
    },

    autoLayout: function() {
      if (!this.nodes.length) return;
      var levels = this.computeLevels();
      var columns = {};
      var maxLevel = 0;

      for (var i = 0; i < this.nodes.length; i++) {
        var node = this.nodes[i];
        var level = levels[node.id] || 0;
        maxLevel = Math.max(maxLevel, level);
        if (!columns[level]) columns[level] = [];
        columns[level].push(node);
      }

      for (var col = 0; col <= maxLevel; col++) {
        var bucket = columns[col] || [];
        bucket.sort(function(a, b) {
          if (a.isStart && !b.isStart) return -1;
          if (!a.isStart && b.isStart) return 1;
          return a.name.localeCompare(b.name);
        });

        for (var row = 0; row < bucket.length; row++) {
          bucket[row].x = 80 + (col * 270);
          bucket[row].y = 80 + (row * 130);
        }
      }

      this.scheduleRender();
    },

    parseCsv: function(text) {
      return String(text || '')
        .split(',')
        .map(function(item) { return item.trim(); })
        .filter(Boolean);
    },

    parseLines: function(text) {
      return String(text || '')
        .split('\n')
        .map(function(item) { return item.trim(); })
        .filter(Boolean);
    },

    buildTriggerConfig: function() {
      if (this.journeyMeta.triggerMode === 'always') return { always: true };
      if (this.journeyMeta.triggerMode === 'substring') return { substring: this.journeyMeta.triggerValue.trim() };
      if (this.journeyMeta.triggerMode === 'contains_all') return { all: this.parseCsv(this.journeyMeta.triggerValue) };
      if (this.journeyMeta.triggerMode === 'regex') return { regex: this.journeyMeta.triggerValue.trim() };
      if (this.journeyMeta.triggerMode === 'json') {
        try { return JSON.parse(this.journeyMeta.triggerJsonText || '{}'); } catch (_) { return {}; }
      }
      return { contains: this.parseCsv(this.journeyMeta.triggerValue) };
    },

    parseTriggerConfig: function(triggerConfig) {
      var config = triggerConfig || {};
      if (config.always === true) return { mode: 'always', value: '', json: '{}' };
      if (typeof config.substring === 'string') return { mode: 'substring', value: config.substring, json: '{}' };
      if (Array.isArray(config.contains)) return { mode: 'contains_any', value: config.contains.join(', '), json: '{}' };
      if (Array.isArray(config.all)) return { mode: 'contains_all', value: config.all.join(', '), json: '{}' };
      if (typeof config.regex === 'string') return { mode: 'regex', value: config.regex, json: '{}' };
      if (typeof config.pattern === 'string') return { mode: 'regex', value: config.pattern, json: '{}' };
      return { mode: 'json', value: '', json: this.prettyJson(config) };
    },

    buildConditionConfig: function(conn) {
      if (!conn || conn.conditionMode === 'always') return {};
      if (conn.conditionMode === 'tool_called') return { tool_called: (conn.conditionValue || '').trim() };
      if (conn.conditionMode === 'response_contains') return { response_contains: (conn.conditionValue || '').trim() };
      if (conn.conditionMode === 'json') {
        try { return JSON.parse(conn.conditionJsonText || '{}'); } catch (_) { return {}; }
      }
      return {};
    },

    parseConditionConfig: function(conditionConfig) {
      var config = conditionConfig || {};
      if (!config || !Object.keys(config).length || config.always === true) {
        return { mode: 'always', value: '', json: '{}' };
      }
      if (typeof config.tool_called === 'string') {
        return { mode: 'tool_called', value: config.tool_called, json: '{}' };
      }
      if (typeof config.response_contains === 'string') {
        return { mode: 'response_contains', value: config.response_contains, json: '{}' };
      }
      return { mode: 'json', value: '', json: this.prettyJson(config) };
    },

    prettyJson: function(obj) {
      try { return JSON.stringify(obj || {}, null, 2); } catch (_) { return '{}'; }
    },

    orderedNodesForSave: function() {
      var levels = this.computeLevels();
      var nodes = this.nodes.slice();
      nodes.sort(function(a, b) {
        if (a.isStart && !b.isStart) return -1;
        if (!a.isStart && b.isStart) return 1;
        var levelA = levels[a.id] || 0;
        var levelB = levels[b.id] || 0;
        if (levelA !== levelB) return levelA - levelB;
        if (a.y !== b.y) return a.y - b.y;
        return a.x - b.x;
      });
      return nodes;
    },

    validateDraft: function() {
      if (!this.scopeId) return 'Select a scope before saving the journey.';
      if (!this.journeyMeta.name.trim()) return 'Journey name is required.';
      if (this.journeyMeta.triggerMode !== 'always' && this.journeyMeta.triggerMode !== 'json' && !this.journeyMeta.triggerValue.trim()) {
        return 'Add a trigger so the journey can activate predictably.';
      }
      if (!this.nodes.length) return 'Add at least one state.';
      if (!this.inferStartNodeId()) return 'Mark one state as the entry state.';
      if (this.journeyMeta.triggerMode === 'json') {
        try { JSON.parse(this.journeyMeta.triggerJsonText || '{}'); } catch (_) { return 'Trigger JSON is invalid.'; }
      }
      for (var i = 0; i < this.connections.length; i++) {
        var conn = this.connections[i];
        if (conn.conditionMode === 'json') {
          try { JSON.parse(conn.conditionJsonText || '{}'); } catch (_) { return 'A transition JSON condition is invalid.'; }
        }
      }
      return '';
    },

    saveJourney: async function() {
      var validationError = this.validateDraft();
      if (validationError) {
        this.builderError = validationError;
        this.notify('error', validationError);
        return;
      }

      this.saving = true;
      this.builderError = '';

      try {
        var createdJourney = await SiliCrewAPI.post('/api/control/journeys', {
          scope_id: this.scopeId,
          name: this.journeyMeta.name.trim(),
          trigger_config: this.buildTriggerConfig(),
          completion_rule: this.journeyMeta.completionRule.trim() || null,
          enabled: !!this.journeyMeta.enabled
        });

        var nodeOrder = this.orderedNodesForSave();
        var stateIdByNodeId = {};

        for (var i = 0; i < nodeOrder.length; i++) {
          var node = nodeOrder[i];
          var createdState = await SiliCrewAPI.post('/api/control/journeys/' + createdJourney.journey_id + '/states', {
            name: node.name,
            description: node.description || null,
            required_fields: this.parseCsv(node.requiredFieldsText),
            guideline_actions: this.parseLines(node.guidelineActionsText)
          });
          stateIdByNodeId[node.id] = createdState.state_id;
        }

        var startNodeId = this.inferStartNodeId();
        if (startNodeId && stateIdByNodeId[startNodeId]) {
          await SiliCrewAPI.post('/api/control/journeys/' + createdJourney.journey_id + '/entry-state', {
            state_id: stateIdByNodeId[startNodeId]
          });
        }

        for (var c = 0; c < this.connections.length; c++) {
          var conn = this.connections[c];
          await SiliCrewAPI.post('/api/control/journeys/' + createdJourney.journey_id + '/transitions', {
            from_state_id: stateIdByNodeId[conn.from],
            to_state_id: stateIdByNodeId[conn.to],
            transition_type: conn.transitionType,
            condition_config: this.buildConditionConfig(conn)
          });
        }

        this.journeyId = createdJourney.journey_id;
        this.builderMode = 'saved';
        this.notify('success', 'Journey "' + this.journeyMeta.name.trim() + '" created');
        window.dispatchEvent(new CustomEvent('control-journey-builder-saved', {
          detail: { journeyId: createdJourney.journey_id }
        }));
      } catch (e) {
        this.builderError = e.message || 'Could not save journey.';
        this.notify('error', 'Failed to save journey: ' + this.builderError);
      } finally {
        this.saving = false;
      }
    },

    importJourneyDraft: function(detail) {
      var journey = detail && detail.journey ? detail.journey : null;
      var states = detail && Array.isArray(detail.states) ? detail.states : [];
      var transitions = detail && Array.isArray(detail.transitions) ? detail.transitions : [];

      if (!journey) {
        this.resetDraft();
        return;
      }

      this.builderMode = 'clone';
      this.journeyId = journey.journey_id || '';
      this.selectedNodeId = '';
      this.selectedConnectionId = '';
      this.nodes = [];
      this.connections = [];
      this.nextId = 1;
      this.builderError = '';

      var trigger = this.parseTriggerConfig(journey.trigger_config || {});
      this.journeyMeta = {
        name: journey.name ? journey.name + ' Copy' : '',
        completionRule: journey.completion_rule || '',
        enabled: journey.enabled !== false,
        triggerMode: trigger.mode,
        triggerValue: trigger.value,
        triggerJsonText: trigger.json
      };

      var inferredStartId = journey && typeof journey.entry_state_id === 'string'
        ? journey.entry_state_id
        : '';
      var inbound = {};
      for (var i = 0; i < transitions.length; i++) inbound[transitions[i].to_state_id] = true;
      if (!inferredStartId) {
        for (var s = 0; s < states.length; s++) {
          if (!inbound[states[s].state_id]) {
            inferredStartId = states[s].state_id;
            break;
          }
        }
      }

      var nodeIdByStateId = {};
      for (var n = 0; n < states.length; n++) {
        var state = states[n];
        var node = this.addStateNode(100, 100, state.name || 'State', state.state_id === inferredStartId || (!inferredStartId && n === 0));
        node.description = state.description || '';
        node.requiredFieldsText = Array.isArray(state.required_fields) ? state.required_fields.join(', ') : '';
        node.guidelineActionsText = Array.isArray(state.guideline_actions) ? state.guideline_actions.join('\n') : '';
        nodeIdByStateId[state.state_id] = node.id;
      }

      for (var t = 0; t < transitions.length; t++) {
        var transition = transitions[t];
        var parsed = this.parseConditionConfig(transition.condition_config || {});
        this.connections.push({
          id: 'cp-conn-' + this.nextId++,
          from: nodeIdByStateId[transition.from_state_id],
          to: nodeIdByStateId[transition.to_state_id],
          transitionType: transition.transition_type || 'auto',
          conditionMode: parsed.mode,
          conditionValue: parsed.value,
          conditionJsonText: parsed.json
        });
      }

      this.autoLayout();
      this.notify('success', 'Journey graph loaded. Saving will create a new journey copy.');
    }
  };
}
