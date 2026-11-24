import QtQuick
import ".."

// h-2.5 w-2.5 rounded-full domain colour.
Rectangle {
    property string domain: 'PRODUCT'
    property int size: Theme.dotSize
    width: size; height: size
    radius: size / 2
    color: Theme.domainColor(domain)
}
