import QtQuick
import QtQuick.Layouts
import ".."

// rounded-lg border border-[--border-color] bg-[--bg-secondary] p-4.
// nested: the bg-primary tile used inside a card (Metric, GateMetric, domain preview).
Rectangle {
    id: card
    property int padding: Theme.s4
    property bool nested: false
    property bool hoverable: false
    property bool pressable: false
    readonly property bool hovered: hoverHandler.hovered
    signal clicked()
    default property alias content: body.data
    property alias spacing: body.spacing
    color: nested ? Theme.bgPrimary : Theme.bgSecondary
    radius: nested ? Theme.radiusSm : Theme.radius
    border.width: Theme.borderWidth
    border.color: (hoverable || pressable) && hovered ? Theme.accentPrimary : Theme.border
    implicitWidth: body.implicitWidth + padding * 2
    implicitHeight: body.implicitHeight + padding * 2
    HoverHandler { id: hoverHandler; enabled: card.hoverable || card.pressable }
    TapHandler { enabled: card.pressable; onTapped: card.clicked() }
    ColumnLayout {
        id: body
        anchors.fill: parent
        anchors.margins: card.padding
        spacing: Theme.s3
    }
    Behavior on border.color { enabled: !Theme.reducedMotion; ColorAnimation { duration: Theme.motion } }
}
