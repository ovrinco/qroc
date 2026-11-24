import QtQuick
import QtQuick.Layouts
import "pipeline/PipelineGraph.js" as Graph

// Gate review body (GateReviewModal look): header, score ring, criteria rows, reason, recorded review, semantic
// standing, evidence and the Pass / Fail / Advance buttons. The heading must stay a direct child of `content`,
// which is a direct child of the root (the installed journey walks heading.parent.parent).
Rectangle {
    id: root
    property var result: null
    property var review: null
    property string nextDomain: ''
    property string standing: ''
    property int revision: 0
    property var semanticInspection: null
    property bool canReview: false
    property bool actionBusy: false
    signal reviewRequested(string decision)
    signal advanceRequested()
    // Revise from gate (Gates 1-3): the engine's record of the last press, whether the press would be accepted,
    // the round in progress and the engine's refusal as it words it.
    property bool revisable: false
    property bool reviseEnabled: false
    property var reviseRecord: null
    property string roundLabel: ''
    property string reviseRefusal: ''
    // The questions only he can answer that hold this gate, as the engine words them.
    property var openQuestions: []
    property string answerError: ''
    // The question being answered, by its id, or by its words where the gate keeps no record yet.
    property string answeringId: ''
    function questionKey(q) { return q.id || q.question || ''; }
    signal questionAnswerRequested(var question, string text)
    onOpenQuestionsChanged: if (!root.openQuestions.some(q => root.questionKey(q) === root.answeringId)) root.answeringId = ''
    // The engine's own count: the last call is over, waits for no press, and has used every round it allows.
    readonly property bool roundsSpent: !!root.reviseLatest && !root.reviseRunning && root.reviseLatest.state !== 'running'
        && !root.reviseLatest.awaiting_press && (root.reviseLatest.rounds || []).length >= (root.reviseRecord.max_rounds || root.reviseLatest.max_rounds || 2)
    // Answers he saved after the last call ended: what a new round would bring that the spent rounds did not have.
    readonly property bool answeredSinceLastCall: root.roundsSpent && (root.reviseRecord.questions || []).some(q =>
        q.standing === 'answered' && Number(q.settled_at) > Number(root.reviseLatest.finished_at || 0))
    signal reviseRequested()
    readonly property var reviseLatest: root.reviseRecord ? root.reviseRecord.latest || null : null
    readonly property bool reviseRunning: !!root.reviseRecord && !!root.reviseRecord.running
    readonly property string reviseRound: root.roundLabel || (root.reviseRunning && root.reviseLatest
        ? 'round ' + (root.reviseLatest.rounds || []).length + ' of ' + (root.reviseRecord.max_rounds || 2) : '')
    // Names people read: a document type as its title, an engine key as words.
    function titleFor(id) { if (id === 'PRD') return 'PRD'; return String(id || '').toLowerCase().split(/[_-]/).filter(w => w).map(w => w.charAt(0).toUpperCase() + w.slice(1)).join(' '); }
    // A round without a score: running while the call runs, else the record's own state in one word.
    function roundStateWord(round) {
        if (root.reviseRunning) return 'Running';
        var state = String(round.status || (root.reviseLatest ? root.reviseLatest.state : '') || '');
        return state ? state.charAt(0).toUpperCase() + state.slice(1).toLowerCase() : 'Stopped';
    }
    function gateWord(status) { return ({passed: 'Scored a pass', failed: 'Failed', pending: 'Pending review', incomplete: 'Gate incomplete', not_applicable: 'No gate'})[status] || ''; }
    readonly property bool invalidSources: !!root.semanticInspection && !root.semanticInspection.valid
    readonly property string status: root.result ? (root.result.status || 'pending') : 'pending'
    readonly property color tone: invalidSources ? Theme.danger : status === 'passed' ? Theme.success : status === 'failed' ? Theme.danger : Theme.warning
    readonly property bool hasScore: !!root.result && root.result.score !== null && root.result.score !== undefined
    // The gate's name ("Gate 1: idea viability"), never its type key.
    function gateName() { return root.result && root.result.document_type ? Graph.titleOf(String(root.result.document_type)) : ''; }
    function toneFor(score, max) { return score * 100 >= max * 70 ? Theme.success : score * 100 >= max * 40 ? Theme.warning : Theme.danger; }
    color: 'transparent'
    implicitHeight: content.implicitHeight + Theme.s4
    visible: result !== null

    ColumnLayout {
        id: content
        anchors.fill: parent
        anchors.margins: Theme.s2
        spacing: Theme.s4
        LabelText {
            objectName: 'qroc-gate-heading'
            Layout.fillWidth: true
            leftPadding: Theme.iconMd + Theme.s3
            text: root.invalidSources ? 'Review unavailable · sources changed' : root.gateName() || 'Gate review'
            color: Theme.textPrimary
            font.pixelSize: Theme.fsXl
            font.weight: Font.DemiBold
            SIcon {
                x: 0; anchors.verticalCenter: parent.verticalCenter
                size: Theme.iconMd
                name: root.status === 'passed' && !root.invalidSources ? 'check-circle' : 'alert'
                color: root.tone
            }
        }
        Flow {
            Layout.fillWidth: true; Layout.preferredHeight: implicitHeight; spacing: Theme.s2
            Pill { visible: root.hasScore; text: 'Score ' + (root.result ? root.result.score : '') + (root.result && root.result.threshold ? ' · threshold ' + root.result.threshold : ''); mono: true; tone: root.tone }
            Pill { visible: !root.hasScore && !!root.result; text: 'No score'; neutral: true }
            Pill { text: 'Revision ' + root.revision + ' · ' + root.standing; tone: root.canReview ? Theme.textSecondary : Theme.danger; neutral: root.canReview }
            Pill { visible: !!root.result && !!root.result.semantic; text: 'Market review'; neutral: true }
            Pill { visible: !!root.result && !!root.result.semantic; text: '3 of 43 types'; neutral: true; mono: true }
        }
        Rectangle {
            visible: !!root.result && !root.invalidSources
            Layout.alignment: Qt.AlignHCenter
            Layout.preferredWidth: 128; Layout.preferredHeight: 128
            radius: 64
            color: Qt.alpha(root.tone, 0.12)
            border.width: 4; border.color: root.tone
            Text {
                anchors.centerIn: parent
                text: root.hasScore ? root.result.score : '–'
                font.family: Theme.font; font.pixelSize: 36; font.weight: Font.Bold; color: root.tone
            }
        }
        Text {
            visible: !!root.result && !root.invalidSources
            Layout.alignment: Qt.AlignHCenter
            text: (root.status === 'passed' ? (root.review ? 'Gate passed' : 'Scored a pass · waiting for your review') : root.status === 'failed' ? 'Gate failed' : root.status.charAt(0).toUpperCase() + root.status.slice(1))
                  + (root.reviseRound ? ' · ' + root.reviseRound : '')
            font.family: Theme.font; font.pixelSize: Theme.fsLg; font.weight: Font.Medium; color: root.tone
        }
        // What holds the gate: each question only he can answer, word for word, with the document it bears on.
        ColumnLayout {
            objectName: 'qroc-gate-questions'
            visible: root.openQuestions.length > 0 && !root.invalidSources
            Layout.fillWidth: true; spacing: Theme.s2
            EyebrowLabel { text: root.openQuestions.length === 1 ? 'One question only you can answer' : root.openQuestions.length + ' questions only you can answer' }
            Text {
                Layout.fillWidth: true; wrapMode: Text.Wrap
                text: root.openQuestions.length === 1 ? 'The gate goes no further until it is answered.' : 'The gate goes no further until they are answered.'
                font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary
            }
            Text {
                objectName: 'qroc-gate-answer-error'
                visible: !!root.answerError
                Layout.fillWidth: true; wrapMode: Text.Wrap
                text: root.answerError
                font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.dangerText
            }
            Repeater {
                model: root.openQuestions
                delegate: Card {
                    id: questionCard
                    required property var modelData
                    required property int index
                    Layout.fillWidth: true; nested: true; padding: Theme.s3; spacing: Theme.s2
                    Text { Layout.fillWidth: true; text: (index + 1) + '. ' + modelData.question; wrapMode: Text.Wrap; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textPrimary }
                    readonly property bool answering: root.answeringId === root.questionKey(modelData)
                    Flow {
                        Layout.fillWidth: true; spacing: Theme.s2
                        Pill { visible: !!modelData.document; text: root.titleFor(modelData.document); neutral: true }
                        Pill { visible: !!modelData.criterion; text: String(modelData.criterion || '').charAt(0).toUpperCase() + String(modelData.criterion || '').slice(1); neutral: true }
                    }
                    STextArea {
                        id: answerBox
                        visible: questionCard.answering
                        Layout.fillWidth: true; Layout.preferredHeight: 96
                        wrapMode: TextEdit.Wrap
                        placeholderText: 'Your answer'
                        Accessible.name: 'Answer'
                    }
                    Flow {
                        Layout.fillWidth: true; spacing: Theme.s2
                        SButton {
                            visible: !questionCard.answering
                            text: 'Answer'; secondary: true
                            enabled: !root.actionBusy && !root.reviseRunning
                            onClicked: { answerBox.text = ''; root.answeringId = root.questionKey(modelData); answerBox.forceActiveFocus(); }
                        }
                        SButton {
                            visible: questionCard.answering
                            text: 'Save answer'; primary: true
                            enabled: !!answerBox.text.trim() && !root.actionBusy
                            onClicked: root.questionAnswerRequested(modelData, answerBox.text.trim())
                        }
                        SButton {
                            visible: questionCard.answering
                            text: 'Cancel'; secondary: true
                            onClicked: root.answeringId = ''
                        }
                    }
                }
            }
        }
        EyebrowLabel { visible: criteria.count > 0; text: 'Criteria' }
        ColumnLayout {
            Layout.fillWidth: true; spacing: Theme.s2
            Repeater {
                id: criteria
                model: root.result && root.result.criteria && !root.invalidSources ? Object.keys(root.result.criteria) : []
                delegate: Rectangle {
                    required property string modelData
                    readonly property var entry: root.result.criteria[modelData]
                    Layout.fillWidth: true
                    implicitHeight: Theme.controlHeight + Theme.s2
                    radius: Theme.radiusMd; color: Theme.bgPrimary
                    border.width: Theme.borderWidth; border.color: Theme.border
                    RowLayout {
                        anchors.fill: parent; anchors.leftMargin: Theme.s3; anchors.rightMargin: Theme.s3; spacing: Theme.s3
                        Text { Layout.fillWidth: true; text: modelData; elide: Text.ElideRight; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary }
                        ProgressBar { Layout.preferredWidth: 96; value: entry.max ? entry.score / entry.max : 0; tone: root.toneFor(entry.score, entry.max); thickness: Theme.progressHeight }
                        Pill { Layout.preferredWidth: 56; text: entry.score + '/' + entry.max; mono: true; tone: root.toneFor(entry.score, entry.max) }
                    }
                }
            }
        }
        Text {
            visible: !!root.result && !!root.result.reason
            Layout.fillWidth: true
            text: root.result ? root.result.reason : ''
            wrapMode: Text.Wrap
            font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary
        }
        // The rounds of the last revision from this gate, one row each; the end shows the last score and reason.
        ColumnLayout {
            objectName: 'qroc-gate-rounds'
            visible: !!root.reviseLatest && (root.reviseLatest.rounds || []).length > 0
            Layout.fillWidth: true; spacing: Theme.s1
            Repeater {
                model: root.reviseLatest ? root.reviseLatest.rounds || [] : []
                delegate: Text {
                    required property var modelData
                    Layout.fillWidth: true
                    text: 'Round ' + modelData.round + ' · ' + (modelData.score !== null && modelData.score !== undefined
                          ? modelData.score + ' of 100 · ' + (root.gateWord(modelData.status) || modelData.status) : root.roundStateWord(modelData))
                    font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary
                    elide: Text.ElideRight
                }
            }
            Text {
                visible: !root.reviseRunning && !!root.reviseLatest && !!root.reviseLatest.last
                Layout.fillWidth: true; wrapMode: Text.Wrap
                text: root.reviseLatest && root.reviseLatest.last
                      ? 'Last score ' + root.reviseLatest.last.score + ' of 100 · threshold ' + root.reviseLatest.last.threshold
                        + (root.reviseLatest.last.reason ? ' · ' + root.reviseLatest.last.reason : '') : ''
                font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textPrimary
            }
            Text {
                visible: !root.reviseRunning && !!root.reviseLatest && !!root.reviseLatest.error
                Layout.fillWidth: true; wrapMode: Text.Wrap
                text: root.reviseLatest ? root.reviseLatest.error || '' : ''
                font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.dangerText
            }
        }
        Text {
            objectName: 'qroc-gate-revise-refusal'
            visible: !!root.reviseRefusal
            Layout.fillWidth: true; wrapMode: Text.Wrap
            text: root.reviseRefusal
            font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.dangerText
        }
        Flow {
            visible: !!root.review
            Layout.fillWidth: true; Layout.preferredHeight: implicitHeight; spacing: Theme.s2
            Pill { text: root.review ? 'Recorded ' + root.review.decision + ' · revision ' + root.review.revision : ''; tone: root.review && root.review.decision === 'passed' ? Theme.success : Theme.danger }
            Pill { visible: false; text: root.review && root.review.semantic_inputs ? root.review.semantic_inputs.profile + ' · ' + root.review.semantic_inputs.standing : ''; neutral: true }
        }
        Text {
            objectName: 'qroc-semantic-standing'
            Layout.fillWidth: true; wrapMode: Text.Wrap
            visible: !!root.result && (!!root.result.semantic || root.invalidSources)
            text: root.semanticInspection && root.semanticInspection.valid ? 'The market review quotes the saved documents; a person still judges it' : 'The market review does not match the saved documents'
            font.family: Theme.font; font.pixelSize: Theme.fsSm
            color: root.semanticInspection && root.semanticInspection.valid ? Theme.textSecondary : Theme.danger
        }
        EyebrowLabel { visible: criteriaEvidence.count > 0; text: 'Evidence' }
        Repeater {
            id: criteriaEvidence
            model: root.semanticInspection && root.semanticInspection.valid && root.semanticInspection.full ? root.semanticInspection.criteria : []
            delegate: Card {
                required property var modelData
                Layout.fillWidth: true; nested: true; padding: Theme.s3; spacing: Theme.s2
                RowLayout {
                    Layout.fillWidth: true; spacing: Theme.s2
                    Text { Layout.fillWidth: true; text: modelData.model.name; wrapMode: Text.Wrap; font.family: Theme.font; font.pixelSize: Theme.fsSm; font.weight: Font.DemiBold; color: Theme.textPrimary }
                    Pill { text: modelData.score + '/' + modelData.max; mono: true; tone: root.toneFor(modelData.score, modelData.max) }
                }
                Text { Layout.fillWidth: true; text: modelData.feedback; wrapMode: Text.Wrap; font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary }
                Repeater {
                    model: modelData.evidence
                    delegate: ColumnLayout {
                        required property var modelData
                        Layout.fillWidth: true; spacing: 2
                        Flow {
                            Layout.fillWidth: true; spacing: Theme.s2
                            Pill { text: 'Revision ' + modelData.revision; neutral: true }
                        }
                        Text { Layout.fillWidth: true; text: '“' + modelData.quote + '”'; wrapMode: Text.Wrap; font.family: Theme.font; font.pixelSize: Theme.fsXs; font.italic: true; color: Theme.textMuted }
                    }
                }
            }
        }
        Repeater {
            model: root.semanticInspection && root.semanticInspection.valid && root.semanticInspection.full ? root.semanticInspection.consistency : []
            delegate: Card {
                required property var modelData
                Layout.fillWidth: true; nested: true; padding: Theme.s3; spacing: Theme.s2
                RowLayout {
                    Layout.fillWidth: true; spacing: Theme.s2
                    Text { Layout.fillWidth: true; text: root.titleFor(modelData.id); wrapMode: Text.Wrap; font.family: Theme.font; font.pixelSize: Theme.fsSm; font.weight: Font.DemiBold; color: Theme.textPrimary }
                    Pill { text: modelData.status; tone: String(modelData.status).toLowerCase().indexOf('pass') >= 0 || String(modelData.status).toLowerCase() === 'consistent' ? Theme.success : Theme.warning }
                }
                Text { Layout.fillWidth: true; text: modelData.feedback; wrapMode: Text.Wrap; font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary }
                Repeater {
                    model: modelData.evidence
                    delegate: ColumnLayout {
                        required property var modelData
                        Layout.fillWidth: true; spacing: 2
                        Flow {
                            Layout.fillWidth: true; spacing: Theme.s2
                            Pill { text: root.titleFor(modelData.force_ref.documentType) + ' · revision ' + modelData.revision; neutral: true }
                        }
                        Text { Layout.fillWidth: true; text: '“' + modelData.quote + '”'; wrapMode: Text.Wrap; font.family: Theme.font; font.pixelSize: Theme.fsXs; font.italic: true; color: Theme.textMuted }
                    }
                }
            }
        }
        Flow {
            Layout.fillWidth: true
            Layout.preferredHeight: implicitHeight
            spacing: Theme.s2
            SButton {
                id: passButton
                text: 'Pass gate'
                enabled: root.canReview && !root.actionBusy && !!root.result && root.result.status === 'passed'
                onClicked: root.reviewRequested('passed')
                implicitWidth: 112
                contentItem: Text { text: passButton.text; font: passButton.font; verticalAlignment: Text.AlignVCenter; horizontalAlignment: Text.AlignHCenter; color: passButton.enabled ? Theme.textOnAccent : Theme.textMuted }
                background: Rectangle { radius: Theme.radiusMd; color: passButton.enabled ? (passButton.down ? Qt.darker(Theme.success, 1.2) : passButton.hovered ? Qt.lighter(Theme.success, 1.1) : Theme.success) : Theme.bgTertiary }
            }
            SButton {
                id: failButton
                text: 'Fail gate'
                enabled: root.canReview && !root.actionBusy
                onClicked: root.reviewRequested('failed')
                implicitWidth: 112
                contentItem: Text { text: failButton.text; font: failButton.font; verticalAlignment: Text.AlignVCenter; horizontalAlignment: Text.AlignHCenter; color: failButton.enabled ? Theme.danger : Theme.textMuted }
                background: Rectangle { radius: Theme.radiusMd; color: failButton.hovered && failButton.enabled ? Qt.alpha(Theme.danger, 0.10) : 'transparent'; border.width: Theme.borderWidth; border.color: failButton.enabled ? Theme.danger : Theme.border }
            }
            SButton {
                objectName: 'qroc-gate-revise'
                text: 'Revise from gate'; secondary: true
                visible: root.revisable && root.status === 'failed' && !root.invalidSources && !root.roundsSpent
                enabled: root.reviseEnabled && !root.actionBusy
                busy: root.reviseRunning
                onClicked: root.reviseRequested()
            }
            // Both rounds are spent but he has answered since: a new round is his explicit choice, confirmed first.
            SButton {
                objectName: 'qroc-gate-new-round'
                text: 'Start a new round with your answers'; secondary: true
                visible: root.revisable && root.status === 'failed' && !root.invalidSources && root.answeredSinceLastCall
                enabled: root.reviseEnabled && !root.actionBusy
                busy: root.reviseRunning
                onClicked: newRoundDialog.open()
            }
            SButton {
                text: 'Advance to ' + root.nextDomain; primary: true; iconName: 'arrow-right'
                visible: !!root.nextDomain
                enabled: root.canReview && !root.actionBusy && !!root.review && root.review.decision === 'passed'
                onClicked: root.advanceRequested()
            }
        }
        Item { Layout.fillHeight: true; Layout.preferredHeight: 0 }
    }
    ModalFrame {
        id: newRoundDialog
        objectName: 'qroc-gate-new-round-dialog'
        title: 'Start a new round'
        maxWidth: Theme.modalLg
        bodySpacing: Theme.s3
        Text {
            Layout.fillWidth: true; wrapMode: Text.Wrap
            text: 'This starts a new round from ' + (root.gateName() || 'this gate') + '. Both earlier rounds are used up. The documents this gate judges are written again with your answers, and the gate judges them again.'
            font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textPrimary
        }
        Text {
            Layout.fillWidth: true; wrapMode: Text.Wrap
            text: 'It runs on the selected route and may cost money.'
            font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary
        }
        footer: [
            SButton { text: 'Cancel'; quiet: true; onClicked: newRoundDialog.close() },
            SButton { objectName: 'qroc-gate-new-round-confirm'; text: 'Start new round'; primary: true; enabled: root.reviseEnabled && !root.actionBusy; onClicked: { newRoundDialog.close(); root.reviseRequested(); } }
        ]
    }
}
