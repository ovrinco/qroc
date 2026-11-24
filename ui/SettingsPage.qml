import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io

// Settings page (sidebar footer): appearance, generation, workspace, PHOD. Read-only except the theme.
Item {
    id: page
    property var workspace: null
    readonly property var authoring: page.workspace ? page.workspace.authoringState : null
    readonly property var provider: page.authoring ? page.authoring.provider : null
    readonly property string resourceRoot: Quickshell.env('QROC_RESOURCE_ROOT') || ''
    readonly property string checkerDigest: (Quickshell.env('QROC_PHOD_ADMITTED_SHA256') || '').slice(0, 12)
    property string releaseId: ''
    readonly property bool sidebarCollapsed: !!(Backend.prefs && Backend.prefs.sidebarCollapsed === true)

    // Status in words: a line that carries an engine reason, a variable name or a path is not shown as it is.
    function plain(text) {
        var t = String(text || '');
        return !t || /capability-unavailable|[A-Z]{2,}_[A-Z]|\/|[a-z]+-[a-z]+:/.test(t) ? '' : t;
    }
    readonly property string generationWords: {
        if (!page.provider) return page.plain(page.authoring ? page.authoring.providerStatus : '') || 'Reading the route';
        if (page.provider.available) return 'Ready to write documents; the route has not been tried yet';
        var reason = String(page.provider.reason || '');
        if (/not declared|no route/i.test(reason)) return 'No route set, so documents cannot be written yet';
        if (/refused|unreachable|connect/i.test(reason)) return 'Not reachable, so documents cannot be written yet';
        return 'Not available, so documents cannot be written yet';
    }
    function readRelease(text) {
        try { page.releaseId = String(JSON.parse(text).release_id || '') } catch (error) { page.releaseId = '' }
    }
    FileView {
        path: page.resourceRoot ? page.resourceRoot + '/release.json' : ''
        printErrors: false
        onLoaded: page.readRelease(text())
    }

    component RowLabel: Text {
        Layout.preferredWidth: 120
        Layout.alignment: Qt.AlignTop
        font.family: Theme.font
        font.pixelSize: Theme.fsSm
        color: Theme.textSecondary
    }
    component RowValue: Text {
        Layout.fillWidth: true
        font.family: Theme.font
        font.pixelSize: Theme.fsSm
        color: Theme.textPrimary
        wrapMode: Text.Wrap
    }
    component MonoValue: Text {
        Layout.fillWidth: true
        font.family: Theme.mono
        font.pixelSize: Theme.fsXs
        color: Theme.textPrimary
        elide: Text.ElideMiddle
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        AppHeader { Layout.fillWidth: true; title: 'Settings' }
        ScrollView {
            id: scroll
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: availableWidth
            contentHeight: body.implicitHeight + Theme.s6 * 2
            clip: true
            ColumnLayout {
                id: body
                x: Math.max(Theme.s6, (scroll.availableWidth - Theme.contentMaxWidth) / 2)
                y: Theme.s6
                width: Math.min(Theme.contentMaxWidth, scroll.availableWidth - Theme.s6 * 2)
                spacing: Theme.s5
                Card {
                    Layout.fillWidth: true
                    padding: Theme.s5
                    EyebrowLabel { text: 'Appearance' }
                    Text { text: 'Theme'; font.family: Theme.font; font.pixelSize: Theme.fsBase; font.weight: Font.DemiBold; color: Theme.textPrimary }
                    Flow {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        Repeater {
                            model: Theme.themeNames
                            delegate: SButton {
                                required property string modelData
                                text: modelData === 'system' ? 'Follow desktop' : modelData.charAt(0).toUpperCase() + modelData.slice(1)
                                primary: Theme.themeName === modelData
                                onClicked: Theme.setTheme(modelData)
                            }
                        }
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        RowLabel { text: 'Sidebar' }
                        Pill { text: page.sidebarCollapsed ? 'Collapsed' : 'Expanded'; neutral: true }
                    }
                }
                Card {
                    Layout.fillWidth: true
                    padding: Theme.s5
                    EyebrowLabel { text: 'Keys' }
                    Repeater {
                        model: [
                            ['Ctrl+Shift+T', 'Cycles the theme'],
                            ['Left and Right', 'Pipeline: move the selection along an edge, to an input or to a dependent'],
                            ['Up and Down', 'Pipeline: move the selection within a column'],
                            ['Enter', 'Pipeline: open the selected node in the reader'],
                            ['Escape', 'Pipeline: return from the reader to the graph'],
                            ['Up and Down', 'Media project: move the selection in the list of revisions or versions'],
                            ['Enter', 'Media project: open the selected row in the reader'],
                            ['Escape', 'Media project: return to the list'],
                            ['Ctrl+S', 'Media project: save a revision']
                        ]
                        delegate: RowLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            spacing: Theme.s2
                            RowLabel { text: modelData[0] }
                            RowValue { text: modelData[1] }
                        }
                    }
                }
                Card {
                    Layout.fillWidth: true
                    padding: Theme.s5
                    EyebrowLabel { text: 'Generation' }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        RowLabel { text: 'Provider' }
                        Pill { text: page.provider && page.provider.available ? 'Configured' : 'Not configured'; tone: page.provider && page.provider.available ? Theme.success : Theme.pending }
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        RowLabel { text: 'Model' }
                        RowValue { text: page.provider && page.provider.model ? Theme.modelLabel(page.provider.model) : '—' }
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        RowLabel { text: 'Runs on' }
                        RowValue { text: !page.provider || !page.provider.available ? '—' : ({apikey: 'OpenAI key', anthropic: 'Anthropic key'})[String(page.provider.model || '').split(':')[0]] || 'This computer' }
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        RowLabel { text: 'Provider status' }
                        RowValue { text: page.generationWords }
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        RowLabel { text: 'Document types' }
                        RowValue { text: page.authoring && page.authoring.catalog ? 'Available' : 'Not available yet' }
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        RowLabel { text: 'Market review' }
                        RowValue { text: page.authoring && page.authoring.semanticProfile ? 'Available; a person still judges each review' : 'Not available' }
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        RowLabel { text: 'Scope' }
                        RowValue { text: page.plain(page.authoring ? page.authoring.scopeStatus : '') || 'Not available' }
                    }
                }
                Card {
                    Layout.fillWidth: true
                    padding: Theme.s5
                    EyebrowLabel { text: 'Workspace' }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        RowLabel { text: 'Projects' }
                        RowValue { text: 'Kept on this computer; open a project\'s folder from its header menu' }
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        RowLabel { text: 'Release' }
                        RowValue { text: page.releaseId.length > 0 ? 'Installed release' : 'Working copy, not an installed release' }
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        RowLabel { text: 'Engine' }
                        RowValue { text: Backend.failed ? 'Stopped; restart QROC' : Backend.ready ? 'Running' : 'Starting' }
                    }
                }
                Card {
                    Layout.fillWidth: true
                    padding: Theme.s5
                    EyebrowLabel { text: 'Phod' }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        RowLabel { text: 'Checker' }
                        Pill { text: page.checkerDigest.length > 0 ? 'Ready' : 'Not available'; tone: page.checkerDigest.length > 0 ? Theme.success : Theme.pending }
                    }
                }
            }
        }
    }
}
