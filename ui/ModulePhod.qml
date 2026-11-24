import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

Item {
    id: root
    property string journeyError: ""
    property string upperPolicy: ""
    property string actor: ""
    property string rationale: ""
    property string lowerBound: "0"
    property string upperBound: "100"
    property string trialUpper: ""
    property bool repairEnabled: false
    function refreshJourney() {
        Backend.send("phod-list",{},function(r,ok,e){if(ok){root.moduleInfo=r;root.jobs=r.jobs || []}else root.journeyError=e})
        if (root.selectedJob && root.selectedJob.id) Backend.send("phod-job-status",{id:root.selectedJob.id},function(r,ok,e){if(ok)root.selectedJob=r;else root.journeyError=e})
    }
    function journeyRequest(command,args,next) {
        root.journeyError=""
        Backend.send(command,args,function(r,ok,e){if(!ok){root.journeyError=e;return}if(next)next(r);root.refreshJourney()})
    }
    function startJourney() {
        var spec={profile:"integer-range/1",name:root.intentName,purpose:root.intentPurpose,min:Number(root.lowerBound),max:Number(root.upperBound),upper_policy:root.upperPolicy,actor:root.actor,rationale:root.rationale,repair_budget:root.repairEnabled?1:0}
        if(root.trialUpper.trim().length)spec.trial_max=Number(root.trialUpper)
        journeyRequest("phod-workbench-spec-save",spec,function(r){root.sourcePath=r.source_path;root.journeyRequest("phod-workbench",{source_path:r.source_path},function(j){root.selectedJob=j})})
    }
    Timer { interval: 500; repeat:true; running:root.visible && root.selectedJob!==null; onTriggered:root.refreshJourney() }
    property var moduleInfo: null
    property var jobs: []
    property var selectedJob: null
    property string sourcePath: ""
    property string charterName: ""
    property string intentName: ""
    property string intentPurpose: ""
    property string constraintsText: ""
    property int actionIndex: 0
    property bool rawOpen: false
    property int panelIndex: 0
    readonly property bool wide: width >= Theme.breakpointLg
    property string artifactPath: ""
    property string artifactText: ""
    property string artifactError: ""
    property int artifactSerial: 0
    property string artifactJobId: ""

    onSelectedJobChanged: {
        var id = selectedJob ? String(selectedJob.id || "") : ""
        if (id !== artifactJobId) { artifactJobId = id; ++artifactSerial; artifactPath = ""; artifactText = ""; artifactError = "" }
    }
    function readArtifact(item) {
        var serial = ++artifactSerial
        artifactPath = String(item.path || "")
        artifactText = ""
        artifactError = ""
        Backend.send("phod-artifact-read", {id: root.selectedJob.id, path: artifactPath}, function(result, ok, error) {
            if (serial !== root.artifactSerial) return
            if (ok) root.artifactText = result.text
            else root.artifactError = error || "Could not read the file"
        })
    }
    function capability() {
        return root.moduleInfo && root.moduleInfo.capabilities ? root.moduleInfo.capabilities[root.actionName()] : null
    }

    signal runRequested(string action, var args)
    signal draftRequested(var args)
    signal inspectRequested(string id)

    function pathFromUrl(url) {
        var value = String(url || "")
        if (value.indexOf("file://") === 0)
            value = value.slice(7)
        try { return decodeURIComponent(value) } catch (error) { return value }
    }
    function evidence() { return root.selectedJob && root.selectedJob.evidence ? root.selectedJob.evidence : {} }
    function jsonValue(text) {
        if (!text || !String(text).length) return null
        try { return JSON.parse(String(text)) } catch (error) { return null }
    }
    function report() {
        var ev = root.evidence()
        var text = ev.verify_stdout || ev.derive_stdout || ev.stdout || ""
        return root.jsonValue(text)
    }
    function outputText() {
        var ev = root.evidence()
        return String(ev.verify_stdout || ev.derive_stdout || ev.stderr || ev.stdout || "")
    }
    function rawReceipt() {
        var ev = root.evidence()
        return JSON.stringify({receipt: root.report(), evidence: ev}, null, 2)
    }
    function artifacts() {
        var ev = root.evidence()
        return Array.isArray(ev.artifacts) ? ev.artifacts : []
    }
    function receiptSummary() {
        var value = root.report()
        if (!root.selectedJob) return "No operation yet"
        var state = String(root.selectedJob.state || root.selectedJob.status || "").toLowerCase()
        if (state === "failed" || state === "error")
            return String(value && (value.error || value.message) || root.evidence().stderr || "Operation failed")
        if (value && value.status === "unavailable") return "Checker unavailable"
        if (value && value.status === "blocked") return "Blocked · resolve the ambiguity first"
        if (value && value.passed === true) return "All checks passed"
        if (value && value.passed === false) return "Checks failed"
        if (value && value.intent) return "Intent: " + String(value.intent.name || "unnamed") + " · " + (Array.isArray(value.constraints) ? value.constraints.length : 0) + " constraints"
        if (value) return "Result received"
        return "No result data"
    }
    function reportStatus() {
        var value = root.report()
        if (value && (value.status === "blocked" || value.status === "unavailable")) return value.status.toUpperCase()
        if (value && value.passed === true) return "VERIFIED"
        if (value && value.passed === false) return "CHECKS FAILED"
        var state = root.selectedJob ? String(root.selectedJob.state || root.selectedJob.status || "") : ""
        if (state.toLowerCase() === "succeeded") return "COMPLETE"
        if (root.selectedJob) return (state || "PENDING").toUpperCase()
        return "NO RESULT"
    }
    function stateWord(s) { return ({queued: "Waiting", running: "Running", succeeded: "Done", failed: "Failed", cancelled: "Cancelled", interrupted: "Interrupted"})[s] || String(s || ""); }
    function reportChecks() {
        var value = root.report()
        return value && Array.isArray(value.checks) ? value.checks : root.findings()
    }
    function reportDetail(item) {
        if (typeof item === "string") return item
        if (!item) return ""
        return String(item.detail || item.message || item.reason || item.name || "No detail")
    }
    function reportName(item) {
        if (typeof item === "string") return "Diagnostic"
        return String(item && (item.name || item.check || item.code) || "Diagnostic")
    }
    function reportVerdict(item) {
        if (typeof item === "string") return "INFO"
        return String(item && (item.status || item.verdict || (item.passed === true ? "PASS" : item.passed === false ? "FAIL" : "INFO")) || "INFO").toUpperCase()
    }
    function reportColor(item) { return reportVerdict(item) === "PASS" ? Theme.success : reportVerdict(item) === "FAIL" ? Theme.danger : Theme.textMuted }
    function findings() {
        var value = root.report()
        return value && Array.isArray(value.findings) ? value.findings : []
    }
    function statusText() {
        return root.reportStatus()
    }
    function draftArgs() {
        var lines = root.constraintsText.split(/\r?\n/).map(function(item) { return item.trim() }).filter(function(item) { return item.length > 0 })
        return {
            intent: {name: root.intentName, purpose: root.intentPurpose},
            constraints: lines
        }
    }
    function actionName() { return ["validate", "compile", "verify", "charter"][root.actionIndex] }

    function statusTone() {
        var s = root.reportStatus()
        if (s === "VERIFIED" || s === "COMPLETE") return Theme.success
        if (s === "CHECKS FAILED" || s === "FAILED" || s === "ERROR") return Theme.danger
        if (s === "BLOCKED") return Theme.warning
        if (s === "UNAVAILABLE" || s === "NO OPERATION") return Theme.pending
        return Theme.executing
    }
    function verdictTone(item) {
        var v = root.reportVerdict(item)
        return v === "PASS" ? Theme.success : v === "FAIL" ? Theme.danger : Theme.pending
    }
    function rustAvailable() {
        var tools = root.moduleInfo && root.moduleInfo.tooling ? root.moduleInfo.tooling : null
        if (tools && tools.rustc) return tools.rustc.available === true
        return !!(root.moduleInfo && root.moduleInfo.capabilities && root.moduleInfo.capabilities.workbench && root.moduleInfo.capabilities.workbench.available)
    }
    function missingTools() { return ["Zig", "Parser"] }
    function jobIndex() {
        if (!root.selectedJob) return -1
        for (var i = 0; i < root.jobs.length; ++i) if (root.jobs[i].id === root.selectedJob.id) return i
        return -1
    }
    function jobIdle() { return !!root.selectedJob && root.selectedJob.state !== "queued" && root.selectedJob.state !== "running" }
    readonly property bool canDerive: root.intentName.length > 0 && root.intentPurpose.length > 0 && root.upperPolicy.length > 0 && root.actor.length > 0 && root.rationale.length > 0 && !!root.moduleInfo && !!root.moduleInfo.capabilities.workbench.available

    component FieldLabel: Text {
        font.family: Theme.font
        font.pixelSize: Theme.fsXs
        font.weight: Font.Medium
        color: Theme.textSecondary
    }
    component CardTitle: Text {
        font.family: Theme.font
        font.pixelSize: Theme.fsLg
        font.weight: Font.DemiBold
        color: Theme.textPrimary
    }

    FileDialog {
        id: candidateDialog
        title: "Choose a candidate"
        fileMode: FileDialog.OpenFile
        nameFilters: ["Phod candidates (*.phod *.json)", "All files (*)"]
        onAccepted: root.sourcePath = root.pathFromUrl(selectedFile)
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        AppHeader { Layout.fillWidth: true; title: "Phod" }
        ScrollView {
            id: scroll
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: availableWidth
            contentHeight: body.implicitHeight + Theme.s6 * 2
            clip: true
            ColumnLayout {
                id: body
                x: Math.max(Theme.s6, (scroll.availableWidth - Theme.contentMaxWidth) / 2)
                y: Theme.s6
                width: Math.min(Theme.contentMaxWidth, scroll.availableWidth - Theme.s6 * 2)
                spacing: Theme.s5
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: Theme.s1
                    EyebrowLabel { text: "Phod workbench" }
                    Text { text: "Integer range"; font.family: Theme.font; font.pixelSize: Theme.fsXl; font.weight: Font.DemiBold; color: Theme.textPrimary }
                    Text { text: "Checks a whole-number range, written in Rust"; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary }
                    Flow {
                        Layout.fillWidth: true
                        Layout.topMargin: Theme.s2
                        spacing: Theme.s2
                        Pill { text: root.rustAvailable() ? "Rust" : "Rust unavailable"; tone: root.rustAvailable() ? Theme.success : Theme.pending }
                        Repeater {
                            model: root.missingTools()
                            delegate: Pill { required property string modelData; text: modelData + " unavailable"; tone: Theme.pending }
                        }
                    }
                }
                Card {
                    Layout.fillWidth: true
                    visible: root.journeyError.length > 0
                    Text { Layout.fillWidth: true; text: root.journeyError; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.dangerText; wrapMode: Text.Wrap }
                }
                GridLayout {
                    Layout.fillWidth: true
                    columns: root.wide ? 2 : 1
                    columnSpacing: Theme.s5
                    rowSpacing: Theme.s5
                    uniformCellWidths: true
                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignTop
                        spacing: Theme.s5
                        Card {
                            Layout.fillWidth: true
                            padding: Theme.s5
                            CardTitle { text: "Intent" }
                            FieldLabel { text: "Name" }
                            Field { Layout.fillWidth: true; text: root.intentName; placeholderText: "Name of the intended outcome"; onTextChanged: root.intentName = text }
                            FieldLabel { text: "Purpose" }
                            Field { Layout.fillWidth: true; text: root.intentPurpose; placeholderText: "What this candidate should establish"; onTextChanged: root.intentPurpose = text }
                            FieldLabel { text: "Constraints (one per line)" }
                            ScrollView {
                                contentWidth: availableWidth
                                Layout.fillWidth: true
                                Layout.preferredHeight: 104
                                clip: true
                                STextArea {
                                    width: parent.width
                                    text: root.constraintsText
                                    placeholderText: "One constraint per line"
                                    wrapMode: TextArea.Wrap
                                    onTextChanged: root.constraintsText = text
                                }
                            }
                        }
                        Card {
                            Layout.fillWidth: true
                            padding: Theme.s5
                            CardTitle { text: "Range" }
                            GridLayout {
                                Layout.fillWidth: true
                                columns: scroll.availableWidth < 560 ? 1 : 2
                                columnSpacing: Theme.s3
                                rowSpacing: Theme.s2
                                ColumnLayout { Layout.fillWidth: true; spacing: Theme.s1
                                    FieldLabel { text: "Lower" }
                                    Field { Layout.fillWidth: true; text: root.lowerBound; placeholderText: "Lower integer"; onTextChanged: root.lowerBound = text } }
                                ColumnLayout { Layout.fillWidth: true; spacing: Theme.s1
                                    FieldLabel { text: "Upper" }
                                    Field { Layout.fillWidth: true; text: root.upperBound; placeholderText: "Upper integer"; onTextChanged: root.upperBound = text } }
                                ColumnLayout { Layout.fillWidth: true; spacing: Theme.s1
                                    FieldLabel { text: "Upper policy" }
                                    Choice {
                                        Layout.fillWidth: true
                                        model: ["Choose policy", "inclusive", "exclusive"]
                                        currentIndex: Math.max(0, model.indexOf(root.upperPolicy))
                                        onActivated: root.upperPolicy = currentIndex === 0 ? "" : model[currentIndex]
                                        Accessible.name: "Upper policy"
                                    } }
                                ColumnLayout { Layout.fillWidth: true; spacing: Theme.s1
                                    FieldLabel { text: "Decision actor" }
                                    Field { Layout.fillWidth: true; text: root.actor; placeholderText: "Who decides"; onTextChanged: root.actor = text } }
                                ColumnLayout { Layout.fillWidth: true; spacing: Theme.s1
                                    FieldLabel { text: "Rationale" }
                                    Field { Layout.fillWidth: true; text: root.rationale; placeholderText: "Why this range"; onTextChanged: root.rationale = text } }
                                ColumnLayout { Layout.fillWidth: true; spacing: Theme.s1
                                    FieldLabel { text: "Trial upper" }
                                    Field { Layout.fillWidth: true; text: root.trialUpper; placeholderText: "Optional"; onTextChanged: root.trialUpper = text } }
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: Theme.s3
                                SCheckBox { text: "Allow one repair"; checked: root.repairEnabled; onToggled: root.repairEnabled = checked }
                                Item { Layout.fillWidth: true }
                                SButton { text: "Derive and compare"; primary: true; enabled: root.canDerive; onClicked: root.startJourney() }
                            }
                        }
                        Card {
                            Layout.fillWidth: true
                            padding: Theme.s5
                            CardTitle { text: "Candidate file" }
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: Theme.s2
                                Field { Layout.fillWidth: true; text: root.sourcePath.split("/").pop(); placeholderText: "Choose a candidate file"; readOnly: true }
                                SButton { text: "Browse"; iconName: "folder"; onClicked: candidateDialog.open() }
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: Theme.s2
                                FieldLabel { text: "Charter" }
                                Field { Layout.fillWidth: true; text: root.charterName; placeholderText: "Optional label"; onTextChanged: root.charterName = text }
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: Theme.s2
                                Choice { Layout.preferredWidth: 140; model: ["validate", "compile", "verify", "charter"]; currentIndex: root.actionIndex; onActivated: root.actionIndex = currentIndex; Accessible.name: "Operation" }
                                SButton {
                                    text: root.actionName().charAt(0).toUpperCase() + root.actionName().slice(1)
                                    primary: true
                                    enabled: root.sourcePath.length > 0 && root.capability() && root.capability().available
                                    onClicked: root.runRequested(root.actionName(), {source_path: root.sourcePath, charter: root.charterName, draft: root.draftArgs()})
                                }
                                Item { Layout.fillWidth: true }
                                SButton {
                                    text: "Save draft"
                                    enabled: root.intentName.trim().length > 0 && root.intentPurpose.trim().length > 0
                                    onClicked: root.draftRequested(root.draftArgs())
                                }
                            }
                        }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignTop
                        spacing: Theme.s5
                        Card {
                            Layout.fillWidth: true
                            padding: Theme.s5
                            CardTitle { text: "Jobs" }
                            Choice {
                                Layout.fillWidth: true
                                model: root.jobs.map(function(j, i) { return "Run " + (i + 1) + " · " + root.stateWord(j.state) })
                                currentIndex: root.jobIndex()
                                onActivated: root.inspectRequested(root.jobs[currentIndex].id)
                                Accessible.name: "Jobs"
                            }
                            Flow {
                                Layout.fillWidth: true
                                spacing: Theme.s2
                                SButton { text: "Refresh"; iconName: "loader"; onClicked: root.refreshJourney() }
                                SButton { text: "Cancel"; enabled: !!root.selectedJob && (root.selectedJob.state === "queued" || root.selectedJob.state === "running"); onClicked: root.journeyRequest("phod-job-cancel", {id: root.selectedJob.id}) }
                                SButton { text: "Replay"; enabled: root.jobIdle(); onClicked: root.journeyRequest("phod-replay", {id: root.selectedJob.id}, function(r) { root.artifactText = JSON.stringify(r, null, 2) }) }
                                SButton { text: "Rerun"; enabled: root.jobIdle(); onClicked: root.journeyRequest("phod-" + root.selectedJob.action, {source_path: root.selectedJob.source_path}, function(j) { root.selectedJob = j }) }
                            }
                        }
                        Card {
                            Layout.fillWidth: true
                            padding: Theme.s5
                            RowLayout {
                                Layout.fillWidth: true
                                CardTitle { text: "Result" }
                                Item { Layout.fillWidth: true }
                                Pill { text: root.statusText(); tone: root.statusTone() }
                            }
                            Text { Layout.fillWidth: true; text: root.receiptSummary(); font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary; wrapMode: Text.Wrap }
                            Repeater {
                                model: root.reportChecks()
                                delegate: Card {
                                    id: checkRow
                                    required property var modelData
                                    Layout.fillWidth: true
                                    nested: true
                                    padding: Theme.s3
                                    RowLayout {
                                        Layout.fillWidth: true
                                        spacing: Theme.s3
                                        Pill { Layout.alignment: Qt.AlignTop; text: root.reportVerdict(checkRow.modelData); tone: root.verdictTone(checkRow.modelData) }
                                        ColumnLayout {
                                            Layout.fillWidth: true
                                            spacing: 2
                                            Text { Layout.fillWidth: true; text: root.reportName(checkRow.modelData); font.family: Theme.font; font.pixelSize: Theme.fsSm; font.weight: Font.Medium; color: Theme.textPrimary; wrapMode: Text.Wrap }
                                            Text { Layout.fillWidth: true; text: root.reportDetail(checkRow.modelData); font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary; wrapMode: Text.Wrap }
                                        }
                                    }
                                }
                            }
                            EmptyState {
                                Layout.fillWidth: true
                                visible: !root.rawOpen && root.reportChecks().length === 0 && root.artifacts().length === 0
                                card: false
                                icon: "chart"
                                tone: Theme.textSecondary
                                title: "No result yet"
                                detail: "Run Validate or Verify."
                            }
                            ColumnLayout {
                                Layout.fillWidth: true
                                visible: root.artifacts().length > 0
                                spacing: Theme.s2
                                EyebrowLabel { text: "Artifacts" }
                                Repeater {
                                    model: root.artifacts()
                                    delegate: Card {
                                        id: artifactRow
                                        required property var modelData
                                        Layout.fillWidth: true
                                        nested: true
                                        pressable: true
                                        padding: Theme.s3
                                        onClicked: root.readArtifact(artifactRow.modelData)
                                        RowLayout {
                                            Layout.fillWidth: true
                                            spacing: Theme.s3
                                            SIcon { name: "file"; size: Theme.icon; color: Theme.textMuted }
                                            Text { Layout.fillWidth: true; text: String(artifactRow.modelData.role || "artifact") + ": " + String(artifactRow.modelData.path || "").split("/").pop(); font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textPrimary; elide: Text.ElideMiddle }
                                            Pill { text: "read-only"; neutral: true }
                                        }
                                    }
                                }
                            }
                            Text { Layout.fillWidth: true; visible: root.artifactPath.length > 0; text: root.artifactPath.split("/").pop(); font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textMuted; elide: Text.ElideMiddle }
                            Text { Layout.fillWidth: true; visible: root.artifactError.length > 0; text: root.artifactError; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.dangerText; wrapMode: Text.Wrap }
                            ScrollView {
                                contentWidth: availableWidth
                                Layout.fillWidth: true
                                Layout.preferredHeight: 200
                                visible: root.artifactText.length > 0
                                clip: true
                                STextArea { width: parent.width; readOnly: true; text: root.artifactText; wrapMode: TextArea.Wrap; font.family: Theme.mono; font.pixelSize: Theme.fsXs }
                            }
                            // The engine's raw receipt (paths, ids, digests) stays off the window; the checks and artifacts above carry the result.
                            SButton {
                                visible: false
                                Layout.alignment: Qt.AlignLeft
                                text: root.rawOpen ? "Hide raw result" : "Show raw result"
                                enabled: root.selectedJob !== null
                                onClicked: root.rawOpen = !root.rawOpen
                            }
                            ScrollView {
                                contentWidth: availableWidth
                                Layout.fillWidth: true
                                Layout.preferredHeight: 240
                                visible: root.rawOpen
                                clip: true
                                STextArea { width: parent.width; readOnly: true; wrapMode: TextArea.Wrap; text: root.rawReceipt(); font.family: Theme.mono; font.pixelSize: Theme.fsXs }
                            }
                        }
                    }
                }
            }
        }
    }
}
