import QtQuick
import ".."

// w-10 h-10 rounded-xl bg-[tone]/10 with a tone icon (modal headers, empty states).
Rectangle {
    id: tile
    property string icon: 'file'
    property color tone: Theme.accentPrimary
    property int size: Theme.iconXl
    property int iconSize: Theme.iconMd
    property int tileRadius: Theme.radiusLg
    width: size; height: size
    radius: tileRadius
    color: Qt.alpha(tone, 0.10)
    SIcon { anchors.centerIn: parent; name: tile.icon; size: tile.iconSize; color: tile.tone }
}
