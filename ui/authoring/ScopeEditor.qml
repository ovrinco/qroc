import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import ".."

// Branch scope editor: state pills, labelled fields (Nature, Rationale, Outputs, Omissions, Constraints), the
// structured preview and saved rows, and the action row.
ColumnLayout {
    id: root
    required property var controller
    spacing: Theme.s3
    function titleFor(id) {if(id==='PRD')return 'PRD';return String(id).toLowerCase().split('_').map(w=>w.charAt(0).toUpperCase()+w.slice(1)).join(' ');}
    function selected(id) {return controller.includedText.split(',').map(x=>x.trim().toUpperCase()).indexOf(id)>=0;}
    function toggleOutput(id,include) {var next=controller.includedText.split(',').map(x=>x.trim().toUpperCase()).filter(x=>x&&x!==id);if(include)next.push(id);controller.includedText=next.join(', ');}
    function omissions(map) {return Object.keys(map||{}).map(k=>({type:root.titleFor(k),reason:String(map[k])}));}
    component FieldLabel: Text {
        Layout.fillWidth: true
        font.family: Theme.font; font.pixelSize: Theme.fsSm; font.weight: Font.Medium; color: Theme.textSecondary
    }
    component FactRow: RowLayout {
        property string label: ''
        default property alias body: slot.data
        Layout.fillWidth: true; spacing: Theme.s3
        EyebrowLabel { text: parent.label; Layout.preferredWidth: 96; Layout.alignment: Qt.AlignTop; Layout.topMargin: 3 }
        Flow { id: slot; Layout.fillWidth: true; spacing: Theme.s2 }
    }

    Flow {
        Layout.fillWidth: true; Layout.preferredHeight: implicitHeight; spacing: Theme.s2
        Pill { text: root.controller.savedScope ? 'Scope · revision ' + root.controller.savedScope.revision : 'No scope'; tone: root.controller.savedScope ? Theme.success : Theme.textSecondary; neutral: !root.controller.savedScope }
        Pill { text: 'Sources · selected and verified'; neutral: true }
    }
    Rectangle {
        visible: !!root.controller.errorText
        Layout.fillWidth: true
        implicitHeight: errorLabel.implicitHeight + Theme.s3 * 2
        radius: Theme.radiusSm; color: Qt.alpha(Theme.danger, 0.10); border.width: Theme.borderWidth; border.color: Qt.alpha(Theme.danger, 0.4)
        Text { id: errorLabel; anchors.fill: parent; anchors.margins: Theme.s3; wrapMode: Text.Wrap; text: root.controller.errorText; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.danger }
    }
    ScrollView {
        id:scroll
        Layout.fillWidth:true;Layout.fillHeight:true;clip:true;contentWidth:availableWidth
        ColumnLayout {
            width:scroll.availableWidth;spacing:Theme.s4
            ColumnLayout {
                Layout.fillWidth: true; spacing: Theme.s2
                FieldLabel { text: 'Nature' }
                Choice {objectName:'qroc-scope-nature';Layout.fillWidth:true;enabled:root.controller.scopeAvailable;model:['internal','research','creative','external-product'];currentIndex:Math.max(0,model.indexOf(root.controller.nature));onActivated:root.controller.nature=model[currentIndex];Accessible.name:'Relevant scope nature'}
            }
            ColumnLayout {
                Layout.fillWidth: true; spacing: Theme.s2
                FieldLabel { text: 'Rationale' }
                Field {objectName:'qroc-scope-rationale';Layout.fillWidth:true;enabled:root.controller.scopeAvailable;text:root.controller.rationale;onTextEdited:root.controller.rationale=text;placeholderText:'Why is this scope relevant?';Accessible.name:'Scope rationale'}
            }
            ColumnLayout {
                Layout.fillWidth: true; spacing: Theme.s2
                FieldLabel { text: 'Outputs' }
                SCheckBox {objectName:'qroc-scope-defaults';enabled:root.controller.scopeAvailable;checked:root.controller.useDefaults;text:'Use suggested outputs for this nature';onToggled:root.controller.useDefaults=checked}
                Text {Layout.fillWidth:true;wrapMode:Text.Wrap;visible:!root.controller.useDefaults&&!!root.controller.catalog;text:'Choose the outputs you need';font.family:Theme.font;font.pixelSize:Theme.fsXs;color:Theme.textMuted}
                Rectangle {
                    visible: !root.controller.useDefaults && !!root.controller.catalog
                    Layout.fillWidth: true; Layout.preferredHeight: 160
                    color: Theme.bgPrimary; radius: Theme.radiusSm; border.width: Theme.borderWidth; border.color: Theme.border
                    ListView {
                        objectName:'qroc-scope-included';anchors.fill:parent;anchors.margins:Theme.s1;visible:!root.controller.useDefaults;clip:true
                        model:root.controller.catalog?root.controller.catalog.types:[];ScrollBar.vertical:SScrollBar {}
                        delegate:SCheckBox {required property var modelData;width:ListView.view.width;text:root.titleFor(modelData.id);checked:root.selected(modelData.id);onToggled:root.toggleOutput(modelData.id,checked);enabled:root.controller.scopeAvailable}
                    }
                }
                EmptyState {Layout.fillWidth:true;visible:!root.controller.catalog;icon:'file';card:false;title:'Document list unavailable'}
            }
            ColumnLayout {
                Layout.fillWidth: true; spacing: Theme.s2
                FieldLabel { text: 'Omissions' }
                Choice {id:omissionChoice;Layout.fillWidth:true;enabled:root.controller.scopeAvailable&&!!root.controller.catalog;textRole:'label';model:root.controller.catalog?root.controller.catalog.types.map(t=>({id:t.id,label:root.titleFor(t.id)})):[];Accessible.name:'Output to omit'}
                Field {id:omissionReason;objectName:'qroc-scope-excluded';Layout.fillWidth:true;enabled:root.controller.scopeAvailable;placeholderText:'Why is this output unnecessary?';Accessible.name:'Reason for omitting this output'}
                RowLayout {
                    Layout.fillWidth: true; spacing: Theme.s2
                    SButton {text:'Add omission';enabled:!!root.controller.catalog&&omissionChoice.currentIndex>=0&&!!omissionReason.text.trim();onClicked:{
                        var type=omissionChoice.model[omissionChoice.currentIndex].id;
                        var lines=root.controller.excludedText.split('\n').filter(line=>line.trim()&&line.split(':')[0].trim()!==type);
                        lines.push(type+': '+omissionReason.text.trim());root.controller.excludedText=lines.join('\n');omissionReason.text='';
                    }}
                    SButton {text:'Clear all';quiet:true;visible:!!root.controller.excludedText;onClicked:root.controller.excludedText=''}
                    Item { Layout.fillWidth: true }
                }
                Repeater {
                    model: root.controller.excludedText.split('\n').filter(line=>line.trim()).map(line=>({type:root.titleFor(line.split(':')[0]),reason:line.slice(line.indexOf(':')+1).trim()}))
                    delegate: RowLayout {
                        required property var modelData
                        Layout.fillWidth: true; spacing: Theme.s2
                        Pill { text: modelData.type; neutral: true }
                        Text { Layout.fillWidth: true; text: modelData.reason; wrapMode: Text.Wrap; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary }
                    }
                }
            }
            ColumnLayout {
                Layout.fillWidth: true; spacing: Theme.s2
                FieldLabel { text: 'Constraints' }
                STextArea {objectName:'qroc-scope-constraints';Layout.fillWidth:true;Layout.preferredHeight:90;enabled:root.controller.scopeAvailable;text:root.controller.constraintsText;onTextChanged:if(activeFocus)root.controller.constraintsText=text;placeholderText:'One constraint per line';wrapMode:TextEdit.Wrap;Accessible.name:'Independent scope constraints'}
            }
            Card {
                visible: !!root.controller.preview
                Layout.fillWidth: true; nested: true; padding: Theme.s3; spacing: Theme.s3
                LabelText {objectName:'qroc-scope-preview-standing';Layout.fillWidth:true;text:root.controller.preview?'Preview · '+root.controller.preview.included.length+' outputs':'';font.pixelSize:Theme.fsSm;font.weight:Font.DemiBold}
                FactRow { label: 'Outputs'
                    Repeater { model: root.controller.preview ? root.controller.preview.included : []; delegate: Pill { required property var modelData; text: root.titleFor(modelData); tone: Theme.accentPrimary } }
                }
                Repeater {
                    model: root.controller.preview ? root.omissions(root.controller.preview.excluded) : []
                    delegate: FactRow { required property var modelData; label: 'Omissions'
                        Pill { text: modelData.type; neutral: true }
                        Text { text: modelData.reason; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary }
                    }
                }
                Repeater {
                    model: root.controller.preview ? root.controller.preview.constraints : []
                    delegate: FactRow { required property var modelData; label: 'Constraints'
                        Text { text: modelData; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary }
                    }
                }
            }
            Card {
                visible: !!root.controller.savedScope
                Layout.fillWidth: true; nested: true; padding: Theme.s3; spacing: Theme.s3
                LabelText {objectName:'qroc-scope-saved-standing';Layout.fillWidth:true;text:root.controller.savedScope?'Saved revision '+root.controller.savedScope.revision:'';font.pixelSize:Theme.fsSm;font.weight:Font.DemiBold}
                FactRow { label: 'Outputs'
                    Repeater { model: root.controller.savedScope ? root.controller.savedScope.included : []; delegate: Pill { required property var modelData; text: root.titleFor(modelData); tone: Theme.accentPrimary } }
                }
                FactRow { label: 'Rationale'; visible: !!root.controller.savedScope && !!root.controller.savedScope.rationale
                    Text { text: root.controller.savedScope ? root.controller.savedScope.rationale : ''; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary }
                }
                Repeater {
                    model: root.controller.savedScope ? root.omissions(root.controller.savedScope.excluded) : []
                    delegate: FactRow { required property var modelData; label: 'Omissions'
                        Pill { text: modelData.type; neutral: true }
                        Text { text: modelData.reason; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary }
                    }
                }
                Repeater {
                    model: root.controller.savedScope ? root.controller.savedScope.constraints : []
                    delegate: FactRow { required property var modelData; label: 'Constraints'
                        Text { text: modelData; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary }
                    }
                }
                FactRow { label: 'Documents'
                    Repeater { model: root.controller.savedScope ? root.controller.savedScope.document_refs : []; delegate: Pill { required property var modelData; neutral: true; text: root.titleFor(modelData.document_type) + ' · revision ' + modelData.revision } }
                }
                FactRow { label: 'Reviews'
                    Repeater { model: root.controller.savedScope ? root.controller.savedScope.gate_reviews : []; delegate: Pill { required property var modelData; text: root.titleFor(modelData.document_type) + ' · revision ' + modelData.revision + ' · ' + modelData.decision; tone: modelData.decision === 'passed' ? Theme.success : Theme.danger } }
                }
            }
        }
    }
    Flow {
        Layout.fillWidth:true;Layout.preferredHeight:implicitHeight;spacing:Theme.s2
        SButton {objectName:'qroc-scope-preview';text:'Preview';enabled:root.controller.scopeAvailable&&root.controller.ownerSelected&&!root.controller.previewing&&!root.controller.saving;busy:root.controller.previewing;onClicked:root.controller.previewScope()}
        SButton {objectName:'qroc-scope-save';text:'Save scope';primary:true;enabled:root.controller.canSave;busy:root.controller.saving;onClicked:root.controller.saveScope()}
        SButton {objectName:'qroc-scope-reload';text:'Reload';quiet:true;enabled:root.controller.scopeAvailable&&root.controller.ownerSelected&&!root.controller.saving&&!root.controller.loadingScope;onClicked:root.controller.loadScope(true)}
        SButton {text:'Use saved';quiet:true;enabled:!!root.controller.savedScope&&!root.controller.saving;onClicked:root.controller.applySaved()}
    }
}
