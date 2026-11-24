import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell

// React App.tsx shell: h-screen flex; fixed sidebar (256 / 64 collapsed); main column with the page's own header.
// Contract preserved from r18: workspace/creativeView/phodView/selectedView aliases, view indices 0..2,
// the two QA Loaders, the T01_SHELL_CONTEXT log line, the window title and onClosed: Qt.quit().
ShellRoot {
    id: shell
    property alias workspace: qroc
    property alias creativeView: creative
    property alias phodView: phod
    property alias selectedView: views.currentIndex
    property alias sidebar: sidebar
    readonly property QtObject backend: Backend
    readonly property QtObject theme: Theme
    readonly property var pageNames: ['workspace', 'phod', 'creative', 'settings']
    property string themeToast: ''
    function showPage(name) { var i = pageNames.indexOf(String(name)); if (i >= 0) views.currentIndex = i; }
    // App.tsx Ctrl+Shift+T: cycle obsidian → slate → light → parchment and show the theme name as a toast.
    function cycleTheme() {
        var next = Theme.nextTheme(Theme.themeName);
        Theme.setTheme(next);
        themeToast = Theme.themeLabel(next);
        toastTimer.restart();
    }
    Component.onCompleted: {
        console.log("T01_SHELL_CONTEXT", Quickshell.env("QROC_NATIVE_SMOKE"), Backend.ownerRoot, Backend.executable)
        var start = Quickshell.env('QROC_START_VIEW'); if (start) showPage(start)
    }
    FloatingWindow {
        id: window
        visible: true
        implicitWidth: 1600
        implicitHeight: 1000
        title: 'QROC'
        onClosed: Qt.quit()
        color: Theme.bgPrimary
        // Sidebar.tsx: the sidebar is w-64, or w-16 only when the user collapsed it; there is no width-derived rail.
        property bool userCollapsed: false
        Connections {
            target: Backend
            function onRestoredChanged() { if (Backend.restored && Backend.prefs) window.userCollapsed = Backend.prefs.sidebarCollapsed === true }
        }
        Item {
            Shortcut { sequence: 'Ctrl+Shift+T'; context: Qt.WindowShortcut; onActivated: shell.cycleTheme() }
        }
        Timer { id: toastTimer; interval: 1200; onTriggered: shell.themeToast = '' }
        NoticePopup {
            scope: window.contentItem
            text: shell.themeToast
            error: false
            dismissible: false
            iconName: Theme.themeIcon(Theme.themeName)
            onDismissed: shell.themeToast = ''
        }
        RowLayout {
            anchors.fill: parent
            spacing: 0
            Sidebar {
                id: sidebar
                Layout.fillHeight: true
                Layout.preferredWidth: implicitWidth
                Layout.minimumWidth: implicitWidth
                Layout.maximumWidth: implicitWidth
                projects: qroc.projects
                activeProjectId: qroc.projectId
                projectOpen: !!qroc.project
                currentPage: shell.pageNames[views.currentIndex]
                collapsed: window.userCollapsed
                onSelectProject: id => { shell.showPage('workspace'); if (typeof qroc.showProject === 'function') qroc.showProject(id); else qroc.loadProject(id) }
                onNewProject: { shell.showPage('workspace'); if (typeof qroc.openNewProject === 'function') qroc.openNewProject() }
                onGoHome: { shell.showPage('workspace'); if (typeof qroc.showLanding === 'function') qroc.showLanding() }
                onSelectPage: name => shell.showPage(name)
                // The chevron and the rail mark toggle at every window width; the choice persists as ui.sidebarCollapsed.
                onToggleCollapsed: { window.userCollapsed = !window.userCollapsed; Backend.setting('sidebarCollapsed', window.userCollapsed) }
            }
            StackLayout {
                id: views
                Layout.fillWidth: true
                Layout.fillHeight: true
                QrocWorkspace { id: qroc; onSettingsRequested: shell.showPage('settings') }
                ModulePhod {
                    id: phod
                    function refresh() { Backend.send('phod-list',{},function(r,ok){if(ok){phod.moduleInfo=r;phod.jobs=r.jobs||[]}}) }
                    onRunRequested: (action,args) => Backend.send('phod-'+action,args,function(r,ok,error){if(ok){phod.selectedJob=r;phod.refresh()}else console.warn(error)})
                    onDraftRequested: args => Backend.send('phod-draft-save',args,function(r,ok,error){if(!ok)console.warn(error);else phod.refresh()})
                    onInspectRequested: id => Backend.send('phod-job-status',{id:id},function(r,ok){if(ok)phod.selectedJob=r})
                    Component.onCompleted: refresh()
                    Timer { interval: 1000; repeat: true; running: views.currentIndex === 1; onTriggered: phod.refresh() }
                }
                CreativeWorkspace { id: creative; projectId: qroc.projectId; branchId: qroc.branchId; projectTitle: qroc.project ? String(qroc.project.title || '') : ''; branchName: qroc.activeBranch ? String(qroc.activeBranch.name || '') : '' }
                SettingsPage { id: settings; workspace: qroc }
            }
        }
    }
    // Owned native harness runs through the same Backend and preserved view.
    Loader { active: Quickshell.env('QROC_NATIVE_SMOKE') === '1'; source: active ? '../tests/T01/NativeSmoke.qml' : ''; onLoaded: item.workspace = qroc }
    Loader { active: Quickshell.env('QROC_RELEASE_QA') === '1'; source: active ? '../tests/T08/InstalledJourney.qml' : ''; onLoaded: item.shell = shell }
    Loader { active: (Quickshell.env('QROC_PROBE') || '') !== ''; source: active ? Quickshell.env('QROC_PROBE') : ''; onLoaded: item.shell = shell }
    Loader { active: (Quickshell.env('QROC_SHOT') || '') !== ''; source: active ? '../tests/T13/Shot.qml' : ''; onLoaded: item.shell = shell }
}
