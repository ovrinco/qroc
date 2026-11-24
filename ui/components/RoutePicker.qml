import QtQuick
import QtQuick.Controls
import ".."

// The brain for what is produced next, in the project header. Each row is a route of the app's table with
// its state in words; a route that is not ready is listed and cannot be chosen. Opening it reads state only.
Choice {
    id: picker
    property var routes: []
    signal refreshRequested()
    signal chosen(string name)
    signal emptyClicked()
    // A relay route is named by the model it carries: codex: is Codex, apikey: his paid OpenAI key, anthropic: his paid Anthropic key.
    function label(route) {
        if (route.kind === 'relay') {
            var m = String(route.model || ''), via = m.indexOf(':') > 0 ? m.slice(0, m.indexOf(':')) : '';
            return ({codex: 'Codex', apikey: 'OpenAI key', anthropic: 'Anthropic key'})[via] || 'Relay';
        }
        return ({local: 'Local model', openrouter: 'OpenRouter'})[route.kind] || route.name;
    }
    function stateWord(route) { return ({ready: 'Ready', 'key-missing': 'Needs a key', unreachable: 'Not reachable', 'not-loaded': 'Model not loaded'})[route.state] || ''; }
    readonly property var current: routes.find(r => r.selected) || null
    model: routes
    currentIndex: routes.findIndex(r => r.selected)
    implicitWidth: 216
    implicitHeight: Theme.controlHeightSm
    displayText: routes.length === 0 ? 'No route set' : current ? 'Route: ' + label(current) : 'Route'
    popup.onAboutToShow: picker.refreshRequested()
    popup.width: Math.max(picker.width, 240)
    onActivated: index => { var r = routes[index]; if (r && r.state === 'ready') picker.chosen(r.name); }
    MouseArea {
        anchors.fill: parent
        visible: picker.routes.length === 0
        cursorShape: Qt.PointingHandCursor
        onClicked: picker.emptyClicked()
    }
    delegate: ItemDelegate {
        id: row
        required property var modelData
        required property int index
        width: picker.popup.width - Theme.s1 * 2
        height: Theme.rowHeight
        enabled: modelData.state === 'ready'
        contentItem: Item {
            Text {
                anchors.left: parent.left; anchors.leftMargin: Theme.s3; anchors.verticalCenter: parent.verticalCenter
                text: picker.label(row.modelData)
                color: !row.enabled ? Theme.textMuted : row.modelData.selected ? Theme.accentPrimary : Theme.textPrimary
                font: picker.font
            }
            Text {
                anchors.right: parent.right; anchors.rightMargin: Theme.s3; anchors.verticalCenter: parent.verticalCenter
                text: picker.stateWord(row.modelData)
                color: row.modelData.state === 'ready' ? Theme.successText : Theme.textMuted
                font.family: Theme.font; font.pixelSize: Theme.fsXs
            }
        }
        background: Rectangle { radius: Theme.radiusMd; color: row.highlighted && row.enabled ? Theme.bgTertiary : 'transparent' }
        highlighted: picker.highlightedIndex === index
    }
    Accessible.name: 'Route'
}
