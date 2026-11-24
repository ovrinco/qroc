import QtQuick
import QtQuick.Window
import QtTest
import Quickshell
import "../../ui"

// Screenshot driver (QROC_SHOT=landing|dashboard|product|kb|new-project|add-branch|phod|creative|settings|theme-cycle).
// theme-cycle: opens Settings, presses Ctrl+Shift+T once (the App.tsx shortcut) and captures with the toast showing.
// Waits for the bridge, drives the shell through the wave-3 contract functions, then logs T13_SHOT_READY.
// With QROC_SHOT_FILE=<path.png> it also grabs the window content to that file (works headlessly with
// QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software) and quits. tools/shot.sh wraps both modes.
Item {
    id: shot
    property var shell
    readonly property string mode: Quickshell.env('QROC_SHOT') || ''
    readonly property string file: Quickshell.env('QROC_SHOT_FILE') || ''
    readonly property string size: Quickshell.env('QROC_SHOT_SIZE') || ''   // e.g. 941x508 to capture the tiled case
    function call(target, name, arg) {
        if (target && typeof target[name] === 'function') { target[name](arg); return true; }
        console.warn('T13_SHOT_MISSING', name);
        return false;
    }
    function drive() {
        var w = shell.workspace, win = w.Window.window;
        if (size.indexOf('x') > 0) { win.width = parseInt(size.split('x')[0]); win.height = parseInt(size.split('x')[1]); }
        switch (mode) {
        case 'landing': call(w, 'showLanding'); break;
        case 'dashboard': call(w, 'showDashboard'); break;
        case 'product': call(w, 'showDomain', 'PRODUCT'); break;
        case 'kb': call(w, 'showKnowledge'); break;
        case 'new-project': call(w, 'openNewProject'); break;
        case 'add-branch': call(w, 'openNewBranch'); break;
        case 'phod': shell.showPage('phod'); break;
        case 'creative': shell.showPage('creative'); break;
        case 'settings': shell.showPage('settings'); break;
        case 'sidebar-toggle': call(w, 'showDashboard'); shell.sidebar.toggleCollapsed(); break;
        case 'sidebar-toggle2': call(w, 'showDashboard'); shell.sidebar.toggleCollapsed(); shell.sidebar.toggleCollapsed(); break;
        case 'theme-cycle': shell.showPage('settings'); win.requestActivate(); grabber.keyClick(Qt.Key_T, Qt.ControlModifier | Qt.ShiftModifier); break;
        default: break;
        }
        settle.start();
    }
    // QtTest's grabImage renders the window synchronously (also under QT_QPA_PLATFORM=offscreen), exactly like
    // tests/T08/InstalledJourney.qml does; Item.grabToImage never completes without an exposed window.
    TestCase { id: grabber; when: false; name: 'T13Shot' }
    function capture() {
        var win = shell.workspace.Window.window;
        console.log('T13_SHOT_READY', mode, win.width, win.height, shell.workspace.width, shell.workspace.height);
        if (!file) return;
        try {
            var image = grabber.grabImage(win.contentItem);
            image.save(file);
            console.log('T13_SHOT_SAVED', file, image.width, image.height);
        } catch (e) {
            console.log('T13_SHOT_SAVE_FAILED', file, String(e));
        }
        Qt.quit();
    }
    Timer { id: settle; interval: 700; onTriggered: shot.capture() }
    Timer { id: wait; interval: 1200; onTriggered: shot.drive() }
    Connections { target: Backend; function onReadyChanged() { if (Backend.ready) wait.restart() } }
    Component.onCompleted: if (Backend.ready) wait.restart()
}
