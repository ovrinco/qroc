import QtQuick
import ".."
import QtQuick.Controls

// ModelSelectModal search: pl-10 pr-4 py-2 rounded-lg border bg-primary text-sm with a 16px search icon at left-3.
Field {
    id: control
    property string scopeName: "this collection"
    placeholderText: "Search"
    Accessible.name: "Search " + scopeName
    leftPadding: Theme.s3 + Theme.icon + Theme.s2
    rightPadding: control.text.length > 0 ? Theme.controlHeightSm + Theme.s1 : Theme.s3
    SIcon { x: Theme.s3; anchors.verticalCenter: parent.verticalCenter; name: "search"; size: Theme.icon; color: Theme.textMuted }
    SButton {
        objectName: "search-clear"
        anchors.right: parent.right; anchors.rightMargin: 2; anchors.verticalCenter: parent.verticalCenter
        size: 'sm'
        iconName: "close"; quiet: true; accessibleLabel: "Clear search"
        visible: control.text.length > 0
        onClicked: { control.clear(); control.textEdited(); control.forceActiveFocus(); }
    }
}
