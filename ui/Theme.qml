pragma Singleton
import QtQuick
import Quickshell
import Quickshell.Io

// Q-ROC design tokens. Source of truth: the retained React interface
// (/example/operator/02_divisions/citium/qroc/src/index.css, index.html Tailwind config
// and the component classes). React values win; the legacy alias block at the
// bottom keeps every pre-fidelity QML file compiling against the new palette.
QtObject {
    id: theme

    // ------------------------------------------------------------------
    // Theme selection: QROC_THEME env > saved preference > 'system' (the
    // current Omarchy palette, read the way Soliton's Theme reads it) when
    // that palette is present > obsidian. The named themes stay selectable.
    // ------------------------------------------------------------------
    readonly property var themeNames: ['obsidian', 'slate', 'light', 'parchment', 'system']
    readonly property string requestedTheme: Quickshell.env('QROC_THEME') || ''
    property string prefTheme: ''
    readonly property bool systemAvailable: !!(colors.background && colors.foreground)
    readonly property string themeName: themeNames.indexOf(requestedTheme) >= 0 ? requestedTheme
        : themeNames.indexOf(prefTheme) >= 0 ? prefTheme : systemAvailable ? 'system' : 'obsidian'
    function setTheme(name) {
        if (themeNames.indexOf(name) < 0) return false;
        prefTheme = name;
        if (typeof Backend !== 'undefined' && Backend.ready) Backend.setting('theme', name);
        return true;
    }
    // Ctrl+Shift+T order from App.tsx: obsidian → slate → light → parchment → obsidian; from 'system' it starts at obsidian.
    readonly property var cycleOrder: ['obsidian', 'slate', 'light', 'parchment']
    function nextTheme(name) {
        var i = cycleOrder.indexOf(String(name || themeName));
        return cycleOrder[(i + 1) % cycleOrder.length];
    }
    function themeLabel(name) {
        var n = String(name || themeName);
        return n === 'system' ? 'Follow desktop' : n.charAt(0).toUpperCase() + n.slice(1);
    }
    function themeIcon(name) {
        switch (String(name || themeName)) {
        case 'obsidian': return 'moon';
        case 'parchment': return 'scroll';
        case 'system': return 'gear';
        default: return 'sun';
        }
    }
    property Connections prefsWatch: Connections {
        target: typeof Backend !== 'undefined' ? Backend : null
        ignoreUnknownSignals: true
        function onRestoredChanged() { if (Backend.restored && Backend.prefs && typeof Backend.prefs.theme === 'string') theme.prefTheme = Backend.prefs.theme; }
    }

    // index.css [data-theme] tables, verbatim.
    readonly property var tables: ({
        obsidian: { bgPrimary: '#0a0a0a', bgSecondary: '#141414', bgTertiary: '#242424',
            textPrimary: '#e8e3f0', textSecondary: '#b8aec6', textMuted: '#8a8499',
            border: '#2a2732', borderHover: '#4d5384', accentPrimary: '#826c9d', accentSecondary: '#6d5a86',
            codeBg: '#101010', codeInline: '#c1abce', scrollbarTrack: '#0a0a0a', scrollbarThumb: '#383838', scrollbarThumbHover: '#4d5384', light: false },
        slate: { bgPrimary: '#0b0f16', bgSecondary: '#111827', bgTertiary: '#1e293b',
            textPrimary: '#e5e7eb', textSecondary: '#a7b0c0', textMuted: '#7b8494',
            border: '#243041', borderHover: '#3b4a61', accentPrimary: '#3b82f6', accentSecondary: '#1d4ed8',
            codeBg: '#080c12', codeInline: '#93c5fd', scrollbarTrack: '#080c12', scrollbarThumb: '#334155', scrollbarThumbHover: '#475569', light: false },
        light: { bgPrimary: '#ffffff', bgSecondary: '#f8fafc', bgTertiary: '#f1f5f9',
            textPrimary: '#0f172a', textSecondary: '#475569', textMuted: '#94a3b8',
            border: '#e2e8f0', borderHover: '#94a3b8', accentPrimary: '#2563eb', accentSecondary: '#1d4ed8',
            codeBg: '#f1f5f9', codeInline: '#7c3aed', scrollbarTrack: '#f8fafc', scrollbarThumb: '#cbd5e1', scrollbarThumbHover: '#94a3b8', light: true },
        parchment: { bgPrimary: '#fdf6e3', bgSecondary: '#faf0d4', bgTertiary: '#f5edcf',
            textPrimary: '#3b3224', textSecondary: '#6b5e4a', textMuted: '#998c73',
            border: '#e8dfc0', borderHover: '#b8ae8e', accentPrimary: '#b45309', accentSecondary: '#92400e',
            codeBg: '#f5edcf', codeInline: '#92400e', scrollbarTrack: '#fdf6e3', scrollbarThumb: '#d4c9a8', scrollbarThumbHover: '#b8ae8e', light: true }
    })
    // Soliton's token map (soliton/ui/Theme.qml): base = background (window), panel = dark_background (side panel),
    // raised = lighter_background, text = foreground, line/edge = foreground tinted over base, accent = accent.
    readonly property var systemTable: systemAvailable ? ({
        bgPrimary: colors.background, bgSecondary: colors.dark_background || colors.background,
        bgTertiary: colors.lighter_background || Qt.tint(colors.background, Qt.alpha(colors.foreground, .08)),
        textPrimary: colors.foreground, textSecondary: colors.light_foreground || mixed(colors.foreground, colors.background, .25),
        textMuted: colors.muted || colors.dark_foreground || mixed(colors.foreground, colors.background, .45),
        border: Qt.tint(colors.background, Qt.alpha(colors.foreground, .16)), borderHover: Qt.tint(colors.background, Qt.alpha(colors.foreground, .28)),
        accentPrimary: colors.accent || tables.obsidian.accentPrimary,
        accentSecondary: mixed(colors.accent || tables.obsidian.accentPrimary, colors.background, .25),
        codeBg: colors.darker_background || colors.dark_background || colors.background,
        codeInline: colors.bright_yellow || colors.accent || tables.obsidian.codeInline,
        scrollbarTrack: colors.background, scrollbarThumb: colors.lighter_background || Qt.tint(colors.background, Qt.alpha(colors.foreground, .16)),
        scrollbarThumbHover: colors.accent || tables.obsidian.scrollbarThumbHover,
        light: colors.mode === 'light' }) : tables.obsidian
    readonly property var t: themeName === 'system' ? systemTable : tables[themeName]

    // ------------------------------------------------------------------
    // Palette (identity layer)
    // ------------------------------------------------------------------
    readonly property color bgPrimary: t.bgPrimary
    readonly property color bgSecondary: t.bgSecondary
    readonly property color bgTertiary: t.bgTertiary
    readonly property color textPrimary: t.textPrimary
    readonly property color textSecondary: t.textSecondary
    readonly property color textMuted: t.textMuted
    readonly property color border: t.border
    readonly property color borderHover: t.borderHover
    readonly property color accentPrimary: t.accentPrimary
    readonly property color accentSecondary: t.accentSecondary
    readonly property color textOnAccent: themeName === 'system' ? foregroundFor(accentPrimary) : '#ffffff'
    readonly property color codeBg: t.codeBg
    readonly property color codeInline: t.codeInline
    readonly property color scrollbarTrack: t.scrollbarTrack
    readonly property color scrollbarThumb: t.scrollbarThumb
    readonly property color scrollbarThumbHover: t.scrollbarThumbHover
    readonly property color overlay: Qt.rgba(0, 0, 0, 0.5)
    readonly property color gold: '#daa520'
    // Fixed by UIUX canon in every theme.
    readonly property color domainProduct: '#3b82f6'
    readonly property color domainBusiness: '#10b981'
    readonly property color domainPresence: '#8b5cf6'
    readonly property color domainLegal: '#f59e0b'
    readonly property color success: '#22c55e'
    readonly property color warning: '#f59e0b'
    readonly property color danger: '#ef4444'
    readonly property color pending: '#6b7280'
    readonly property color executing: '#3b82f6'
    readonly property color purple: '#8b5cf6'
    function domainColor(domain) {
        switch (String(domain || '').toUpperCase()) {
        case 'PRODUCT': return domainProduct;
        case 'BUSINESS': return domainBusiness;
        case 'PRESENCE': return domainPresence;
        case 'LEGAL': return domainLegal;
        default: return accentPrimary;
        }
    }
    function domainLabel(domain) {
        var d = String(domain || '').toUpperCase();
        return d ? d.charAt(0) + d.slice(1).toLowerCase() : '';
    }
    // A model as people name it: the route prefix dropped, "gpt-6.1-sol" as "GPT-6.1 Sol", "claude-opus-5-5" as "Claude Opus 5.5".
    function modelLabel(model) {
        var id = String(model || '').split(/[:\/]/).pop();
        if (!id) return '';
        var parts = id.split('-'), word = w => w.charAt(0).toUpperCase() + w.slice(1);
        if (parts[0].toLowerCase() === 'gpt') return 'GPT-' + parts.slice(1).map((w, i) => i === 0 ? w : word(w)).join(' ');
        var words = [], digits = [];
        parts.forEach(w => { if (/^\d+$/.test(w)) digits.push(w); else words.push(word(w)); });
        return words.concat(digits.length ? [digits.join('.')] : []).join(' ');
    }
    // Stored text shown to people without the engine's own record keeping: the content hashes it writes into names
    // and source headings, and the one-line intake provenance record it appends to an inbox note.
    function readableStored(text) {
        return String(text || '').replace(/\s*^Intake provenance: \{.*\}[ \t]*$/gm, '').replace(/[ \t]*"?\b[0-9a-f]{12,}\b"?/g, '');
    }
    function domainShort(domain) {
        return ({PRODUCT: 'PROD', BUSINESS: 'BUS', PRESENCE: 'PRES', LEGAL: 'LEGAL'})[String(domain || '').toUpperCase()] || '';
    }
    function statusColor(status) {
        switch (String(status || '').toLowerCase()) {
        case 'passed': case 'succeeded': case 'completed': case 'verified': return success;
        case 'failed': case 'error': case 'rejected': return danger;
        case 'running': case 'executing': case 'queued': return executing;
        case 'suspended': case 'cancelling': case 'warning': case 'stale': return warning;
        default: return pending;
        }
    }

    // ------------------------------------------------------------------
    // Typography. Inter per the React CSS when installed; the spec PNGs
    // were rendered with Noto Sans (Inter is absent on this machine).
    // ------------------------------------------------------------------
    readonly property var installedFamilies: Qt.fontFamilies()
    readonly property string fontSans: installedFamilies.indexOf('Inter') >= 0 ? 'Inter' : 'Noto Sans'
    readonly property string fontMono: installedFamilies.indexOf('JetBrains Mono') >= 0 ? 'JetBrains Mono'
        : installedFamilies.indexOf('JetBrainsMono Nerd Font') >= 0 ? 'JetBrainsMono Nerd Font' : 'monospace'
    readonly property string font: fontSans
    readonly property string mono: fontMono
    readonly property int fsMicro: 10      // text-[10px] eyebrows, chips
    readonly property int fsMono: 11       // text-[11px] model ids
    readonly property int fsXs: 12         // text-xs
    readonly property int fsSm: 14         // text-sm (body)
    readonly property int fsBase: 16       // text-base
    readonly property int fsLg: 18         // text-lg
    readonly property int fsXl: 20         // text-xl
    readonly property int lhXs: 16
    readonly property int lhSm: 20
    readonly property int lhBase: 24
    readonly property int lhLg: 28
    readonly property int lhXl: 28
    readonly property real readerLineHeight: 1.6
    function trackingWider(size) { return size * 0.05; }   // tracking-wider
    function trackingWide(size) { return size * 0.025; }   // tracking-wide

    // ------------------------------------------------------------------
    // Metrics (Tailwind 4px scale)
    // ------------------------------------------------------------------
    readonly property int sidebarWidth: 256        // w-64
    readonly property int sidebarRail: 64          // w-16
    readonly property int headerHeight: 56         // h-14
    readonly property int domainTabHeight: 48      // px-4 py-3 + 2px underline + 1px rule
    readonly property int branchTabHeight: 46      // .branch-tab 12px 16px + 2px
    readonly property int controlHeight: 36        // px-4 py-2 text-sm
    readonly property int controlHeightLg: 40      // py-2.5
    readonly property int controlHeightSm: 32      // py-1.5
    readonly property int rowHeight: 36            // sidebar rows min-h-9
    readonly property int docRowHeight: 56         // .doc-row 12px 16px + two lines
    readonly property int radiusSm: 4              // rounded
    readonly property int radiusMd: 6              // rounded-md
    readonly property int radius: 8                // rounded-lg
    readonly property int radiusLg: 12             // rounded-xl, .branch-card
    readonly property int radiusXl: 16             // rounded-2xl
    readonly property int radiusFull: 999
    readonly property int s1: 4
    readonly property int s2: 8
    readonly property int s3: 12
    readonly property int s4: 16
    readonly property int s5: 20
    readonly property int s6: 24
    readonly property int s8: 32
    readonly property int contentMaxWidth: 1152    // max-w-6xl (project dashboard)
    readonly property int landingMaxWidth: 1024    // max-w-5xl (recent projects)
    readonly property int modalSm: 384
    readonly property int modalMd: 448
    readonly property int modalLg: 512
    readonly property int modalXl: 576
    readonly property int modal2xl: 672
    readonly property int modalWide: 1000
    readonly property int iconXs: 12
    readonly property int iconSm: 14
    readonly property int icon: 16
    readonly property int iconMd: 20
    readonly property int iconLg: 28
    readonly property int iconXl: 40
    readonly property int progressHeight: 6        // h-1.5
    readonly property int progressThin: 4          // h-1
    readonly property int dotSize: 10              // h-2.5
    readonly property int dotSmall: 8              // h-2
    readonly property int borderWidth: 1
    readonly property int tabUnderline: 2
    readonly property int gateBar: 4               // border-l-4 on gate rows
    readonly property int shadowY: 4
    readonly property int shadowBlur: 12
    readonly property real shadowAlpha: 0.15
    readonly property int motionFast: 150
    readonly property int motion: 200
    readonly property int motionSlow: 300
    readonly property int breakpointSm: 640
    readonly property int breakpointMd: 768
    readonly property int breakpointLg: 1024
    readonly property int breakpointXl: 1280
    readonly property int sidebarAutoCollapse: 1000
    readonly property int compactHeight: 680
    readonly property int readerMeasure: 760
    readonly property int readerSize: 14
    readonly property bool reducedMotion: Quickshell.env('QROC_REDUCED_MOTION') === '1'
    readonly property int feedbackDuration: reducedMotion ? 0 : motionFast

    // ------------------------------------------------------------------
    // Legacy aliases: every token the pre-fidelity QML referenced.
    // Mapping fixes the surface inversion (React surfaces are lighter than
    // the page) and the accent hue without touching call sites.
    // ------------------------------------------------------------------
    readonly property color base: bgPrimary
    readonly property color panel: bgSecondary
    readonly property color raised: bgTertiary
    readonly property color text: textPrimary
    readonly property color muted: textMuted
    readonly property color line: border
    readonly property color edge: borderHover
    readonly property color accent: accentPrimary
    readonly property color accentForeground: textOnAccent
    readonly property color selection: Qt.alpha(accentPrimary, 0.16)
    readonly property color hover: bgTertiary
    readonly property color fieldSurface: bgPrimary
    readonly property color fieldLine: border
    readonly property color wash: Qt.alpha(accentPrimary, 0.10)
    readonly property color green: success
    readonly property color accentText: readableAcross(accentPrimary, [bgPrimary, bgSecondary])
    readonly property color dangerText: readableAcross(danger, [bgPrimary, bgSecondary, bgTertiary])
    readonly property color successText: readableAcross(success, [bgPrimary, bgSecondary, bgTertiary])
    readonly property bool light: t.light === true
    readonly property int cornerRadius: radius
    readonly property int chromeRadius: radius
    readonly property int actionHeight: controlHeightLg
    readonly property int denseRowHeight: rowHeight
    readonly property int twoLineRowHeight: docRowHeight
    readonly property int tabStripHeight: branchTabHeight
    readonly property int iconTargetSize: controlHeight
    readonly property int shellHeight: headerHeight
    readonly property int pageInset: s6
    readonly property int pageTitleSize: fsXl
    readonly property int sectionTitleSize: fsLg
    readonly property int titleSize: fsXl
    readonly property int fontSizeNormal: fsSm
    readonly property int fontSizeSecondary: fsXs
    readonly property int fontSizeSmall: fsXs
    readonly property int space4: s1
    readonly property int space8: s2
    readonly property int space12: s3
    readonly property int space16: s4
    readonly property int space24: s6

    // ------------------------------------------------------------------
    // Omarchy palette file: the same file Soliton's Theme reads.
    // ------------------------------------------------------------------
    property var colors: ({})
    readonly property string palettePath: Quickshell.env('QROC_THEME_FILE') || ((Quickshell.env('XDG_STATE_HOME') || Quickshell.env('HOME') + '/.local/state') + '/omarchy/current/theme/colors.toml')
    property FileView palette: FileView {
        path: theme.palettePath
        preload: true
        printErrors: false
        watchChanges: true
        onFileChanged: reload()
        onLoaded: theme.apply(text())
    }
    property Timer themeRefresh: Timer {
        interval: 2500
        repeat: true
        running: theme.palettePath.length > 0 && theme.themeName === 'system'
        onTriggered: theme.palette.reload()
    }
    function apply(raw) {
        var next = {}, pattern = /^\s*([a-z_]+)\s*=\s*["'](#[0-9a-fA-F]{6}|dark|light)["']/gm, match;
        while ((match = pattern.exec(raw)) !== null) next[match[1]] = match[2];
        if (next.background && next.foreground && JSON.stringify(next) !== JSON.stringify(colors)) colors = next;
    }

    // ------------------------------------------------------------------
    // Colour helpers (unchanged semantics)
    // ------------------------------------------------------------------
    function mixed(a, b, amount) {
        a = Qt.tint(a, 'transparent'); b = Qt.tint(b, 'transparent');
        return Qt.rgba(a.r + (b.r - a.r) * amount, a.g + (b.g - a.g) * amount, a.b + (b.b - a.b) * amount, 1);
    }
    function readable(value, background) { return readableAcross(value, [background]); }
    function readableAcross(value, backgrounds) {
        var c = Qt.tint(value, 'transparent');
        var target = foregroundFor(backgrounds[0]);
        for (var step = 0; step <= 20; ++step) {
            var candidate = mixed(c, target, step / 20);
            if (backgrounds.every(function(bg) { return contrast(bg, candidate) >= 4.5; })) return candidate;
        }
        return target;
    }
    function channel(value) { return value <= 0.03928 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4); }
    function luminance(value) { return 0.2126 * channel(value.r) + 0.7152 * channel(value.g) + 0.0722 * channel(value.b); }
    function contrast(background, foreground) {
        var a = luminance(background) + 0.05, b = luminance(foreground) + 0.05;
        return Math.max(a, b) / Math.min(a, b);
    }
    function foregroundFor(background) {
        var candidates = [bgPrimary, textPrimary, Qt.rgba(17 / 255, 18 / 255, 20 / 255, 1), Qt.rgba(247 / 255, 247 / 255, 247 / 255, 1)];
        var best = candidates[0], bestRatio = contrast(background, best);
        for (var i = 1; i < candidates.length; ++i) {
            var ratio = contrast(background, candidates[i]);
            if (ratio > bestRatio) { best = candidates[i]; bestRatio = ratio; }
        }
        return best;
    }
}
