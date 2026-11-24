.pragma library
// The pipeline graph the Pipeline tab draws: per domain, each node with the inputs it needs.
// This is the one adapter between the view and the engine's graph export (qroc-product-graph/1).
// Product comes from the export: its nodes, inputs, barriers, tail states and fan-out instances.
// The lists below give every node its title, and they are the graph only for the predicted domains
// the export does not cover. Columns, rows and edges are computed, never placed by hand.

var DOMAINS = ['PRODUCT', 'BUSINESS', 'PRESENCE', 'LEGAL'];

var LISTS = {
    PRODUCT: [
        {key: 'IDEA', title: 'Idea', needs: []},
        {key: 'STRATEGY_BRIEF', title: 'Strategy brief', needs: ['IDEA']},
        {key: 'PRE_MORTEM', title: 'Pre-mortem', needs: ['STRATEGY_BRIEF']},
        {key: 'PRD', title: 'PRD', needs: ['STRATEGY_BRIEF']},
        {key: 'VALIDATION_PLAN', title: 'Validation plan', needs: ['PRD']},
        {key: 'TECHNICAL_ARCHITECTURE', title: 'Technical architecture', needs: ['PRD']},
        {key: 'GATE_1', title: 'Gate 1: idea viability', gate: 1, needs: ['STRATEGY_BRIEF', 'PRE_MORTEM', 'PRD', 'VALIDATION_PLAN', 'TECHNICAL_ARCHITECTURE']},
        {key: 'DESIGN_ALTERNATIVES', title: 'Design alternatives', needs: ['PRD', 'TECHNICAL_ARCHITECTURE']},
        {key: 'ROADMAP', title: 'Roadmap', needs: ['PRD', 'TECHNICAL_ARCHITECTURE']},
        {key: 'UI_UX_SPEC', title: 'UI and UX spec', needs: ['PRD', 'DESIGN_ALTERNATIVES']},
        {key: 'IMPLEMENTATION_PLAN', title: 'Implementation plan', needs: ['TECHNICAL_ARCHITECTURE', 'ROADMAP']},
        {key: 'GATE_2', title: 'Gate 2: technical viability', gate: 2, needs: ['DESIGN_ALTERNATIVES', 'ROADMAP', 'UI_UX_SPEC', 'IMPLEMENTATION_PLAN']},
        {key: 'EPICS', title: 'Epics', needs: ['PRD', 'IMPLEMENTATION_PLAN', 'UI_UX_SPEC']},
        {key: 'SLICE_INDEX', title: 'Slice index', needs: ['EPICS']},
        {key: 'GATE_3', title: 'Gate 3: structure quality', gate: 3, needs: ['EPICS', 'SLICE_INDEX']},
        {key: 'AGENT_DIRECTIVES', title: 'Agent directives', needs: ['SLICE_INDEX', 'TECHNICAL_ARCHITECTURE']},
        {key: 'TASKS', title: 'Tasks', needs: ['SLICE_INDEX', 'EPICS']},
        {key: 'COMPONENT_CONTRACTS', title: 'Component contracts', needs: ['TECHNICAL_ARCHITECTURE', 'SLICE_INDEX']},
        {key: 'PHOD_SPEC', title: 'Phod spec', needs: ['COMPONENT_CONTRACTS']},
        {key: 'VERIFICATION', title: 'Verification', needs: ['PHOD_SPEC']},
        {key: 'CONSISTENCY_JOIN', title: 'Consistency join', needs: ['PHOD_SPEC', 'VERIFICATION']}
    ],
    BUSINESS: [
        {key: 'MARKET_ANALYSIS', title: 'Market analysis', needs: ['STRATEGY_BRIEF', 'PRD', 'TECHNICAL_ARCHITECTURE']},
        {key: 'COMPETITOR_ANALYSIS', title: 'Competitor analysis', needs: ['MARKET_ANALYSIS', 'STRATEGY_BRIEF', 'PRD', 'TECHNICAL_ARCHITECTURE']},
        {key: 'GATE_4', title: 'Gate 4: market research', gate: 4, needs: ['MARKET_ANALYSIS', 'COMPETITOR_ANALYSIS']},
        {key: 'FACET_DISTRIBUTION', title: 'Distribution', facet: true, needs: ['MARKET_ANALYSIS', 'COMPETITOR_ANALYSIS']},
        {key: 'FACET_ECOSYSTEM', title: 'Ecosystem', facet: true, needs: ['MARKET_ANALYSIS', 'COMPETITOR_ANALYSIS']},
        {key: 'FACET_BRAND', title: 'Brand', facet: true, needs: ['MARKET_ANALYSIS', 'COMPETITOR_ANALYSIS']},
        {key: 'FACET_DATA', title: 'Data', facet: true, needs: ['MARKET_ANALYSIS', 'COMPETITOR_ANALYSIS']},
        {key: 'FACET_COMMUNITY', title: 'Community', facet: true, needs: ['MARKET_ANALYSIS', 'COMPETITOR_ANALYSIS']},
        {key: 'FACET_CAPITAL', title: 'Capital', facet: true, needs: ['MARKET_ANALYSIS', 'COMPETITOR_ANALYSIS']},
        {key: 'BUSINESS_PLAN', title: 'Business plan', needs: ['MARKET_ANALYSIS', 'COMPETITOR_ANALYSIS', 'PRD', 'FACET_DISTRIBUTION', 'FACET_ECOSYSTEM', 'FACET_BRAND', 'FACET_DATA', 'FACET_COMMUNITY', 'FACET_CAPITAL']},
        {key: 'BRAND_GUIDE', title: 'Brand guide', needs: ['STRATEGY_BRIEF', 'PRD', 'FACET_BRAND']},
        {key: 'GO_TO_MARKET', title: 'Go-to-market', needs: ['BUSINESS_PLAN', 'COMPETITOR_ANALYSIS', 'FACET_DISTRIBUTION', 'FACET_COMMUNITY']},
        {key: 'GATE_5', title: 'Gate 5: go-to-market', gate: 5, needs: ['BUSINESS_PLAN', 'BRAND_GUIDE', 'GO_TO_MARKET']},
        {key: 'FINANCIAL_OVERVIEW', title: 'Financial overview', needs: ['BUSINESS_PLAN', 'GO_TO_MARKET', 'FACET_CAPITAL']},
        {key: 'SLIDE_DECK', title: 'Slide deck', needs: ['BUSINESS_PLAN', 'FINANCIAL_OVERVIEW', 'BRAND_GUIDE']},
        {key: 'FINANCIAL_SENSITIVITY', title: 'Financial sensitivity', needs: ['FINANCIAL_OVERVIEW']},
        {key: 'GATE_6', title: 'Gate 6: financials and pitch', gate: 6, needs: ['FINANCIAL_OVERVIEW', 'SLIDE_DECK', 'FINANCIAL_SENSITIVITY']}
    ],
    PRESENCE: [
        {key: 'MESSAGING_FRAMEWORK', title: 'Messaging framework', needs: ['BRAND_GUIDE', 'PRD']},
        {key: 'OUTBOUND_STRATEGY', title: 'Outbound strategy', needs: ['MESSAGING_FRAMEWORK', 'GO_TO_MARKET']},
        {key: 'INBOUND_STRATEGY', title: 'Inbound strategy', needs: ['MESSAGING_FRAMEWORK', 'GO_TO_MARKET']},
        {key: 'GATE_7', title: 'Gate 7: acquisition', gate: 7, needs: ['MESSAGING_FRAMEWORK', 'OUTBOUND_STRATEGY', 'INBOUND_STRATEGY']},
        {key: 'WEBSITE_SOCIAL', title: 'Website and social', needs: ['MESSAGING_FRAMEWORK', 'BRAND_GUIDE', 'INBOUND_STRATEGY']},
        {key: 'ADS_STRATEGY', title: 'Ads strategy', needs: ['MESSAGING_FRAMEWORK', 'OUTBOUND_STRATEGY']},
        {key: 'VISUAL_ASSETS', title: 'Visual assets', needs: ['BRAND_GUIDE', 'MESSAGING_FRAMEWORK']},
        {key: 'GATE_8', title: 'Gate 8: creative execution', gate: 8, needs: ['WEBSITE_SOCIAL', 'ADS_STRATEGY', 'VISUAL_ASSETS']}
    ],
    LEGAL: [
        {key: 'COMPLIANCE_PRIVACY', title: 'Compliance and privacy', needs: ['PRD', 'TECHNICAL_ARCHITECTURE']},
        {key: 'PR_POLICY', title: 'PR policy', needs: ['MESSAGING_FRAMEWORK']},
        {key: 'LICENSING', title: 'Licensing', needs: ['TECHNICAL_ARCHITECTURE', 'BUSINESS_PLAN']},
        {key: 'GATE_9', title: 'Gate 9: regulatory foundation', gate: 9, needs: ['COMPLIANCE_PRIVACY', 'PR_POLICY', 'LICENSING']},
        {key: 'TERMS', title: 'Terms', needs: ['COMPLIANCE_PRIVACY', 'BUSINESS_PLAN']},
        {key: 'CONTRACTS', title: 'Contracts', needs: ['TERMS', 'BUSINESS_PLAN']},
        {key: 'REGISTRY_PREP', title: 'Registry preparation', needs: ['LICENSING', 'TERMS']},
        {key: 'GATE_10', title: 'Gate 10: operational legal', gate: 10, needs: ['TERMS', 'CONTRACTS', 'REGISTRY_PREP']}
    ]
};

// The gate whose pass, with the person's ask, starts each domain (design sheet 3.4).
var DOMAIN_TRIGGER_GATE = {BUSINESS: 3, PRESENCE: 6, LEGAL: 6};
var FACET_TRIGGER_GATE = 4;
// Gates whose feedback the engine can revise from (qroc-gate-revise-start).
var REVISABLE_GATES = ['GATE_1', 'GATE_2', 'GATE_3'];

var COLUMN_WIDTH = 200;
var COLUMN_GAP = 24;
var NODE_HEIGHT = 52;
var NODE_GAP = 12;
var HEADER_HEIGHT = 0;

function titleOf(key) {
    for (var d = 0; d < DOMAINS.length; d++) {
        var list = LISTS[DOMAINS[d]];
        for (var i = 0; i < list.length; i++) if (list[i].key === key) return list[i].title;
    }
    return '';
}

function domainOf(key) {
    for (var d = 0; d < DOMAINS.length; d++)
        if (LISTS[DOMAINS[d]].some(function(n) { return n.key === key; })) return DOMAINS[d];
    return '';
}

// A node the lists do not title yet reads as words, never as its key.
function nameOf(key) {
    var words = String(key).toLowerCase().replace(/_/g, ' ');
    return words.charAt(0).toUpperCase() + words.slice(1);
}

var EXPORT_SCHEMA = 'qroc-product-graph/1';

function exportCovers(domain, graphExport) {
    return domain === 'PRODUCT' && !!graphExport && graphExport.schema === EXPORT_SCHEMA
        && graphExport.domain === 'PRODUCT' && Array.isArray(graphExport.nodes) && graphExport.nodes.length > 0;
}

function gateNumber(key) {
    var m = /^GATE_(\d+)$/.exec(key);
    return m ? parseInt(m[1]) : 0;
}

// The export's Product nodes as list entries; each fan-out instance follows its node in the same
// stage, a spec needing only its contract step and a verification only its own spec.
function exportList(graphExport) {
    var list = [];
    graphExport.nodes.forEach(function(n) {
        var key = String(n.id);
        list.push({key: key, title: titleOf(key) || nameOf(key), needs: (n.inputs || []).slice(), gate: gateNumber(key),
                   barrier: n.barrier || '', fromExport: true, exportState: String(n.state || ''),
                   fanOut: !!n.fan_out, revise: n.revise || null});
        (Array.isArray(n.instances) ? n.instances : []).forEach(function(inst) {
            var i = inst.index, spec = String(inst.spec || '');
            if (key === 'PHOD_SPEC')
                list.push({key: 'PHOD_SPEC#' + i, title: String(inst.name || ''), needs: ['COMPONENT_CONTRACTS'],
                           barrier: n.barrier || '', fromExport: true, instanceOf: key, exportState: spec});
            else if (key === 'VERIFICATION')
                list.push({key: 'VERIFICATION#' + i, title: String(inst.name || ''),
                           needs: spec === 'produced' ? ['PHOD_SPEC#' + i] : ['PHOD_SPEC'],
                           barrier: n.barrier || '', fromExport: true, instanceOf: key,
                           exportState: String(inst.verification || ''), specState: spec});
        });
    });
    return list;
}

// One domain laid out. With the export (Product), a node needs its listed inputs and its barrier
// gate; from the sheet's lists, its inputs and the last gate before it in the list (a gate blocks
// what follows it). Column = one past the deepest same-domain input.
function layout(domain, graphExport) {
    var fromExport = exportCovers(domain, graphExport);
    var list = fromExport ? exportList(graphExport) : (LISTS[domain] || []);
    var local = {}, nodes = [], byKey = {}, lastGate = '';
    list.forEach(function(n) { local[n.key] = true; });
    list.forEach(function(n) {
        var after = fromExport ? (n.barrier ? [n.barrier] : []) : n.gate ? [] : (lastGate ? [lastGate] : []);
        var inbound = n.needs.filter(function(k) { return local[k]; });
        after.forEach(function(k) { if (local[k] && inbound.indexOf(k) < 0) inbound.push(k); });
        var column = 0;
        inbound.forEach(function(k) { if (byKey[k]) column = Math.max(column, byKey[k].column + 1); });
        if (n.instanceOf && byKey[n.instanceOf]) column = byKey[n.instanceOf].column;
        var node = {key: n.key, title: n.title, domain: domain, gate: n.gate || 0, facet: !!n.facet,
                    needs: n.needs.slice(), inbound: inbound, column: column, row: 0, x: 0, y: 0,
                    fromExport: !!n.fromExport, exportState: n.exportState || '', instanceOf: n.instanceOf || '',
                    specState: n.specState || '', fanOut: !!n.fanOut, revise: n.revise || null,
                    domainFirst: n.needs.every(function(k) { return !local[k]; })};
        byKey[n.key] = node; nodes.push(node);
        if (n.gate) lastGate = n.key;
    });
    var rows = {}, columns = 0;
    nodes.forEach(function(node) {
        node.row = rows[node.column] || 0;
        rows[node.column] = node.row + 1;
        node.x = node.column * (COLUMN_WIDTH + COLUMN_GAP);
        node.y = HEADER_HEIGHT + node.row * (NODE_HEIGHT + NODE_GAP);
        columns = Math.max(columns, node.column + 1);
    });
    var maxRows = 0;
    Object.keys(rows).forEach(function(c) { maxRows = Math.max(maxRows, rows[c]); });
    nodes.forEach(function(node) {
        node.dependents = nodes.filter(function(o) { return o.inbound.indexOf(node.key) >= 0; }).map(function(o) { return o.key; });
    });
    return {domain: domain, fromExport: fromExport, nodes: nodes, byKey: byKey, columns: columns,
            width: columns * COLUMN_WIDTH + Math.max(0, columns - 1) * COLUMN_GAP,
            height: HEADER_HEIGHT + maxRows * NODE_HEIGHT + Math.max(0, maxRows - 1) * NODE_GAP};
}
