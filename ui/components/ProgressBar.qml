import QtQuick
import ".."

// h-1.5 rounded-full bg-tertiary track with a tone-coloured fill.
Rectangle {
    id: bar
    property real value: 0
    property color tone: Theme.accentPrimary
    property int thickness: Theme.progressHeight
    implicitWidth: 120
    implicitHeight: thickness
    color: Theme.bgTertiary
    radius: thickness / 2
    clip: true
    Rectangle {
        width: parent.width * Math.max(0, Math.min(1, bar.value))
        height: parent.height
        radius: parent.radius
        color: bar.tone
        Behavior on width { enabled: !Theme.reducedMotion; NumberAnimation { duration: Theme.motionSlow; easing.type: Easing.OutCubic } }
    }
}
