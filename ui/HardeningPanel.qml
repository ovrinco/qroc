import QtQuick
import QtQuick.Layouts

// The hardening loop in the reader: the score of the last round, which parts of the idea it found,
// the questions the route asked, the gate's decision, the answer field and one row per round.
// Everything shown is the engine's record; nothing is scored here.
ColumnLayout {
    id: panel
    required property var workspace
    property bool hasIdea: false
    readonly property var hardening: workspace.hardening
    readonly property var rounds: hardening && hardening.record ? hardening.record.rounds || [] : []
    readonly property var last: rounds.length ? rounds[rounds.length - 1] : null
    readonly property bool running: !!hardening && hardening.running
    readonly property bool answerable: !!last && last.sent_back && !last.answers && !running
    readonly property var foundOnDisk: last && last.questions ? last.questions.found || [] : []
    // The engine names a missing part by its element; the seven set-up fields read as one row.
    readonly property var dimensions: [
        {word: 'Problem', names: ['problem']}, {word: 'Customer', names: ['customer']}, {word: 'Market', names: ['market']},
        {word: 'Solution', names: ['solution']}, {word: 'Monetisation', names: ['monetization']}, {word: 'Constraints', names: ['constraints']},
        {word: 'Set-up', names: ['teamSize', 'fundingModel', 'architecturePhilosophy', 'pitchAudience', 'techStack', 'antiPatterns', 'customConstraints']}
    ]
    // Why the route asked nothing, in the sheet's words; any other engine reason is not shown as text.
    function questionsWord(q) {
        var r = q && q.status !== 'asked' ? String(q.reason || '') : '';
        return r.indexOf('no route is declared') === 0 ? 'No route set' : r.indexOf('did not answer') >= 0 ? 'Not reachable' : '';
    }
    // A record by its own file name and line, never its folder path: "Operator decisions, line 1626".
    function recordName(source) {
        var text = String(source || '');
        var m = text.match(/^(.*):(\d+)$/);
        var path = m ? m[1] : text;
        var name = path.split('/').pop().replace(/\.[A-Za-z0-9]+$/, '').replace(/[_-]+/g, ' ').trim();
        name = name ? name.charAt(0).toUpperCase() + name.slice(1).toLowerCase() : 'Record';
        return m ? name + ', line ' + m[2] : name;
    }
    function missingNames(round) { return (round.score.missing || []).map(m => String(m).split(':')[0]); }
    function verdictWord(v) { return ({refuse: 'Refused', clarify: 'Needs answers', partial: 'Partly ready', pass: 'Ready'})[v] || ''; }
    function verdictTone(v) { return v === 'refuse' ? Theme.danger : v === 'pass' ? Theme.success : Theme.warning; }
    spacing: Theme.s4

    // No idea yet.
    Card {
        Layout.fillWidth: true
        visible: !panel.hasIdea
        Text { text: 'Give QROC an idea'; font.family: Theme.font; font.pixelSize: Theme.fsBase; font.weight: Font.DemiBold; color: Theme.textPrimary }
        SButton { text: 'Write the idea'; primary: true; onClicked: panel.workspace.openProjectNotes() }
    }
    // The idea as given, before the first round.
    Text {
        Layout.fillWidth: true
        visible: panel.hasIdea && !panel.last
        text: Theme.readableStored(panel.workspace.project ? panel.workspace.project.brain_dump || '' : '')
        wrapMode: Text.Wrap; maximumLineCount: 12; elide: Text.ElideRight
        font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary
    }
    SButton {
        visible: panel.hasIdea && !panel.last
        text: 'Hardening'; primary: true; busy: panel.running
        enabled: !panel.running
        onClicked: panel.workspace.startHardening()
    }

    // The last round.
    RowLayout {
        visible: !!panel.last
        spacing: Theme.s3
        Text {
            text: panel.last ? panel.last.score.total : ''
            font.family: Theme.font; font.pixelSize: 36; font.weight: Font.Bold
            color: panel.last ? panel.verdictTone(panel.last.verdict) : Theme.textPrimary
        }
        ColumnLayout {
            spacing: 2
            Text { text: 'of 100'; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary }
        }
    }
    ColumnLayout {
        Layout.fillWidth: true
        visible: !!panel.last
        spacing: Theme.s1
        Repeater {
            model: panel.dimensions
            delegate: RowLayout {
                required property var modelData
                readonly property bool missing: !!panel.last && modelData.names.some(n => panel.missingNames(panel.last).indexOf(n) >= 0)
                Layout.fillWidth: true; spacing: Theme.s2
                SIcon { name: parent.missing ? 'circle' : 'check-circle'; size: Theme.iconSm; color: parent.missing ? Theme.textMuted : Theme.success }
                Text {
                    Layout.fillWidth: true
                    text: modelData.word
                    font.family: Theme.font; font.pixelSize: Theme.fsSm
                    color: parent.missing ? Theme.textSecondary : Theme.textPrimary
                }
            }
        }
    }

    // The route's questions, or the engine's reason when it asked none.
    ColumnLayout {
        Layout.fillWidth: true
        visible: !!panel.last && !!panel.last.questions
        spacing: Theme.s2
        Repeater {
            model: panel.last && panel.last.questions ? panel.last.questions.items || [] : []
            delegate: Text {
                required property string modelData
                Layout.fillWidth: true
                text: modelData
                wrapMode: Text.Wrap
                font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textPrimary
                leftPadding: Theme.s3
                Rectangle { width: 2; height: parent.height; color: Theme.border }
            }
        }
        Text {
            Layout.fillWidth: true
            visible: !!text
            text: panel.last ? panel.questionsWord(panel.last.questions) : ''
            wrapMode: Text.Wrap
            font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary
        }
    }

    // What our own records already answer, each with the record it was found in, by name.
    ColumnLayout {
        Layout.fillWidth: true
        visible: panel.foundOnDisk.length > 0
        spacing: Theme.s1
        Text {
            text: 'Found on our own disk'
            font.family: Theme.font; font.pixelSize: Theme.fsXs; font.weight: Font.DemiBold; color: Theme.textSecondary
        }
        Repeater {
            model: panel.foundOnDisk
            delegate: ColumnLayout {
                required property var modelData
                Layout.fillWidth: true
                spacing: 0
                Text {
                    Layout.fillWidth: true
                    text: modelData.gap + ': ' + modelData.says
                    wrapMode: Text.Wrap
                    font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textPrimary
                    leftPadding: Theme.s3
                    Rectangle { width: 2; height: parent.height; color: Theme.success }
                }
                Text {
                    Layout.fillWidth: true
                    text: panel.recordName(modelData.source)
                    elide: Text.ElideMiddle
                    font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textMuted
                    leftPadding: Theme.s3
                }
            }
        }
    }
    STextArea {
        id: answers
        Layout.fillWidth: true; Layout.preferredHeight: 120
        visible: panel.answerable
        wrapMode: TextEdit.Wrap
        Accessible.name: 'Answers'
    }
    SButton {
        visible: panel.answerable
        text: 'Round ' + (panel.rounds.length + 1); primary: true
        enabled: !!answers.text.trim()
        onClicked: { panel.workspace.answerHardening(answers.text); answers.text = ''; }
    }
    Text {
        Layout.fillWidth: true
        visible: !!panel.hardening && !!panel.hardening.error
        text: panel.hardening ? panel.hardening.error || '' : ''
        wrapMode: Text.Wrap
        font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.dangerText
    }

    // The loop: one row per round, newest last.
    ColumnLayout {
        Layout.fillWidth: true
        visible: panel.rounds.length > 0
        spacing: Theme.s1
        Repeater {
            model: panel.rounds
            delegate: Text {
                required property var modelData
                Layout.fillWidth: true
                text: 'Round ' + modelData.round + ' · ' + modelData.score.total + ' of 100 · ' + panel.verdictWord(modelData.verdict)
                font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary
                elide: Text.ElideRight
            }
        }
    }
}
