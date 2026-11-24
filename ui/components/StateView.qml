import QtQuick
import ".."
import QtQuick.Controls
import QtQuick.Layouts

// EmptyState look for the r18 state screens: centred card with an icon tile, title, detail and one action.
Item {
    id: state
    property string title: ''
    property string detail: ''
    property string actionText: ''
    property bool error: false
    property bool pending: false
    signal actionRequested()
    implicitHeight: body.implicitHeight
    EmptyState {
        id: body
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        icon: state.error ? 'warning' : state.pending ? 'clock' : 'file'
        tone: state.error ? Theme.danger : state.pending ? Theme.pending : Theme.accentPrimary
        title: state.title
        detail: state.detail
        actionText: state.actionText
        actionIcon: ''
        onActionRequested: state.actionRequested()
    }
    Accessible.role: Accessible.StaticText
    Accessible.name: title + (detail ? '. '+detail : '')
}
