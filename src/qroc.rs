//! Durable native Q-ROC workspace.
//!
//! This module keeps Q-ROC authoring state separate from the planner journal. It
//! stores projects, branches, document revisions, and generation receipts under
//! one private directory and exposes a small command surface for the QML
//! workspace. Generation is local-only and uses the configured OpenAI-compatible
//! gateway; no quality score is invented when the model or gate is unavailable.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::{Shutdown, TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

mod gate_revise;

const MAX_TITLE: usize = 400;
const MAX_BODY: usize = 2 * 1024 * 1024;
const MAX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
const MAX_RECORD_BYTES: u64 = 16 * 1024 * 1024;
const MAX_GENERATION_CONTEXT_BYTES: usize = 512 * 1024;
const MAX_ATTACHMENT_CONTEXT_BYTES: usize = 128 * 1024;
const MAX_JOBS: usize = 64;
const PROMPT_BUILDER_SHA256: &str =
    "2900239d9fee50b794be84cc02faec6dce72531780707cfab5a0d39c85b9ed65";
const PROMPT_COMPILER_SHA256: &str =
    "452878d313f21b2440b5c23c1f4db1a187f829dd09202f4d3c6d27571ef7563d";
const BASE_PERSONA: &str = "You are an expert technical writer, architect, and product strategist. Produce complete, clear, actionable, consistent, evidence-based documentation. Use RFC 2119 keywords for requirements, measurable criteria, explicit assumptions, open questions, risks, and cross-references. Never invent evidence, approval, scores, implementation results, or facts.";

const DOMAINS: [&str; 7] = crate::registry::DOMAIN_ORDER;
const PRODUCT_DOCUMENTS: [&str; 16] = [
    "STRATEGY_BRIEF",
    "PRE_MORTEM",
    "PRD",
    "VALIDATION_PLAN",
    "TECHNICAL_ARCHITECTURE",
    "GATE_1",
    "DESIGN_ALTERNATIVES",
    "UI_UX_SPEC",
    "ROADMAP",
    "IMPLEMENTATION_PLAN",
    "GATE_2",
    "EPICS",
    "SLICE_INDEX",
    "GATE_3",
    "AGENT_DIRECTIVES",
    "TASKS",
];
const BUSINESS_DOCUMENTS: [&str; 11] = [
    "MARKET_ANALYSIS",
    "COMPETITOR_ANALYSIS",
    "GATE_4",
    "BUSINESS_PLAN",
    "BRAND_GUIDE",
    "GO_TO_MARKET",
    "GATE_5",
    "FINANCIAL_OVERVIEW",
    "SLIDE_DECK",
    "FINANCIAL_SENSITIVITY",
    "GATE_6",
];
const PRESENCE_DOCUMENTS: [&str; 8] = [
    "MESSAGING_FRAMEWORK",
    "OUTBOUND_STRATEGY",
    "INBOUND_STRATEGY",
    "GATE_7",
    "WEBSITE_SOCIAL",
    "ADS_STRATEGY",
    "VISUAL_ASSETS",
    "GATE_8",
];
const LEGAL_DOCUMENTS: [&str; 8] = [
    "COMPLIANCE_PRIVACY",
    "PR_POLICY",
    "LICENSING",
    "GATE_9",
    "TERMS",
    "CONTRACTS",
    "REGISTRY_PREP",
    "GATE_10",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub title: String,
    pub brain_dump: String,
    #[serde(default)]
    pub sources: Vec<SourceRef>,
    #[serde(default)]
    pub knowledge: Vec<KnowledgeItem>,
    #[serde(default = "initial_domain")]
    pub current_domain: String,
    #[serde(default)]
    pub gate_reviews: Vec<RecordedGateReview>,
    #[serde(default)]
    pub domain_transitions: Vec<DomainTransition>,
    /// "product" (the document pipeline) or "media" (a track or a piece).
    #[serde(default = "product_kind")]
    pub kind: String,
    /// A media project made in alignment with an earlier product project, or none when it
    /// was started on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_project_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

fn product_kind() -> String {
    "product".into()
}

fn initial_domain() -> String {
    DOMAINS[0].into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedGateReview {
    pub document_id: String,
    pub branch_id: String,
    pub document_type: String,
    pub revision: u32,
    pub sha256: String,
    pub decision: String,
    pub reviewed_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_inputs: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainTransition {
    pub from: String,
    pub to: String,
    pub branch_id: String,
    pub gates: Vec<RecordedGateReview>,
    pub recorded_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeItem {
    pub id: String,
    pub project_id: String,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub source_path: Option<String>,
    pub selected: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    pub id: String,
    pub name: String,
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub model: String,
    pub provider: String,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentRevision {
    pub id: String,
    pub project_id: String,
    pub branch_id: String,
    pub document_type: String,
    pub revision: u32,
    pub content: String,
    pub rendered: String,
    pub source_revision: Option<u32>,
    pub created_at: i64,
    pub sha256: String,
    #[serde(default)]
    pub provenance: Option<SynthesisProvenance>,
    #[serde(default)]
    pub generation_finish_reason: Option<String>,
    #[serde(default)]
    pub generation_inputs: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesisSource {
    pub branch_id: String,
    pub branch_name: String,
    pub document_id: String,
    pub document_type: String,
    pub revision: u32,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesisProvenance {
    pub source_branch_ids: Vec<String>,
    pub source_documents: Vec<SynthesisSource>,
    pub target_branch_id: String,
    pub document_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateResult {
    pub document_type: String,
    pub status: String,
    pub score: Option<u32>,
    pub criteria: BTreeMap<String, CriterionResult>,
    pub threshold: Option<u32>,
    pub reason: String,
    pub evaluated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic: Option<Value>,
    /// Facts or decisions only the project's owner can supply, as the gate named them.
    /// Kept with the verdict; they explain a failure and never soften it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<GateQuestion>,
    /// Content repairs a writer can make without the owner's answer, as the gate named them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repairs: Vec<GateRepair>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub questions_note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateQuestion {
    pub criterion: String,
    pub document: String,
    pub question: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateRepair {
    pub document: String,
    pub criterion: String,
    pub repair: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriterionResult {
    pub score: u32,
    pub max: u32,
    pub feedback: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JobRecord {
    id: String,
    project_id: String,
    branch_id: String,
    document_type: String,
    state: String,
    model: String,
    #[serde(default)]
    after_job: Option<String>,
    #[serde(default)]
    batch_id: Option<String>,
    #[serde(default)]
    batch_width: usize,
    #[serde(default)]
    dependency_jobs: Vec<String>,
    #[serde(default)]
    queued_context: String,
    #[serde(default)]
    pinned_inputs: Option<Value>,
    #[serde(default)]
    hardening_round: Option<u32>,
    /// Set only by a revise-from-gate call: the round, the gate revision it answers and,
    /// for a joined document, the gate's feedback added to its context.
    #[serde(default)]
    gate_revise: Option<Value>,
    #[serde(default = "default_generation_timeout_seconds")]
    timeout_seconds: u64,
    /// Prior transport attempts retained when explicitly resuming the same logical job.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    attempts: Vec<Value>,
    #[serde(default)]
    max_tokens: u32,
    /// The output limit the route says it honoured for this job's reply, when it says one;
    /// a shim may honour less than was asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    honoured_max_tokens: Option<u32>,
    error: Option<String>,
    document_id: Option<String>,
    /// Revision visible when the model context was pinned. A completion may
    /// publish only when this is still the branch head.
    #[serde(default)]
    base_revision: Option<u32>,
    /// Set before publishing generated content so a restart can reconcile a
    /// document that was committed just before the receipt write.
    #[serde(default)]
    output_sha256: Option<String>,
    #[serde(default)]
    generation_finish_reason: Option<String>,
    #[serde(default)]
    conflict_path: Option<String>,
    #[serde(default)]
    synthesis: Option<SynthesisProvenance>,
    started_at: i64,
    finished_at: Option<i64>,
}

struct RunningJob {
    cancel: Arc<AtomicBool>,
    receiver: Receiver<Result<GenerationResult>>,
}

struct GenerationResult {
    content: String,
    finish_reason: String,
    model: String,
    route: Option<String>,
    honoured_max_tokens: Option<u32>,
}

pub struct QrocStore {
    root: PathBuf,
    projects: BTreeMap<String, Project>,
    branches: BTreeMap<String, Branch>,
    jobs: BTreeMap<String, JobRecord>,
    running: BTreeMap<String, RunningJob>,
    sequence: u64,
}

impl QrocStore {
    pub fn open(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)?;
        let metadata = fs::symlink_metadata(root)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("Q-ROC root must be a real directory"));
        }
        for dir in [
            "projects",
            "branches",
            "documents",
            "jobs",
            "conflicts",
            "exports",
            "sources",
        ] {
            fs::create_dir_all(root.join(dir))?;
        }
        fs::create_dir_all(root.join("drafts"))?;
        let mut store = Self {
            root: root.to_path_buf(),
            projects: BTreeMap::new(),
            branches: BTreeMap::new(),
            jobs: BTreeMap::new(),
            running: BTreeMap::new(),
            sequence: 0,
        };
        store.load_records()?;
        Ok(store)
    }

    /// Refuse missing token budgets and invalid waits before creating pipeline work.
    pub(crate) fn validate_pipeline_budget(&self) -> Result<()> {
        generation_max_tokens(&self.root, std::env::var("QROC_MAX_TOKENS").ok().as_deref())?;
        self.generation_wait_seconds()?;
        Ok(())
    }

    fn generation_wait_seconds(&self) -> Result<u64> {
        let configured = std::env::var("QROC_TIMEOUT_SECONDS").ok();
        if configured.is_none()
            && crate::routes::selected_at(&self.root)?
                .is_some_and(|r| r.kind == crate::routes::Kind::Relay)
        {
            // Six measured native responses took 67–164 seconds; allow bounded relay overhead.
            return Ok(600);
        }
        generation_timeout_seconds(configured.as_deref())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn command(&mut self, command: &str, args: &Value) -> Result<Value> {
        if args.get("gate_revise").is_some() {
            return Err(invalid(
                "A revise-from-gate round is set by the engine, not caller-supplied metadata",
            ));
        }
        if args.get("synthesis_provenance").is_some() {
            return Err(invalid(
                "Synthesis provenance is derived from reviewed source revisions, not caller-supplied metadata",
            ));
        }
        self.poll_jobs()?;
        match command {
            "qroc-intake" => crate::intake::command(self, args),
            "qroc-run" => crate::pipeline::command(self, args),
            "qroc-project-open-folder" => {
                Ok(json!({"folder": self.open_project_folder(s(args, "project_id"))?}))
            }
            "qroc-project-list" => self.project_list(),
            "qroc-project-create" => self.project_create(args),
            "qroc-project-link" => self.project_link(args),
            "qroc-project-import" => self.project_import(args),
            "qroc-project-detail" => self.project_detail(s(args, "id")),
            "qroc-project-save" => self.project_save(args),
            "qroc-kb-list" => self.kb_list(s(args, "project_id")),
            "qroc-kb-save" => self.kb_save(args),
            "qroc-kb-remove" => self.kb_remove(args),
            "qroc-kb-context" => self.kb_context(s(args, "project_id")),
            "qroc-context-preview" => self.context_preview(args),
            "qroc-skills-import" => self.skills_import(args),
            "qroc-branch-list" => self.branch_list(s(args, "project_id")),
            "qroc-branch-create" => self.branch_create(args),
            "qroc-document-list" => self.document_list(args),
            "qroc-document-detail" => self.document_detail(args),
            "qroc-document-save" => self.document_save(args),
            "qroc-document-history" => self.document_history(args),
            "qroc-draft-save" => self.draft_save(args),
            "qroc-draft-get" => self.draft_get(args),
            "qroc-draft-clear" => self.draft_clear(args),
            "qroc-generate-start" => self.generate_start(args),
            "qroc-synthesis-start" => self.synthesis_start(args),
            "qroc-job-list" => self.job_list(args),
            "qroc-job-status" => self.job_status(s(args, "id")),
            "qroc-job-cancel" => self.job_cancel(s(args, "id")),
            "qroc-gate-evaluate" => self.gate_evaluate(args),
            "qroc-gate-semantic-evaluate" => {
                if s(args, "document_type") != "GATE_4" {
                    return Err(invalid(
                        "capability-unavailable: unknown semantic gate profile",
                    ));
                }
                self.gate_evaluate(args)
            }
            "qroc-gate-review" => self.gate_review(args),
            "qroc-gate-revise-start" => self.gate_revise_start(args),
            "qroc-gate-revise-get" => self.gate_revise_get(args),
            "qroc-gate-question-answer" => self.gate_question_answer(args),
            "qroc-domain-advance" => self.domain_advance(args),
            "qroc-analysis" => self.analysis(args),
            "qroc-batch-start" => self.batch_start(args),
            "qroc-graph-export" => self.graph_export(args),
            "qroc-export" => self.export(args),
            _ => Err(invalid(format!("Unknown Q-ROC command: {command}"))),
        }
    }

    fn load_records(&mut self) -> Result<()> {
        for item in fs::read_dir(self.root.join("projects"))? {
            let path = item?.path();
            if path.extension().and_then(|v| v.to_str()) != Some("json") {
                continue;
            }
            let project: Project = read_json(&path).map_err(|error| {
                invalid(format!(
                    "Could not load Q-ROC project {}: {error}",
                    path.display()
                ))
            })?;
            self.observe_id(&project.id);
            for item in &project.knowledge {
                self.observe_id(&item.id);
            }
            self.projects.insert(project.id.clone(), project);
        }
        for item in fs::read_dir(self.root.join("branches"))? {
            let path = item?.path();
            if path.extension().and_then(|v| v.to_str()) != Some("json") {
                continue;
            }
            let branch: Branch = read_json(&path).map_err(|error| {
                invalid(format!(
                    "Could not load Q-ROC branch {}: {error}",
                    path.display()
                ))
            })?;
            self.observe_id(&branch.id);
            self.branches.insert(branch.id.clone(), branch);
        }
        for item in fs::read_dir(self.root.join("jobs"))? {
            let path = item?.path();
            if path.extension().and_then(|v| v.to_str()) != Some("json") {
                continue;
            }
            let mut job: JobRecord = read_json(&path).map_err(|error| {
                invalid(format!(
                    "Could not load Q-ROC job {}: {error}",
                    path.display()
                ))
            })?;
            self.observe_id(&job.id);
            if job.state == "queued" || job.state == "running" {
                let reconciled = job
                    .output_sha256
                    .as_deref()
                    .map(|output_sha256| {
                        find_document_by_sha(
                            &self.root,
                            &job.project_id,
                            &job.branch_id,
                            &job.document_type,
                            output_sha256,
                        )
                    })
                    .transpose()?
                    .flatten();
                let reconciled = reconciled.filter(|d| {
                    job.pinned_inputs.is_none()
                        || d.generation_inputs
                            .as_ref()
                            .is_some_and(|v| v["job_id"] == job.id)
                });
                if let Some(document) = reconciled {
                    job.state = if document
                        .generation_finish_reason
                        .as_deref()
                        .is_some_and(|r| r != "stop")
                    {
                        "partial"
                    } else {
                        "succeeded"
                    }
                    .into();
                    job.generation_finish_reason = document.generation_finish_reason.clone();
                    if job.state == "partial" {
                        job.error=Some("Provider returned an incomplete draft; retained revision requires review".into());
                    }
                    job.document_id = Some(document.id);
                } else {
                    job.state = "interrupted".into();
                    job.error = Some(
                        "The Q-ROC service restarted before this generation completed.".into(),
                    );
                }
                job.finished_at = Some(now());
                write_json_atomic(&path, &job)?;
            }
            self.jobs.insert(job.id.clone(), job);
        }
        self.observe_document_ids(&self.root.join("documents"))?;
        Ok(())
    }

    fn observe_id(&mut self, id: &str) {
        if let Some(suffix) = id
            .rsplit('-')
            .next()
            .and_then(|value| value.parse::<u64>().ok())
        {
            self.sequence = self.sequence.max(suffix);
        }
    }

    fn observe_document_ids(&mut self, root: &Path) -> Result<()> {
        if !root.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(root)? {
            let path = entry?.path();
            if path.is_dir() {
                self.observe_document_ids(&path)?;
            } else if path.extension().and_then(|value| value.to_str()) == Some("json")
                && let Ok(doc) = read_json::<DocumentRevision>(&path)
            {
                self.observe_id(&doc.id);
            }
        }
        Ok(())
    }

    fn new_id(&mut self, kind: &str) -> String {
        self.sequence = self.sequence.saturating_add(1);
        format!("qroc-{kind}-{}-{:04}", now(), self.sequence)
    }

    fn project_list(&self) -> Result<Value> {
        Ok(json!({"projects": self.projects.values().collect::<Vec<_>>() }))
    }

    fn project_create(&mut self, args: &Value) -> Result<Value> {
        let title = bounded_text(s(args, "title"), MAX_TITLE, "Project title")?;
        let brain_dump = bounded_text(s(args, "brain_dump"), MAX_BODY, "Brain dump")?;
        if title.trim().is_empty() {
            return Err(invalid("Project title is required"));
        }
        if brain_dump.trim().is_empty() {
            return Err(invalid("Brain dump is required before creating a project"));
        }
        let kind = match s(args, "kind") {
            "" | "product" => product_kind(),
            "media" => "media".to_string(),
            _ => return Err(invalid("Project kind must be product or media")),
        };
        let linked_project_id = self.creative_link(&kind, args)?;
        let id = self.new_id("project");
        let stamp = now();
        let project = Project {
            id: id.clone(),
            title,
            brain_dump,
            sources: parse_sources(args)?,
            knowledge: Vec::new(),
            current_domain: initial_domain(),
            gate_reviews: Vec::new(),
            domain_transitions: Vec::new(),
            kind,
            linked_project_id,
            created_at: stamp,
            updated_at: stamp,
        };
        write_json_atomic(
            &self.root.join("projects").join(format!("{id}.json")),
            &project,
        )?;
        self.projects.insert(id.clone(), project.clone());
        Ok(serde_json::to_value(project)?)
    }

    /// The project a creative project is made in alignment with: absent or empty starts it on
    /// its own; otherwise it names an earlier product project.
    fn creative_link(&self, kind: &str, args: &Value) -> Result<Option<String>> {
        let linked = match &args["linked_project_id"] {
            Value::Null => return Ok(None),
            Value::String(v) if v.trim().is_empty() => return Ok(None),
            Value::String(v) => v.trim().to_owned(),
            _ => return Err(invalid("The linked project must be named by its project")),
        };
        if kind != "media" {
            return Err(invalid(
                "Only a creative project links to an earlier project",
            ));
        }
        match self.projects.get(&linked) {
            Some(p) if p.kind == "product" => Ok(Some(linked)),
            Some(_) => Err(invalid(
                "A creative project links to a product project, not another creative one",
            )),
            None => Err(invalid("The linked project was not found")),
        }
    }

    /// Link a creative project to an earlier product project, or clear the link.
    fn project_link(&mut self, args: &Value) -> Result<Value> {
        let id = s(args, "project_id");
        let kind = self
            .projects
            .get(id)
            .ok_or_else(|| invalid("Q-ROC project not found"))?
            .kind
            .clone();
        if kind != "media" {
            return Err(invalid(
                "Only a creative project links to an earlier project",
            ));
        }
        let linked = self.creative_link(&kind, args)?;
        let mut project = self.projects[id].clone();
        project.linked_project_id = linked;
        project.updated_at = now();
        write_json_atomic(
            &self.root.join("projects").join(format!("{id}.json")),
            &project,
        )?;
        self.projects.insert(id.to_owned(), project);
        self.project_detail(id)
    }

    /// Both sides of the creative link, by name: the project a creative project follows,
    /// and the creative projects that follow a product project.
    fn project_links(&self, project: &Project) -> Value {
        let linked = project.linked_project_id.as_ref().map(|lid| {
            self.projects.get(lid).map_or_else(
                || json!({"id":lid,"title":null,"found":false}),
                |p| json!({"id":p.id,"title":p.title,"kind":p.kind,"found":true}),
            )
        });
        let mut creative = self
            .projects
            .values()
            .filter(|p| p.linked_project_id.as_deref() == Some(project.id.as_str()))
            .map(|p| json!({"id":p.id,"title":p.title,"created_at":p.created_at}))
            .collect::<Vec<_>>();
        creative.sort_by_key(|p| p["created_at"].as_i64());
        json!({"linked_project":linked,"linked_creative_projects":creative})
    }

    fn project_detail(&self, id: &str) -> Result<Value> {
        let project = self
            .projects
            .get(id)
            .ok_or_else(|| invalid("Q-ROC project not found"))?;
        let branches = self
            .branches
            .values()
            .filter(|b| b.project_id == id)
            .collect::<Vec<_>>();
        let links = self.project_links(project);
        Ok(
            json!({"project":project,"branches":branches,"domains":self.production_overview(project)?,"documents":{"PRODUCT":PRODUCT_DOCUMENTS,"BUSINESS":BUSINESS_DOCUMENTS,"PRESENCE":PRESENCE_DOCUMENTS,"LEGAL":LEGAL_DOCUMENTS},
                "linked_project":links["linked_project"],"linked_creative_projects":links["linked_creative_projects"]}),
        )
    }

    fn project_import(&mut self, args: &Value) -> Result<Value> {
        let input = Path::new(s(args, "path"));
        let files = collect_import_files(input)?;
        if files.is_empty() {
            return Err(invalid("Q-ROC import folder contains no regular files"));
        }
        let title = bounded_text(
            if s(args, "title").trim().is_empty() {
                input
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Imported Q-ROC project")
            } else {
                s(args, "title")
            },
            MAX_TITLE,
            "Project title",
        )?;
        let project_id = self.new_id("project");
        let branch_id = self.new_id("branch");
        let source_dir = self.root.join("sources").join(&project_id);
        fs::create_dir_all(&source_dir)?;
        let mut sources = Vec::new();
        let mut documents = Vec::new();
        let mut skipped = Vec::new();
        let mut total_bytes = 0_u64;
        for (index, file_path) in files.iter().enumerate() {
            let metadata = fs::symlink_metadata(file_path)?;
            if metadata.len() > MAX_DOCUMENT_BYTES as u64 {
                skipped.push(json!({"path":file_path,"reason":"file exceeds 8 MiB"}));
                continue;
            }
            total_bytes = total_bytes.saturating_add(metadata.len());
            if total_bytes > MAX_SOURCE_BYTES * 2 {
                return Err(invalid("Q-ROC import exceeds the 32 MiB total bound"));
            }
            let bytes = fs::read(file_path)?;
            let sha256 = digest(&bytes);
            let generated_name = format!("source-{index:04}-{sha256}.bin");
            let copied = source_dir.join(generated_name);
            write_bytes_atomic(&copied, &bytes)?;
            let relative_name = file_path
                .strip_prefix(input)
                .unwrap_or(file_path)
                .to_string_lossy()
                .into_owned();
            sources.push(SourceRef {
                id: format!("source-{index:04}"),
                name: relative_name,
                path: copied.to_string_lossy().into_owned(),
                sha256: sha256.clone(),
                size: metadata.len(),
            });
            let Some(document_type) = imported_document_type(file_path) else {
                skipped.push(json!({"path":file_path,"reason":"filename has no supported Q-ROC document type"}));
                continue;
            };
            let content = String::from_utf8_lossy(&bytes).into_owned();
            documents.push((document_type, content));
        }
        let stamp = now();
        let project = Project {
            id: project_id.clone(),
            title,
            brain_dump: format!(
                "Imported local Q-ROC material from {}. Inspect source hashes and document provenance before relying on it.",
                input.display()
            ),
            sources,
            knowledge: Vec::new(),
            current_domain: initial_domain(),
            gate_reviews: Vec::new(),
            domain_transitions: Vec::new(),
            kind: product_kind(),
            linked_project_id: None,
            created_at: stamp,
            updated_at: stamp,
        };
        let branch = Branch {
            id: branch_id.clone(),
            project_id: project_id.clone(),
            name: "Imported material".into(),
            model: "".into(),
            provider: "imported-local".into(),
            status: "ready".into(),
            created_at: stamp,
        };
        write_json_atomic(
            &self
                .root
                .join("projects")
                .join(format!("{project_id}.json")),
            &project,
        )?;
        write_json_atomic(
            &self.root.join("branches").join(format!("{branch_id}.json")),
            &branch,
        )?;
        self.projects.insert(project_id.clone(), project.clone());
        self.branches.insert(branch_id.clone(), branch.clone());
        let imported_count = documents.len();
        for (document_type, content) in documents {
            let revision = next_revision(&self.root, &project_id, &branch_id, &document_type)?;
            let doc =
                self.make_document(&project_id, &branch_id, &document_type, revision, content)?;
            self.persist_document(&doc)?;
        }
        Ok(
            json!({"project":project,"branch":branch,"imported":imported_count,"skipped":skipped,"source_count":self.projects[&project_id].sources.len()}),
        )
    }

    fn project_save(&mut self, args: &Value) -> Result<Value> {
        let id = s(args, "id");
        let mut project = self
            .projects
            .get(id)
            .cloned()
            .ok_or_else(|| invalid("Q-ROC project not found"))?;
        if args.get("title").is_some() {
            project.title = bounded_text(s(args, "title"), MAX_TITLE, "Project title")?;
        }
        if args.get("brain_dump").is_some() {
            project.brain_dump = bounded_text(s(args, "brain_dump"), MAX_BODY, "Brain dump")?;
        }
        if args.get("sources").is_some() {
            project.sources = parse_sources(args)?;
        }
        project.updated_at = now();
        write_json_atomic(
            &self.root.join("projects").join(format!("{id}.json")),
            &project,
        )?;
        self.projects.insert(id.to_owned(), project.clone());
        Ok(serde_json::to_value(project)?)
    }

    fn kb_list(&self, project_id: &str) -> Result<Value> {
        let project = self
            .projects
            .get(project_id)
            .ok_or_else(|| invalid("Q-ROC project not found"))?;
        Ok(
            json!({"items": project.knowledge, "selected": project.knowledge.iter().filter(|item| item.selected).count()}),
        )
    }

    fn kb_save(&mut self, args: &Value) -> Result<Value> {
        let project_id = s(args, "project_id");
        let mut project = self
            .projects
            .get(project_id)
            .cloned()
            .ok_or_else(|| invalid("Q-ROC project not found"))?;
        let title = bounded_text(s(args, "title"), MAX_TITLE, "Knowledge item title")?;
        let content = bounded_text(s(args, "content"), MAX_BODY, "Knowledge item")?;
        let kind = bounded_text(
            if s(args, "kind").is_empty() {
                "note"
            } else {
                s(args, "kind")
            },
            40,
            "Knowledge item type",
        )?;
        if title.trim().is_empty() || content.trim().is_empty() {
            return Err(invalid("Knowledge item title and content are required"));
        }
        let id = if s(args, "id").is_empty() {
            self.new_id("kb")
        } else {
            safe_component(s(args, "id"), "knowledge item id")?
        };
        let stamp = now();
        let item = if let Some(previous) = project.knowledge.iter_mut().find(|item| item.id == id) {
            previous.title = title;
            previous.kind = kind;
            previous.content = content;
            previous.selected = args
                .get("selected")
                .and_then(Value::as_bool)
                .unwrap_or(previous.selected);
            previous.updated_at = stamp;
            previous.clone()
        } else {
            let item = KnowledgeItem {
                id: id.clone(),
                project_id: project_id.into(),
                kind,
                title,
                content,
                source_path: (!s(args, "source_path").is_empty())
                    .then(|| s(args, "source_path").to_owned()),
                selected: args
                    .get("selected")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                created_at: stamp,
                updated_at: stamp,
            };
            project.knowledge.push(item.clone());
            item
        };
        project.updated_at = stamp;
        write_json_atomic(
            &self
                .root
                .join("projects")
                .join(format!("{project_id}.json")),
            &project,
        )?;
        self.projects.insert(project_id.to_owned(), project);
        Ok(serde_json::to_value(item)?)
    }

    fn kb_remove(&mut self, args: &Value) -> Result<Value> {
        let project_id = s(args, "project_id");
        let item_id = s(args, "id");
        let mut project = self
            .projects
            .get(project_id)
            .cloned()
            .ok_or_else(|| invalid("Q-ROC project not found"))?;
        let before = project.knowledge.len();
        project.knowledge.retain(|item| item.id != item_id);
        if before == project.knowledge.len() {
            return Err(invalid("Knowledge item not found"));
        }
        project.updated_at = now();
        write_json_atomic(
            &self
                .root
                .join("projects")
                .join(format!("{project_id}.json")),
            &project,
        )?;
        self.projects.insert(project_id.to_owned(), project);
        Ok(json!({"removed":item_id}))
    }

    fn kb_context(&self, project_id: &str) -> Result<Value> {
        let project = self
            .projects
            .get(project_id)
            .ok_or_else(|| invalid("Q-ROC project not found"))?;
        let selected = project
            .knowledge
            .iter()
            .filter(|item| item.selected)
            .collect::<Vec<_>>();
        let context = selected
            .iter()
            .map(|item| format!("## {} ({})\n{}", item.title, item.kind, item.content))
            .collect::<Vec<_>>()
            .join("\n\n");
        Ok(
            json!({"project_id":project_id,"items":selected,"context":context,"evidence":"Context assembled from explicitly selected local knowledge items; no quality score or synthesis claim is made."}),
        )
    }

    fn skills_import(&mut self, args: &Value) -> Result<Value> {
        let project_id = s(args, "project_id");
        if !self.projects.contains_key(project_id) {
            return Err(invalid("Q-ROC project not found"));
        }
        let root = Path::new(s(args, "path"));
        let files = collect_import_files(root)?;
        let mut imported = Vec::new();
        let mut skipped = Vec::new();
        for path in files {
            let is_skill = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"));
            let markdown = path.extension().and_then(|ext| ext.to_str()) == Some("md");
            if !is_skill && !markdown {
                continue;
            }
            let bytes = fs::read(&path)?;
            if bytes.len() > MAX_BODY {
                skipped.push(json!({"path":path,"reason":"skill exceeds 2 MiB"}));
                continue;
            }
            let title = path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("Imported skill")
                .to_owned();
            let saved = self.kb_save(&json!({
                "project_id":project_id,
                "title":title,
                "kind":"skill",
                "content":String::from_utf8_lossy(&bytes),
                "source_path":path,
                "selected":true
            }))?;
            imported.push(saved);
        }
        Ok(
            json!({"project_id":project_id,"imported":imported,"skipped":skipped,"executed":false,"notice":"Imported skill text is selected knowledge only; no code was executed."}),
        )
    }

    fn branch_list(&self, project_id: &str) -> Result<Value> {
        Ok(
            json!({"branches": self.branches.values().filter(|b| b.project_id == project_id).collect::<Vec<_>>() }),
        )
    }

    fn branch_create(&mut self, args: &Value) -> Result<Value> {
        let project_id = s(args, "project_id");
        if !self.projects.contains_key(project_id) {
            return Err(invalid("Q-ROC project not found"));
        }
        let name = bounded_text(s(args, "name"), MAX_TITLE, "Branch name")?;
        let model = bounded_text(
            if s(args, "model").is_empty() {
                ""
            } else {
                s(args, "model")
            },
            200,
            "Model",
        )?;
        let provider = bounded_text(
            if s(args, "provider").is_empty() {
                "unavailable"
            } else {
                s(args, "provider")
            },
            200,
            "Provider",
        )?;
        if name.trim().is_empty() {
            return Err(invalid("Branch name is required"));
        }
        let id = self.new_id("branch");
        let branch = Branch {
            id: id.clone(),
            project_id: project_id.into(),
            name,
            model,
            provider,
            status: "ready".into(),
            created_at: now(),
        };
        write_json_atomic(
            &self.root.join("branches").join(format!("{id}.json")),
            &branch,
        )?;
        self.branches.insert(id, branch.clone());
        Ok(serde_json::to_value(branch)?)
    }

    fn production_overview(&self, project: &Project) -> Result<Vec<Value>> {
        let mut kinds = std::collections::BTreeSet::new();
        for branch in self
            .branches
            .values()
            .filter(|b| b.project_id == project.id)
        {
            for path in document_paths(&self.root, &project.id, &branch.id)? {
                let document: DocumentRevision = read_json(&path)?;
                if document.project_id != project.id
                    || document.branch_id != branch.id
                    || digest(document.content.as_bytes()) != document.sha256
                {
                    return Err(invalid("Domain document owner/digest mismatch"));
                }
                kinds.insert(document.document_type);
            }
        }
        let path = self
            .root
            .join("pipeline")
            .join(&project.id)
            .join("Pipeline run.json");
        let complete = if path.exists() {
            let record: Value = read_json(&path)?;
            record["project_id"] == project.id
                && record["status"] == "completed"
                && record["steps"].as_array().is_some_and(|steps| {
                    steps.len() == crate::pipeline::STEPS.len()
                        && steps.iter().all(|step| step["status"] == "completed")
                })
        } else {
            false
        };
        crate::scope::domain_overview(&kinds, complete)
    }

    fn document_list(&self, args: &Value) -> Result<Value> {
        let project_id = s(args, "project_id");
        let branch_id = s(args, "branch_id");
        let mut documents = Vec::new();
        for path in document_paths(&self.root, project_id, branch_id)? {
            if let Ok(doc) = read_json::<DocumentRevision>(&path) {
                documents.push(doc);
            }
        }
        documents.sort_by_key(|d| {
            (
                document_order(&d.document_type),
                std::cmp::Reverse(d.revision),
            )
        });
        let page = crate::bridge::page(&documents, args)?;
        Ok(page.reply(
            "documents",
            json!({"domains":self.production_overview(self.projects.get(project_id).ok_or_else(|| invalid("Q-ROC project not found"))?)?,"document_order":document_order_map()}),
        ))
    }

    fn document_detail(&self, args: &Value) -> Result<Value> {
        let path = self.document_path(args)?;
        let doc: DocumentRevision = read_json(&path)?;
        Ok(serde_json::to_value(doc)?)
    }

    fn document_save(&mut self, args: &Value) -> Result<Value> {
        if !crate::harden::Hardening::open(&self.root)?.passed(s(args, "project_id"))? {
            return Err(invalid(
                "Hardening gate has not passed; answer the hardening questions before writing documents",
            ));
        }
        let project_id = s(args, "project_id");
        let branch_id = s(args, "branch_id");
        let document_type = normalized_document(s(args, "document_type"))?;
        if !self.projects.contains_key(project_id)
            || self
                .branches
                .get(branch_id)
                .is_none_or(|b| b.project_id != project_id)
        {
            return Err(invalid("Project and branch must belong together"));
        }
        let content = bounded_text(s(args, "content"), MAX_DOCUMENT_BYTES, "Document content")?;
        if content.trim().is_empty() {
            return Err(invalid("Document content cannot be empty"));
        }
        let current_revision = current_revision(&self.root, project_id, branch_id, &document_type)?;
        if current_revision > 0 && args.get("source_revision").is_none() {
            return Err(invalid(
                "Exact source_revision is required to edit an existing document",
            ));
        }
        if let Some(expected) = args.get("source_revision") {
            let expected = expected
                .as_u64()
                .ok_or_else(|| invalid("source_revision must be a non-negative integer"))?;
            let expected = u32::try_from(expected)
                .map_err(|_| invalid("source_revision exceeds the supported range"))?;
            if expected != current_revision {
                return Err(invalid(format!(
                    "Document changed since revision {expected}; current revision is {current_revision}"
                )));
            }
        }
        let revision = next_revision(&self.root, project_id, branch_id, &document_type)?;
        let mut doc =
            self.make_document(project_id, branch_id, &document_type, revision, content)?;
        doc.source_revision =
            Some(args["source_revision"].as_u64().unwrap_or(0) as u32).filter(|v| *v > 0);
        self.write_document(&doc)?;
        Ok(serde_json::to_value(doc)?)
    }

    fn document_history(&self, args: &Value) -> Result<Value> {
        let project_id = s(args, "project_id");
        let branch_id = s(args, "branch_id");
        let document_type = normalized_document(s(args, "document_type"))?;
        let mut docs = Vec::new();
        for path in document_paths(&self.root, project_id, branch_id)? {
            if let Ok(doc) = read_json::<DocumentRevision>(&path)
                && doc.document_type == document_type
            {
                docs.push(doc);
            }
        }
        docs.sort_by_key(|d| d.revision);
        let page = crate::bridge::page(&docs, args)?;
        Ok(page.reply("items", json!({})))
    }

    fn draft_key(&self, args: &Value) -> Result<(String, String, String)> {
        let project_id = safe_component(s(args, "project_id"), "project_id")?;
        let branch_id = safe_component(s(args, "branch_id"), "branch_id")?;
        let document_type = normalized_document(s(args, "document_type"))?;
        if !self.projects.contains_key(&project_id)
            || self
                .branches
                .get(&branch_id)
                .is_none_or(|branch| branch.project_id != project_id)
        {
            return Err(invalid("Project and branch must belong together"));
        }
        Ok((project_id, branch_id, document_type))
    }

    fn draft_path(&self, project_id: &str, branch_id: &str, document_type: &str) -> PathBuf {
        self.root
            .join("drafts")
            .join(project_id)
            .join(branch_id)
            .join(format!("{document_type}.json"))
    }

    fn draft_save(&self, args: &Value) -> Result<Value> {
        if !crate::harden::Hardening::open(&self.root)?.passed(s(args, "project_id"))? {
            return Err(invalid(
                "Hardening gate has not passed; answer the hardening questions before writing documents",
            ));
        }
        let (project_id, branch_id, document_type) = self.draft_key(args)?;
        let content = bounded_text(s(args, "content"), MAX_BODY, "Draft content")?;
        let base_revision = match args.get("base_revision") {
            None | Some(Value::Null) => Value::Null,
            Some(value) => {
                let revision = value
                    .as_u64()
                    .ok_or_else(|| invalid("Draft base_revision must be an integer"))?;
                if revision > u32::MAX as u64 {
                    return Err(invalid("Draft base_revision is too large"));
                }
                json!(revision as u32)
            }
        };
        let path = self.draft_path(&project_id, &branch_id, &document_type);
        if let Ok(metadata) = fs::symlink_metadata(&path)
            && metadata.file_type().is_symlink()
        {
            return Err(invalid("Draft path cannot be a symlink"));
        }
        let draft = json!({
            "project_id": project_id,
            "branch_id": branch_id,
            "document_type": document_type,
            "content": content,
            "base_revision": base_revision,
            "updated_at": now()
        });
        write_json_atomic(&path, &draft)?;
        Ok(
            json!({"content":draft["content"],"base_revision":draft["base_revision"],"updated_at":draft["updated_at"]}),
        )
    }

    fn draft_get(&self, args: &Value) -> Result<Value> {
        let (project_id, branch_id, document_type) = self.draft_key(args)?;
        let path = self.draft_path(&project_id, &branch_id, &document_type);
        if !path.exists() {
            return Ok(json!({}));
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid("Draft path is not a regular file"));
        }
        let draft: Value = read_json(&path)?;
        Ok(
            json!({"content":draft.get("content").cloned().unwrap_or_else(|| json!("")),"base_revision":draft.get("base_revision").cloned().unwrap_or(Value::Null),"updated_at":draft.get("updated_at").cloned().unwrap_or(Value::Null)}),
        )
    }

    fn draft_clear(&self, args: &Value) -> Result<Value> {
        let (project_id, branch_id, document_type) = self.draft_key(args)?;
        let path = self.draft_path(&project_id, &branch_id, &document_type);
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() {
                return Err(invalid("Draft path cannot be a symlink"));
            }
            fs::remove_file(&path)?;
            if let Some(parent) = path.parent() {
                File::open(parent)?.sync_all()?;
            }
        }
        Ok(
            json!({"cleared":true,"project_id":project_id,"branch_id":branch_id,"document_type":document_type}),
        )
    }

    fn generate_start(&mut self, args: &Value) -> Result<Value> {
        self.generate_start_after(args, None)
    }

    fn generate_start_after(&mut self, args: &Value, after_job: Option<String>) -> Result<Value> {
        if !crate::harden::Hardening::open(&self.root)?.passed(s(args, "project_id"))? {
            return Err(invalid(
                "Hardening gate has not passed; answer the hardening questions before writing documents",
            ));
        }
        if after_job.is_none() && self.running.len() >= MAX_JOBS {
            return Err(invalid(
                "Too many Q-ROC generation jobs; inspect or cancel an existing job",
            ));
        }
        let project_id = s(args, "project_id").to_owned();
        let branch_id = s(args, "branch_id").to_owned();
        let document_type = normalized_document(s(args, "document_type"))?;
        let project = self
            .projects
            .get(&project_id)
            .cloned()
            .ok_or_else(|| invalid("Q-ROC project not found"))?;
        if project.kind == "product"
            && domain_for(&document_type) != "PRODUCT"
            && !self
                .production_overview(&project)?
                .iter()
                .any(|d| d["id"] == "PRODUCT" && d["status"] == "produced")
        {
            return Err(invalid(
                "Complete the Product pipeline before generating a later-domain portion",
            ));
        }
        let branch = self
            .branches
            .get(&branch_id)
            .cloned()
            .filter(|b| b.project_id == project_id)
            .ok_or_else(|| invalid("Q-ROC branch not found"))?;
        let (_, declared_model) = crate::registry::provider_config_at(&self.root)?;
        if branch.provider != crate::registry::provider_name_at(&self.root)?
            || branch.model != declared_model
        {
            return Err(invalid(
                "capability-unavailable: branch provider/model does not match the declared local provider",
            ));
        }
        let extra_context = bounded_text(s(args, "context"), MAX_BODY, "Context")?;
        self.generation_context(&project, &branch, &document_type, &extra_context)?;
        let base_revision = current_revision(&self.root, &project_id, &branch_id, &document_type)?;
        let timeout_seconds = self.generation_wait_seconds()?;
        let max_tokens =
            generation_max_tokens(&self.root, std::env::var("QROC_MAX_TOKENS").ok().as_deref())?;
        let job_id = self.new_id("job");
        let job = JobRecord {
            id: job_id.clone(),
            project_id: project_id.clone(),
            branch_id: branch_id.clone(),
            document_type: document_type.clone(),
            state: if after_job.is_some() {
                "queued"
            } else {
                "running"
            }
            .into(),
            after_job: after_job.clone(),
            batch_id: None,
            batch_width: 0,
            dependency_jobs: Vec::new(),
            queued_context: extra_context,
            pinned_inputs: None,
            hardening_round: None,
            gate_revise: args.get("gate_revise").cloned(),
            model: branch.model.clone(),
            timeout_seconds,
            attempts: Vec::new(),
            max_tokens,
            honoured_max_tokens: None,
            error: None,
            document_id: None,
            base_revision: Some(base_revision),
            output_sha256: None,
            generation_finish_reason: None,
            conflict_path: None,
            synthesis: args
                .get("synthesis_provenance")
                .cloned()
                .map(serde_json::from_value)
                .transpose()?,
            started_at: now(),
            finished_at: None,
        };
        self.persist_job(&job)?;
        self.jobs.insert(job_id.clone(), job);
        if after_job.is_none() {
            self.launch_job(&job_id)?;
        }
        Ok(serde_json::to_value(
            self.jobs.get(&job_id).expect("job was inserted"),
        )?)
    }

    fn launch_job(&mut self, job_id: &str) -> Result<()> {
        let mut job = self
            .jobs
            .get(job_id)
            .cloned()
            .ok_or_else(|| invalid("Queued job no longer exists"))?;
        let mut project = self
            .projects
            .get(&job.project_id)
            .cloned()
            .ok_or_else(|| invalid("Q-ROC project not found"))?;
        let hardening = crate::harden::Hardening::open(&self.root)?;
        if !hardening.passed(&job.project_id)? {
            return Err(invalid("Hardening gate no longer passes"));
        }
        let record = hardening.load(&job.project_id)?;
        let round = record
            .rounds
            .last()
            .ok_or_else(|| invalid("Passed hardening round missing"))?;
        project.brain_dump = round.idea_text.clone();
        job.hardening_round = Some(round.round);
        let branch = self
            .branches
            .get(&job.branch_id)
            .cloned()
            .ok_or_else(|| invalid("Q-ROC branch not found"))?;
        let document_type = job.document_type.clone();
        // Pin context only when this worker starts, after its predecessor commits.
        let mut generation_context =
            self.generation_context(&project, &branch, &document_type, &job.queued_context)?;
        let mut consumed_documents = BTreeMap::new();
        let revise = job.gate_revise.is_some();
        for kind in crate::registry::product_inputs(&document_type).unwrap_or(&[]) {
            if *kind != "IDEA" {
                // A synthesis job carries its own exact source revisions; it binds the
                // target branch's inputs that exist rather than requiring every one.
                let document =
                    match self.revise_input_document(revise, &project.id, &branch.id, kind) {
                        Ok(document) => document,
                        Err(_) if job.synthesis.is_some() => continue,
                        Err(error) => return Err(error),
                    };
                consumed_documents.insert(
                    kind.to_string(),
                    json!({"id":document.id,"revision":document.revision,"sha256":document.sha256}),
                );
            }
        }
        if let Some(tag) = job.gate_revise.as_mut() {
            if let Some(feedback) = tag["feedback"].as_str() {
                generation_context.push_str(&format!("\n\nGate feedback to answer\n{feedback}"));
                generation_context.push_str("\n\nRevision binding: Earlier branch document headers identify the completed dependencies consumed by this generation. This round's documents are revised one after another from one shared, current baseline. Each Revision baseline section is that sibling's current revision: a sibling revised earlier in this round appears at its new revision, and this document agrees with it; a sibling not yet revised is current and is revised after this document, following the gate's repair lines. Where siblings conflict, the gate's repair lines say which document changes; no sibling is set aside as out of date. Cite completed dependencies and baseline siblings by their supplied identities; do not copy superseded revision/hash references or assertions that companions are pending, and do not invent future IDs or hashes. Preserve unresolved requirements as unresolved.\n");
            }
            if let Some(baseline) = tag["baseline"].as_object() {
                let mut bound = BTreeMap::new();
                for (kind, value) in baseline {
                    // The gate judges only the completed join already in its context.
                    // Earlier-stage PRD/architecture remain current required inputs.
                    if consumed_documents.contains_key(kind) {
                        continue;
                    }
                    // The round runs as one chain, so the current revision is the shared
                    // baseline: a sibling revised earlier in the round at its new revision,
                    // any other as the round found it. The consumed identity is recorded.
                    let mut document: DocumentRevision =
                        match self.revise_input_document(revise, &project.id, &branch.id, kind) {
                            Ok(document) => document,
                            Err(_) => serde_json::from_value(value.clone())?,
                        };
                    document.generation_inputs = None;
                    if document.project_id != project.id
                        || document.branch_id != branch.id
                        || digest(document.content.as_bytes()) != document.sha256
                    {
                        return Err(invalid("Revise baseline owner/digest mismatch"));
                    }
                    generation_context.push_str(&format!(
                        "\nRevision baseline {kind} revision {} id {} sha256 {}\n{}\n",
                        document.revision, document.id, document.sha256, document.content
                    ));
                    bound.insert(kind.clone(), json!({"id":document.id,"revision":document.revision,"sha256":document.sha256}));
                }
                tag["consumed_baseline"] = json!(bound);
            }
        }
        job.base_revision = Some(current_revision(
            &self.root,
            &job.project_id,
            &job.branch_id,
            &document_type,
        )?);
        let (endpoint, declared_model) = crate::registry::provider_config_at(&self.root)?;
        if branch.provider != crate::registry::provider_name_at(&self.root)?
            || branch.model != declared_model
        {
            return Err(invalid(
                "capability-unavailable: branch provider/model mismatch",
            ));
        }
        let scope = self.scope_for(&project.id, &branch.id)?;
        job.pinned_inputs = Some(
            json!({"schema":"qroc-generation-inputs/1","job_id":job.id,"hardening_round":job.hardening_round,"project":project,"branch":branch,"scope":scope,"consumed_documents":consumed_documents,"context":generation_context,"context_sha256":digest(generation_context.as_bytes()),"prompt_sha256":digest(qroc_prompt(&project,&branch,&document_type,&generation_context).as_bytes()),"registry_version":crate::registry::VERSION,"semantic_profile":if crate::registry::supports_semantic(&document_type){Some(crate::registry::SEMANTIC_PROFILE)}else{None},"semantic_profile_sha256":if crate::registry::supports_semantic(&document_type){Some(crate::registry::profile_sha256())}else{None},"semantic_source_identity":if crate::registry::supports_semantic(&document_type){Some(crate::registry::semantic_sources())}else{None},"compiler_version":if crate::registry::supports_semantic(&document_type){"qroc-source-semantic-compiler/1"}else{"qroc-native-compiler/1"},"prompt_builder_sha256":PROMPT_BUILDER_SHA256,"prompt_compiler_sha256":PROMPT_COMPILER_SHA256,"endpoint":endpoint,"model":declared_model,"provider":branch.provider,"timeout_seconds":job.timeout_seconds,"max_tokens":job.max_tokens,"base_revision":job.base_revision,"gate_revise":job.gate_revise}),
        );
        job.state = "running".into();
        job.started_at = now();
        let timeout_seconds = job.timeout_seconds;
        let max_tokens = job.max_tokens;
        if !(1..=generation_token_ceiling(&self.root)?).contains(&max_tokens) {
            return Err(invalid("Missing/invalid pinned QROC_MAX_TOKENS"));
        }
        self.persist_job(&job)?;
        self.jobs.insert(job_id.into(), job);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let (sender, receiver) = mpsc::channel();
        let route_root = self.root.clone();
        thread::spawn(move || {
            let result = generate_from_gateway(
                &project,
                &branch,
                &document_type,
                &generation_context,
                &worker_cancel,
                GatewayCall {
                    root: &route_root,
                    endpoint: &endpoint,
                    timeout_seconds,
                    max_tokens,
                },
            );
            let _ = sender.send(result);
        });
        self.running
            .insert(job_id.into(), RunningJob { cancel, receiver });
        Ok(())
    }

    fn synthesis_start(&mut self, args: &Value) -> Result<Value> {
        if !crate::harden::Hardening::open(&self.root)?.passed(s(args, "project_id"))? {
            return Err(invalid(
                "Hardening gate has not passed; answer the hardening questions before writing documents",
            ));
        }
        let project_id = s(args, "project_id");
        let target_branch_id = s(args, "target_branch_id");
        let source_branches = args
            .get("source_branch_ids")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("source_branch_ids must be an array"))?;
        if source_branches.is_empty() || source_branches.len() > 16 {
            return Err(invalid("Choose between 1 and 16 source branches"));
        }
        let target = self
            .branches
            .get(target_branch_id)
            .filter(|branch| branch.project_id == project_id)
            .ok_or_else(|| invalid("Target branch not found for project"))?;
        if source_branches
            .iter()
            .any(|value| value.as_str() == Some(target_branch_id))
        {
            return Err(invalid(
                "Synthesis target branch must be new and separate from sources",
            ));
        }
        if !document_paths(&self.root, project_id, target_branch_id)?.is_empty() {
            return Err(invalid(
                "Synthesis target branch already contains documents",
            ));
        }
        let document_type = normalized_document(s(args, "document_type"))?;
        let mut context = String::new();
        let mut source_documents = Vec::new();
        let mut source_branch_ids = Vec::new();
        for source_id in source_branches {
            let source_id = source_id
                .as_str()
                .ok_or_else(|| invalid("source branch id must be text"))?;
            let source = self
                .branches
                .get(source_id)
                .filter(|branch| branch.project_id == project_id)
                .ok_or_else(|| invalid("Source branch not found for project"))?;
            if source_branch_ids.contains(&source.id) {
                return Err(invalid("Duplicate synthesis source branch"));
            }
            source_branch_ids.push(source.id.clone());
            context.push_str(&format!(
                "\n\n## Pinned source branch {} ({})\n",
                source.name, source.id
            ));
            let docs = if let Some(selections) = args.get("source_revisions") {
                let revision = selections
                    .get(source_id)
                    .and_then(Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok())
                    .filter(|r| *r > 0)
                    .ok_or_else(|| {
                        invalid(
                            "Exact synthesis source revision required for every selected branch",
                        )
                    })?;
                vec![read_json::<DocumentRevision>(&self.document_path(&json!({"project_id":project_id,"branch_id":source_id,"document_type":document_type,"revision":revision}))?)?]
            } else {
                // Preserve retained full-branch synthesis: every supplied revision is named.
                let mut docs = document_paths(&self.root, project_id, source_id)?
                    .into_iter()
                    .map(|path| read_json::<DocumentRevision>(&path))
                    .collect::<Result<Vec<_>>>()?;
                docs.sort_by_key(|d| (document_order(&d.document_type), d.revision));
                docs
            };
            if docs.is_empty() {
                return Err(invalid("Synthesis source has no selected document"));
            }
            for doc in &docs {
                if digest(doc.content.as_bytes()) != doc.sha256 {
                    return Err(invalid("Synthesis source digest mismatch"));
                }
            }
            for doc in docs {
                source_documents.push(SynthesisSource {
                    branch_id: source.id.clone(),
                    branch_name: source.name.clone(),
                    document_id: doc.id.clone(),
                    document_type: doc.document_type.clone(),
                    revision: doc.revision,
                    sha256: doc.sha256.clone(),
                });
                context.push_str(&format!(
                    "\n### {} revision {} sha256 {}\n{}\n",
                    doc.document_type, doc.revision, doc.sha256, doc.content
                ));
            }
        }
        let mut payload = args.clone();
        let provenance = SynthesisProvenance {
            source_branch_ids,
            source_documents,
            target_branch_id: target_branch_id.to_owned(),
            document_type: document_type.clone(),
        };
        payload["branch_id"] = json!(target.id);
        payload["context"] = json!(bounded_text(&context, MAX_BODY, "Synthesis context")?);
        payload["document_type"] = json!(document_type);
        payload["synthesis_provenance"] = serde_json::to_value(&provenance)?;
        let mut result = self.generate_start(&payload)?;
        result["synthesis"] = json!({"provenance":provenance,"context_pinned":true});
        Ok(result)
    }

    fn context_preview(&self, args: &Value) -> Result<Value> {
        let project = self
            .projects
            .get(s(args, "project_id"))
            .ok_or_else(|| invalid("Project not found"))?;
        let branch = self
            .branches
            .get(s(args, "branch_id"))
            .filter(|b| b.project_id == project.id)
            .ok_or_else(|| invalid("Branch not found"))?;
        let document_type = normalized_document(s(args, "document_type"))?;
        let context =
            self.generation_context(project, branch, &document_type, &project.brain_dump)?;
        Ok(
            json!({"project_id":project.id,"branch_id":branch.id,"document_type":document_type,"context":context,"evidence":"Exact current generation context: selected knowledge, verified attachments, earlier branch documents and project notes. Omitted content is identified below. Document-specific generation instructions are applied separately."}),
        )
    }

    fn scope_for(&self, project: &str, branch: &str) -> Result<Option<Value>> {
        match self.root.parent() {
            Some(parent) => crate::scope::ScopeStore::open(&parent.join("scope"))?
                .current(&json!({"project_id":project,"branch_id":branch})),
            None => Ok(None),
        }
    }

    /// What a job reads as a sibling's current document. A revise writer never reads a draft
    /// the provider cut short: it reads the latest complete revision, and the partial stays on
    /// disk as evidence.
    fn revise_input_document(
        &self,
        revise: bool,
        project: &str,
        branch: &str,
        kind: &str,
    ) -> Result<DocumentRevision> {
        let current = self.semantic_current_document(project, branch, kind)?;
        if !revise
            || current
                .generation_finish_reason
                .as_deref()
                .is_none_or(|r| r == "stop")
        {
            return Ok(current);
        }
        let mut complete: Option<DocumentRevision> = None;
        for path in document_paths(&self.root, project, branch)? {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !name.starts_with(&format!("{kind}-r")) {
                continue;
            }
            let source: DocumentRevision = read_json(&path)?;
            if source.document_type == kind
                && source
                    .generation_finish_reason
                    .as_deref()
                    .is_none_or(|r| r == "stop")
                && complete
                    .as_ref()
                    .is_none_or(|d| d.revision < source.revision)
            {
                complete = Some(source);
            }
        }
        complete.ok_or_else(|| invalid(format!("{kind} has no complete revision to read")))
    }

    fn semantic_current_document(
        &self,
        project: &str,
        branch: &str,
        kind: &str,
    ) -> Result<DocumentRevision> {
        let mut latest: Option<DocumentRevision> = None;
        for path in document_paths(&self.root, project, branch)? {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !name.starts_with(&format!("{kind}-r")) {
                continue;
            }
            let source: DocumentRevision = read_json(&path)?;
            if source.project_id != project
                || source.branch_id != branch
                || source.document_type != kind
                || source.revision == 0
                || name != format!("{kind}-r{}.json", source.revision)
                || digest(source.content.as_bytes()) != source.sha256
            {
                return Err(invalid("Semantic source owner/revision/digest mismatch"));
            }
            if latest.as_ref().is_none_or(|d| d.revision < source.revision) {
                latest = Some(source);
            }
        }
        latest.ok_or_else(|| invalid("Missing required semantic reviewed document"))
    }

    fn evaluate_owned_gate(&self, doc: &DocumentRevision) -> GateResult {
        if doc.document_type != "GATE_4" {
            return evaluate_gate(doc);
        }
        if doc
            .generation_finish_reason
            .as_deref()
            .is_some_and(|r| r != "stop")
        {
            return GateResult {
                document_type: doc.document_type.clone(),
                status: "incomplete".into(),
                score: None,
                criteria: BTreeMap::new(),
                threshold: Some(90),
                reason: "Truncated semantic generation cannot pass".into(),
                evaluated_at: now(),
                semantic: Some(
                    json!({"profile":crate::registry::SEMANTIC_PROFILE,"standing":"incomplete; no semantic pass"}),
                ),
                questions: Vec::new(),
                repairs: Vec::new(),
                questions_note: None,
            };
        }
        let result = (|| -> Result<GateResult> {
            if digest(doc.content.as_bytes()) != doc.sha256 {
                return Err(invalid("Gate document digest mismatch"));
            }
            let profile = crate::registry::semantic_profile()?;
            let mut owned = BTreeMap::new();
            for kind in profile["gates"][0]["reviewedDocuments"].as_array().unwrap() {
                let kind = kind.as_str().unwrap();
                let source =
                    self.semantic_current_document(&doc.project_id, &doc.branch_id, kind)?;
                owned.insert(kind.to_string(), serde_json::to_value(source)?);
            }
            let mut value = crate::registry::evaluate_semantic(
                &doc.content,
                doc.generation_finish_reason.as_deref(),
                &owned,
            )?;
            value["evaluated_at"] = json!(now());
            Ok(serde_json::from_value(value)?)
        })();
        result.unwrap_or_else(|e|GateResult{document_type:doc.document_type.clone(),status:"failed".into(),score:None,criteria:BTreeMap::new(),threshold:Some(90),reason:e.to_string(),evaluated_at:now(),semantic:Some(json!({"profile":crate::registry::SEMANTIC_PROFILE,"standing":"semantic binding failed; no pass"})),questions:Vec::new(),repairs:Vec::new(),questions_note:None})
    }

    fn generation_context(
        &self,
        project: &Project,
        branch: &Branch,
        document_type: &str,
        extra_context: &str,
    ) -> Result<String> {
        let mut context = String::new();
        let semantic_model = crate::registry::semantic_model(document_type)?;
        let allowed_semantic = semantic_model
            .as_ref()
            .map(|m| {
                if document_type == "GATE_4" {
                    m["reviewedDocuments"].as_array().unwrap()
                } else {
                    m["inputPolicy"]["allowedUpstream"].as_array().unwrap()
                }
            })
            .map(|v| {
                v.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            });
        if let Some(model) = &semantic_model {
            context.push_str(&format!(
                "Source-backed semantic model {} sha256 {}: {}\n",
                crate::registry::SEMANTIC_PROFILE,
                crate::registry::profile_sha256(),
                model
            ));
            if document_type == "GATE_4" {
                let scope = self.scope_for(&project.id, &branch.id)?;
                for kind in allowed_semantic.as_ref().unwrap() {
                    if scope.as_ref().is_some_and(|v| {
                        !v["included"]
                            .as_array()
                            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(kind)))
                    }) {
                        return Err(invalid("Required semantic gate source excluded by scope"));
                    }
                    self.semantic_current_document(&project.id, &branch.id, kind)?;
                }
            }
        }
        if let Some(parent) = self.root.parent() {
            let scope_store = crate::scope::ScopeStore::open(&parent.join("scope"))?;
            if let Some(scope) =
                scope_store.current(&json!({"project_id":project.id,"branch_id":branch.id}))?
            {
                if !scope["included"]
                    .as_array()
                    .is_some_and(|v| v.iter().any(|v| v.as_str() == Some(document_type)))
                {
                    return Err(invalid("Document excluded by the current branch scope"));
                }
                context.push_str(&format!(
                    "Pinned branch scope (exclusions do not erase constraints): {}\n",
                    scope
                ));
            }
        }
        let mut omitted = Vec::new();
        for item in project.knowledge.iter().filter(|item| item.selected) {
            append_context(
                &mut context,
                &mut omitted,
                format!("Selected knowledge: {} [{}]", item.title, item.kind),
                item.content.clone(),
            );
        }
        for source in &project.sources {
            let bytes = fs::read(&source.path)?;
            let actual_hash = digest(&bytes);
            if actual_hash != source.sha256 {
                return Err(invalid(format!(
                    "Attached source changed since it was recorded: {}",
                    source.name
                )));
            }
            let header = format!(
                "Attached source: {}\npath: {}\nsha256: {}\nsize: {}\n",
                source.name, source.path, source.sha256, source.size
            );
            let body = if bytes.len() > MAX_ATTACHMENT_CONTEXT_BYTES {
                format!("{header}[content omitted: exceeds per-source context bound]")
            } else {
                format!("{header}{}", String::from_utf8_lossy(&bytes))
            };
            append_context(
                &mut context,
                &mut omitted,
                format!("Pinned attachment: {}", source.name),
                body,
            );
            if bytes.len() > MAX_ATTACHMENT_CONTEXT_BYTES {
                omitted.push(format!("Attached source {} content", source.name));
            }
        }
        let mut prior = Vec::new();
        for path in document_paths(&self.root, &project.id, &branch.id)? {
            let record = read_json::<DocumentRevision>(&path);
            if semantic_model.is_some() && record.is_err() {
                return Err(invalid(
                    "Malformed branch record while compiling semantic context",
                ));
            }
            if let Ok(doc) = record
                && doc.document_type != document_type
                && document_order(&doc.document_type) < document_order(document_type)
                && allowed_semantic
                    .as_ref()
                    .is_none_or(|v| v.contains(&doc.document_type))
                && crate::registry::product_inputs(document_type)
                    .is_none_or(|v| v.contains(&doc.document_type.as_str()))
            {
                if semantic_model.is_some()
                    && (doc.project_id != project.id
                        || doc.branch_id != branch.id
                        || digest(doc.content.as_bytes()) != doc.sha256)
                {
                    return Err(invalid("Semantic upstream owner/digest mismatch"));
                }
                prior.push(doc);
            }
        }
        prior.sort_by_key(|doc| (document_order(&doc.document_type), doc.revision));
        let mut latest = BTreeMap::new();
        for doc in prior {
            latest.insert(doc.document_type.clone(), doc);
        }
        for doc in latest.values() {
            append_context(
                &mut context,
                &mut omitted,
                format!(
                    "Earlier branch document: {} id {} revision {} sha256 {}",
                    doc.document_type, doc.id, doc.revision, doc.sha256
                ),
                doc.content.clone(),
            );
        }
        if !extra_context.trim().is_empty() {
            append_context(
                &mut context,
                &mut omitted,
                "Operator supplied context".into(),
                extra_context.to_owned(),
            );
        }
        if !omitted.is_empty() {
            context.push_str(&format!(
                "\n\nContext omissions (the model must not infer omitted material): {}",
                omitted.join(", ")
            ));
        }
        Ok(context)
    }

    fn job_list(&self, args: &Value) -> Result<Value> {
        let project_id = s(args, "project_id");
        let jobs = self
            .jobs
            .values()
            .filter(|j| project_id.is_empty() || j.project_id == project_id)
            .collect::<Vec<_>>();
        let page = crate::bridge::page(&jobs, args)?;
        Ok(page.reply("jobs", json!({})))
    }

    fn job_status(&mut self, id: &str) -> Result<Value> {
        self.poll_jobs()?;
        self.jobs
            .get(id)
            .map(serde_json::to_value)
            .transpose()?
            .ok_or_else(|| invalid("Q-ROC job not found"))
    }

    fn job_cancel(&mut self, id: &str) -> Result<Value> {
        let mut job = self
            .jobs
            .get(id)
            .cloned()
            .ok_or_else(|| invalid("Q-ROC job not found"))?;
        if job.state == "running" {
            if let Some(running) = self.running.get(id) {
                running.cancel.store(true, Ordering::Relaxed);
            }
            job.state = "cancelling".into();
            self.persist_job(&job)?;
            self.jobs.insert(id.to_owned(), job.clone());
        }
        if job.state == "queued" {
            job.state = "cancelled".into();
            job.finished_at = Some(now());
            self.persist_job(&job)?;
            self.jobs.insert(id.to_owned(), job.clone());
        }
        Ok(serde_json::to_value(job)?)
    }

    fn poll_jobs(&mut self) -> Result<()> {
        let ids = self.running.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            let result = match self
                .running
                .get(&id)
                .and_then(|j| j.receiver.try_iter().last())
            {
                Some(result) => result,
                None => continue,
            };
            let running = self.running.remove(&id).expect("running job exists");
            let mut job = self
                .jobs
                .get(&id)
                .cloned()
                .ok_or_else(|| invalid("Q-ROC job record disappeared"))?;
            if running.cancel.load(Ordering::Relaxed) {
                job.state = "cancelled".into();
                job.error = Some("Generation cancelled by operator".into());
            } else {
                match result {
                    Ok(generated) => {
                        if let Some(inputs) = job.pinned_inputs.as_mut() {
                            inputs["route"] =
                                json!(generated.route.unwrap_or_else(|| job.model.clone()));
                            inputs["response_model"] = json!(generated.model);
                        }
                        let output_sha256 = digest(generated.content.as_bytes());
                        job.output_sha256 = Some(output_sha256.clone());
                        job.generation_finish_reason = Some(generated.finish_reason.clone());
                        job.honoured_max_tokens = generated.honoured_max_tokens;
                        // A receipt is written after the immutable document. Persist the
                        // output identity first so a crash between those writes can be
                        // reconciled on the next open instead of producing another revision.
                        self.persist_job(&job)?;
                        self.jobs.insert(id.clone(), job.clone());
                        let current_revision = current_revision(
                            &self.root,
                            &job.project_id,
                            &job.branch_id,
                            &job.document_type,
                        )?;
                        if job
                            .base_revision
                            .is_some_and(|base| base != current_revision)
                            || job.pinned_inputs.as_ref().is_some_and(|v| {
                                self.scope_for(&job.project_id, &job.branch_id)
                                    .map(|scope| v["scope"] != json!(scope))
                                    .unwrap_or(true)
                            })
                        {
                            let conflict_path =
                                self.root.join("conflicts").join(format!("{}.json", job.id));
                            let conflict = json!({
                                "schema": "qroc-generation-conflict/1",
                                "job_id": job.id,
                                "project_id": job.project_id,
                                "branch_id": job.branch_id,
                                "document_type": job.document_type,
                                "base_revision": job.base_revision,
                                "current_revision": current_revision,
                                "sha256": output_sha256,
                                "generation_finish_reason": generated.finish_reason,
                                "content": generated.content,
                                "generation_inputs":job.pinned_inputs,
                                "created_at": now(),
                                "notice": "Generated content was preserved separately because the branch changed while generation was running. Review it before saving a new revision."
                            });
                            write_json_atomic(&conflict_path, &conflict)?;
                            job.state = "conflicted".into();
                            job.error = Some(format!(
                                "Branch advanced from revision {} to {}; generated output preserved as a conflict",
                                job.base_revision.unwrap_or_default(),
                                current_revision
                            ));
                            job.conflict_path = Some(conflict_path.to_string_lossy().into_owned());
                            job.finished_at = Some(now());
                            self.persist_job(&job)?;
                            self.jobs.insert(id, job);
                            continue;
                        }
                        let existing = find_document_by_sha(
                            &self.root,
                            &job.project_id,
                            &job.branch_id,
                            &job.document_type,
                            &output_sha256,
                        )?;
                        let existing = existing.filter(|d| {
                            d.generation_inputs
                                .as_ref()
                                .is_some_and(|v| v["job_id"] == job.id)
                        });
                        let doc = if let Some(existing) = existing {
                            existing
                        } else {
                            let revision = next_revision(
                                &self.root,
                                &job.project_id,
                                &job.branch_id,
                                &job.document_type,
                            )?;
                            let mut doc = self.make_document(
                                &job.project_id,
                                &job.branch_id,
                                &job.document_type,
                                revision,
                                generated.content,
                            )?;
                            doc.provenance = job.synthesis.clone();
                            doc.generation_finish_reason = job.generation_finish_reason.clone();
                            doc.generation_inputs = job.pinned_inputs.clone();
                            self.write_document(&doc)?;
                            doc
                        };
                        job.state = if job.generation_finish_reason.as_deref() == Some("stop") {
                            "succeeded"
                        } else {
                            "partial"
                        }
                        .into();
                        if job.state == "partial" {
                            job.error = Some(format!(
                                "Provider stopped with {}; partial draft retained. Review or regenerate before relying on it.",
                                job.generation_finish_reason
                                    .as_deref()
                                    .unwrap_or("unknown completion status")
                            ));
                        }
                        job.document_id = Some(doc.id);
                    }
                    Err(error) => {
                        job.state = "failed".into();
                        job.error = Some(error.to_string());
                    }
                }
            }
            job.finished_at = Some(now());
            self.persist_job(&job)?;
            self.jobs.insert(id, job);
        }
        let queued = self
            .jobs
            .values()
            .filter(|j| j.state == "queued")
            .map(|j| j.id.clone())
            .collect::<Vec<_>>();
        for id in queued {
            let mut job = self.jobs.get(&id).cloned().expect("queued job exists");
            if let Some(batch_id) = &job.batch_id {
                let dependencies = job
                    .dependency_jobs
                    .iter()
                    .map(|id| self.jobs.get(id).map(|j| j.state.as_str()))
                    .collect::<Vec<_>>();
                match batch_readiness(&dependencies) {
                    BatchReadiness::Waiting => continue,
                    BatchReadiness::Blocked => {
                        job.state = "blocked".into();
                        job.error = Some("Batch stopped: required document or gate did not complete successfully".into());
                    }
                    BatchReadiness::Ready => {
                        if self
                            .jobs
                            .values()
                            .filter(|j| {
                                j.batch_id.as_ref() == Some(batch_id)
                                    && matches!(j.state.as_str(), "running" | "cancelling")
                            })
                            .count()
                            >= job.batch_width
                            || self.running.len() >= MAX_JOBS
                        {
                            continue;
                        }
                        let mut gate_failed = false;
                        for id in &job.dependency_jobs {
                            if let Some(source) = self.jobs.get(id)
                                && source.document_type.starts_with("GATE_")
                            {
                                let doc = self.semantic_current_document(
                                    &source.project_id,
                                    &source.branch_id,
                                    &source.document_type,
                                )?;
                                gate_failed |= self.evaluate_owned_gate(&doc).status != "passed";
                            }
                        }
                        // The hardening gate guards every launch of a graph batch, not only its start.
                        let hardened =
                            crate::harden::Hardening::open(&self.root)?.passed(&job.project_id)?;
                        if !hardened {
                            job.state = "blocked".into();
                            job.error =
                                Some("Batch stopped: the hardening gate no longer passes".into());
                        } else if gate_failed {
                            job.state = "blocked".into();
                            job.error = Some("Batch stopped: required gate did not pass".into());
                        } else if let Err(error) = self.launch_job(&id) {
                            job.state = "blocked".into();
                            job.error = Some(error.to_string());
                        } else {
                            continue;
                        }
                    }
                }
                job.finished_at = Some(now());
                self.persist_job(&job)?;
                self.jobs.insert(id, job);
                continue;
            }
            let predecessor = job.after_job.as_ref().and_then(|id| self.jobs.get(id));
            if predecessor
                .is_some_and(|j| matches!(j.state.as_str(), "running" | "queued" | "cancelling"))
            {
                continue;
            }
            let mut can_continue = predecessor.is_some_and(|j| j.state == "succeeded");
            if let Some(previous) = predecessor
                && can_continue
                && previous.document_type.starts_with("GATE_")
            {
                let revision = current_revision(
                    &self.root,
                    &previous.project_id,
                    &previous.branch_id,
                    &previous.document_type,
                )?;
                let gate:DocumentRevision = read_json(&self.document_path(&json!({"project_id":previous.project_id,"branch_id":previous.branch_id,"document_type":previous.document_type,"revision":revision}))?)?;
                can_continue = self.evaluate_owned_gate(&gate).status == "passed";
            }
            let error = if can_continue {
                self.launch_job(&id).err().map(|e| e.to_string())
            } else {
                Some("Cascade stopped: the preceding document or gate did not complete successfully. Retained results are unchanged.".into())
            };
            if let Some(error) = error {
                job.state = "blocked".into();
                job.error = Some(error);
                job.finished_at = Some(now());
                self.persist_job(&job)?;
                self.jobs.insert(id, job);
            }
        }
        self.advance_gate_revisions()
    }

    fn gate_evaluate(&self, args: &Value) -> Result<Value> {
        let path = self.document_path(args)?;
        let doc: DocumentRevision = read_json(&path)?;
        Ok(serde_json::to_value(self.evaluate_owned_gate(&doc))?)
    }

    // Retained owner semantics: browsing/drafting is unrestricted; completion
    // requires a recorded review of each current gate in the same branch.
    fn reviewed_gate_document(&self, args: &Value) -> Result<DocumentRevision> {
        let (project, branch, kind) = self.draft_key(args)?;
        if args["revision"].as_u64().is_none() {
            return Err(invalid("Exact gate revision is required"));
        }
        if !kind.starts_with("GATE_") {
            return Err(invalid("Only a gate document can receive a gate review"));
        }
        let document: DocumentRevision = read_json(&self.document_path(args)?)?;
        if document.project_id != project
            || document.branch_id != branch
            || document.revision != current_revision(&self.root, &project, &branch, &kind)?
        {
            return Err(invalid(
                "This gate changed; reopen the current revision before reviewing or advancing",
            ));
        }
        Ok(document)
    }

    fn gate_review(&mut self, args: &Value) -> Result<Value> {
        let document = self.reviewed_gate_document(args)?;
        if s(args, "sha256") != document.sha256 {
            return Err(invalid(
                "Gate content changed; reopen it before recording a review",
            ));
        }
        let decision = s(args, "decision");
        if decision != "passed" && decision != "failed" {
            return Err(invalid("Choose an explicit passed or failed review"));
        }
        let evaluation = self.evaluate_owned_gate(&document);
        if decision == "passed" && evaluation.status != "passed" {
            return Err(invalid(
                "This gate does not meet its criteria and score threshold",
            ));
        }
        let mut project = self
            .projects
            .get(&document.project_id)
            .cloned()
            .ok_or_else(|| invalid("Q-ROC project not found"))?;
        if let Some(review) = project
            .gate_reviews
            .iter()
            .rev()
            .find(|r| r.document_id == document.id)
            && review.decision == decision
            && review.sha256 == document.sha256
        {
            return Ok(json!({"review":review,"evaluation":evaluation,"project":project}));
        }
        let review = RecordedGateReview {
            document_id: document.id,
            branch_id: document.branch_id,
            document_type: document.document_type,
            revision: document.revision,
            sha256: document.sha256,
            decision: decision.into(),
            reviewed_at: now(),
            semantic_inputs: evaluation.semantic.clone(),
        };
        project.gate_reviews.push(review.clone());
        project.updated_at = now();
        // One atomic project publication contains the durable review history.
        write_json_atomic(
            &self
                .root
                .join("projects")
                .join(format!("{}.json", project.id)),
            &project,
        )?;
        self.projects.insert(project.id.clone(), project.clone());
        Ok(json!({"review":review,"evaluation":evaluation,"project":project}))
    }

    fn domain_advance(&mut self, args: &Value) -> Result<Value> {
        let final_gate = self.reviewed_gate_document(args)?;
        let mut project = self
            .projects
            .get(&final_gate.project_id)
            .cloned()
            .ok_or_else(|| invalid("Q-ROC project not found"))?;
        if s(args, "expected_domain") != project.current_domain {
            return Err(invalid("Project domain changed; reopen before advancing"));
        }
        let index = DOMAINS
            .iter()
            .position(|d| *d == crate::registry::canonical_domain(&project.current_domain))
            .ok_or_else(|| invalid("Unknown current domain"))?;
        let target = DOMAINS
            .get(index + 1)
            .ok_or_else(|| invalid("No later domain exists"))?;
        let requested_target = s(args, "target_domain");
        if crate::registry::canonical_domain(requested_target) != *target {
            return Err(invalid(
                "Advance only to the next domain after reviewing its predecessor gates",
            ));
        }
        // Legacy callers may keep the legacy identity; the overview is canonical.
        let target = &requested_target;
        if index == 0
            && !self
                .production_overview(&project)?
                .iter()
                .any(|d| d["id"] == "PRODUCT" && d["status"] == "produced")
        {
            return Err(invalid(
                "Complete the Product pipeline before triggering a later domain",
            ));
        }
        let required = crate::registry::domain_gates(&project.current_domain);
        if required.is_empty() {
            return Err(invalid("No later domain exists"));
        }
        if Some(&final_gate.document_type.as_str()) != required.last() {
            return Err(invalid(
                "Review the current domain's final gate before advancing",
            ));
        }
        let mut gates = Vec::new();
        for kind in required {
            let revision = current_revision(&self.root, &project.id, &final_gate.branch_id, kind)?;
            if revision == 0 {
                return Err(invalid(format!(
                    "{kind} needs a recorded passing review in the selected branch"
                )));
            }
            let gate: DocumentRevision = read_json(&self.document_path(&json!({"project_id":project.id,"branch_id":final_gate.branch_id,"document_type":kind,"revision":revision}))?)?;
            let review = project
                .gate_reviews
                .iter()
                .rev()
                .find(|r| r.document_id == gate.id)
                .ok_or_else(|| {
                    invalid(format!(
                        "{kind} needs a recorded passing review in the selected branch"
                    ))
                })?;
            if review.decision != "passed"
                || review.revision != gate.revision
                || review.sha256 != gate.sha256
                || self.evaluate_owned_gate(&gate).status != "passed"
                || review.semantic_inputs != self.evaluate_owned_gate(&gate).semantic
            {
                return Err(invalid(format!(
                    "{kind} review is stale, failed or below its deterministic threshold"
                )));
            }
            gates.push(review.clone());
        }
        project.domain_transitions.push(DomainTransition {
            from: project.current_domain.clone(),
            to: (*target).into(),
            branch_id: final_gate.branch_id,
            gates,
            recorded_at: now(),
        });
        project.current_domain = (*target).into();
        project.updated_at = now();
        // State and exact evidence pins commit together, never in separate files.
        write_json_atomic(
            &self
                .root
                .join("projects")
                .join(format!("{}.json", project.id)),
            &project,
        )?;
        self.projects.insert(project.id.clone(), project.clone());
        Ok(serde_json::to_value(project)?)
    }

    fn analysis(&self, args: &Value) -> Result<Value> {
        let project_id = s(args, "project_id");
        let document_type = normalized_document(s(args, "document_type"))?;
        let mut rows = Vec::new();
        for branch in self
            .branches
            .values()
            .filter(|b| b.project_id == project_id)
        {
            let docs = document_paths(&self.root, project_id, &branch.id)?;
            let latest = docs
                .into_iter()
                .filter_map(|p| read_json::<DocumentRevision>(&p).ok())
                .filter(|d| d.document_type == document_type)
                .max_by_key(|d| d.revision);
            rows.push(json!({"branch":branch,"document":latest,"evidence":"Local document revision only; no automatic quality ranking."}));
        }
        Ok(json!({"project_id":project_id,"document_type":document_type,"items":rows}))
    }

    fn batch_start(&mut self, args: &Value) -> Result<Value> {
        if !crate::harden::Hardening::open(&self.root)?.passed(s(args, "project_id"))? {
            return Err(invalid(
                "Hardening gate has not passed; answer the hardening questions before writing documents",
            ));
        }
        let types = args["document_types"]
            .as_array()
            .ok_or_else(|| invalid("document_types must be an array"))?;
        if types.is_empty() || types.len() > 32 {
            return Err(invalid("Choose 1–32 document types"));
        }
        if crate::registry::provider_name_at(&self.root)? != "local-gateway"
            && std::env::var("QROC_BATCH_MAX")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .is_none_or(|n| n < types.len())
        {
            return Err(invalid(
                "capability-unavailable: QROC_BATCH_MAX must explicitly cover the requested cascade",
            ));
        }
        let project = self
            .projects
            .get(s(args, "project_id"))
            .ok_or_else(|| invalid("Q-ROC project not found"))?;
        let branch = self
            .branches
            .get(s(args, "branch_id"))
            .filter(|b| b.project_id == project.id)
            .ok_or_else(|| invalid("Q-ROC branch not found"))?;
        if self.jobs.values().any(|j| {
            j.branch_id == branch.id
                && matches!(j.state.as_str(), "running" | "queued" | "cancelling")
        }) {
            return Err(invalid(
                "Inspect or cancel the existing generation in this branch before starting a cascade",
            ));
        }
        let mut last_order = None;
        for kind in types {
            let kind = normalized_document(kind.as_str().unwrap_or(""))?;
            let order = document_order(&kind);
            if last_order.is_some_and(|previous| order <= previous) {
                return Err(invalid(
                    "Cascade documents must be unique and in their canonical order",
                ));
            }
            last_order = Some(order);
            self.generation_context(project, branch, &kind, s(args, "context"))?;
        }
        let default_mode = if types
            .iter()
            .all(|k| crate::registry::product_inputs(k.as_str().unwrap_or("")).is_some())
        {
            "graph"
        } else {
            "chain"
        };
        let mode = args
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or(default_mode);
        if !matches!(mode, "graph" | "chain") {
            return Err(invalid("Batch mode must be graph or chain"));
        }
        let width = args
            .get("width")
            .map(|v| {
                v.as_u64()
                    .ok_or_else(|| invalid("Batch width must be an integer"))
            })
            .transpose()?
            .unwrap_or(3) as usize;
        if !(1..=3).contains(&width) {
            return Err(invalid("Batch width must be 1–3"));
        }
        if mode == "graph"
            && types
                .iter()
                .any(|k| crate::registry::product_inputs(k.as_str().unwrap_or("")).is_none())
        {
            return Err(invalid(
                "Graph batch currently supports Product documents only",
            ));
        }
        // Required inputs outside this batch must already exist in this branch.
        for kind in types {
            if let Some(inputs) = crate::registry::product_inputs(kind.as_str().unwrap_or("")) {
                for input in inputs
                    .iter()
                    .copied()
                    .chain(crate::registry::product_barrier(
                        kind.as_str().unwrap_or(""),
                    ))
                {
                    if input != "IDEA" && !types.iter().any(|v| v.as_str() == Some(input)) {
                        let source =
                            self.semantic_current_document(&project.id, &branch.id, input)?;
                        if source
                            .generation_finish_reason
                            .as_deref()
                            .is_some_and(|r| r != "stop")
                        {
                            return Err(invalid("Required batch source is partial"));
                        }
                        if input.starts_with("GATE_")
                            && self.evaluate_owned_gate(&source).status != "passed"
                        {
                            return Err(invalid("Required batch gate has not passed"));
                        }
                    }
                }
            }
        }
        let batch_id = self.new_id("batch");
        let mut jobs = Vec::new();
        let mut ids = BTreeMap::new();
        for kind in types {
            let mut payload = args.clone();
            payload["document_type"] = kind.clone();
            let job = self.generate_start_after(&payload, Some(batch_id.clone()))?;
            ids.insert(
                kind.as_str().unwrap_or("").to_string(),
                s(&job, "id").to_string(),
            );
            jobs.push(job);
        }
        let mut previous = None;
        for value in &jobs {
            let id = s(value, "id");
            let mut job = self
                .jobs
                .get(id)
                .cloned()
                .ok_or_else(|| invalid("Batch job disappeared"))?;
            job.batch_id = Some(batch_id.clone());
            job.batch_width = if mode == "chain" { 1 } else { width };
            job.after_job = None;
            if mode == "chain" {
                job.dependency_jobs = previous.clone().into_iter().collect();
            } else if let Some(inputs) = crate::registry::product_inputs(&job.document_type) {
                for kind in inputs
                    .iter()
                    .copied()
                    .chain(crate::registry::product_barrier(&job.document_type))
                {
                    if let Some(id) = ids.get(kind)
                        && !job.dependency_jobs.contains(id)
                    {
                        job.dependency_jobs.push(id.clone());
                    }
                }
            }
            previous = Some(id.to_string());
            self.persist_job(&job)?;
            self.jobs.insert(id.to_string(), job);
        }
        self.poll_jobs()?;
        let jobs = jobs
            .iter()
            .filter_map(|v| self.jobs.get(s(v, "id")))
            .collect::<Vec<_>>();
        Ok(
            json!({"state":"started","batch_id":batch_id,"jobs":jobs,"count":jobs.len(),"execution":mode,"width":if mode == "chain" {1} else {width}}),
        )
    }

    /// What the Product tail reads, checked: a passed hardening loop, a passed Gate 3 (the
    /// tail's barrier) and complete current architecture and slice index on this branch.
    pub fn tail_input(
        &self,
        project_id: &str,
        branch_id: &str,
    ) -> Result<crate::contracts::TailInput> {
        let project = self
            .projects
            .get(project_id)
            .ok_or_else(|| invalid("Q-ROC project not found"))?;
        let branch = self
            .branches
            .get(branch_id)
            .filter(|b| b.project_id == project.id)
            .ok_or_else(|| invalid("Q-ROC branch not found"))?;
        let hardening = crate::harden::Hardening::open(&self.root)?;
        if !hardening.passed(&project.id)? {
            return Err(invalid(
                "Hardening gate has not passed; answer the hardening questions before writing documents",
            ));
        }
        let idea = hardening
            .load(&project.id)?
            .rounds
            .last()
            .map(|r| r.idea_text.clone())
            .ok_or_else(|| invalid("Hardening not started"))?;
        let gate = self
            .semantic_current_document(&project.id, &branch.id, "GATE_3")
            .map_err(|_| invalid("Gate 3 has no document yet; the tail waits for it"))?;
        if self.evaluate_owned_gate(&gate).status != "passed" {
            return Err(invalid("Gate 3 has not passed; the tail waits for it"));
        }
        let current = |kind: &str| -> Result<String> {
            let doc = self.semantic_current_document(&project.id, &branch.id, kind)?;
            if doc
                .generation_finish_reason
                .as_deref()
                .is_some_and(|r| r != "stop")
            {
                return Err(invalid("A tail input is partial"));
            }
            Ok(doc.content)
        };
        let architecture = current("TECHNICAL_ARCHITECTURE")?;
        let slice_index = current("SLICE_INDEX")?;
        Ok(crate::contracts::TailInput {
            project_id: project.id.clone(),
            branch_id: branch.id.clone(),
            idea_sha256: digest(idea.as_bytes()),
            idea,
            architecture_sha256: digest(architecture.as_bytes()),
            architecture,
            slice_index_sha256: digest(slice_index.as_bytes()),
            slice_index,
        })
    }

    /// Library and bridge share this export; discovery never starts generation.
    pub fn graph_export(&self, args: &Value) -> Result<Value> {
        let project = self
            .projects
            .get(s(args, "project_id"))
            .ok_or_else(|| invalid("Q-ROC project not found"))?;
        let branch = self
            .branches
            .get(s(args, "branch_id"))
            .filter(|b| b.project_id == project.id)
            .ok_or_else(|| invalid("Q-ROC branch not found"))?;
        let mut nodes = Vec::new();
        nodes.push(json!({"id":"IDEA","inputs":[],"column":0,"state":"provided","kind":"idea","standing":"Hardened standing belongs to T401"}));
        let tail = crate::contracts::tail_state(&self.root, &project.id, &branch.id)?;
        for kind in PRODUCT_DOCUMENTS
            .iter()
            .copied()
            .chain(crate::contracts::TAIL_NODES)
        {
            if let Some(step) = crate::contracts::tail_step(kind) {
                nodes.push(json!({"id":kind,"inputs":crate::registry::product_inputs(kind),"column":crate::registry::product_column(kind),"barrier":crate::registry::product_barrier(kind),"state":tail["nodes"][kind],"kind":"document","job_id":null,"gate":null,"fan_out":matches!(kind,"COMPONENT_CONTRACTS"|"PHOD_SPEC"|"VERIFICATION"),"inputs_binding":if kind == "PHOD_SPEC" {"its contract"} else if kind == "VERIFICATION" {"its spec"} else if kind == "CONSISTENCY_JOIN" {"every spec and verification"} else {"listed documents"},"step":step,"revision":tail["revision"],"instances":if kind == "PHOD_SPEC" || kind == "VERIFICATION" {tail["instances"].clone()} else {json!([])}}));
                continue;
            }
            let job = self
                .jobs
                .values()
                .filter(|j| {
                    j.project_id == project.id
                        && j.branch_id == branch.id
                        && j.document_type == kind
                })
                .max_by_key(|j| (&j.started_at, &j.id));
            let document = self
                .semantic_current_document(&project.id, &branch.id, kind)
                .ok();
            let state = if let Some(job) = job {
                job.state.as_str()
            } else if document.is_some() {
                "produced"
            } else {
                "waiting"
            };
            nodes.push(json!({"id":kind,"inputs":crate::registry::product_inputs(kind),"column":crate::registry::product_column(kind),"barrier":crate::registry::product_barrier(kind),"state":state,"kind":if kind.starts_with("GATE_") {"gate"} else {"document"},"job_id":job.map(|j| &j.id),"gate":document.as_ref().filter(|_|kind.starts_with("GATE_")).map(|d|self.evaluate_owned_gate(d)),"fan_out":matches!(kind,"COMPONENT_CONTRACTS"|"PHOD_SPEC"|"VERIFICATION"),"inputs_binding":if kind == "PHOD_SPEC" {"its contract"} else if kind == "VERIFICATION" {"its spec"} else if kind == "CONSISTENCY_JOIN" {"every spec and verification"} else {"listed documents"},"revise":self.graph_revise(&project.id,&branch.id,kind)?}));
        }
        Ok(
            json!({"schema":"qroc-product-graph/1","project_id":project.id,"branch_id":branch.id,"domain":"PRODUCT","nodes":nodes,"default_width":3,"tail_owner":"contracts","tail_error":tail["error"],"execution_authority":false}),
        )
    }

    fn export(&self, args: &Value) -> Result<Value> {
        let project_id = s(args, "project_id");
        let project = self
            .projects
            .get(project_id)
            .ok_or_else(|| invalid("Q-ROC project not found"))?;
        for source in &project.sources {
            let bytes = fs::read(&source.path)?;
            if bytes.len() as u64 != source.size || digest(&bytes) != source.sha256 {
                return Err(invalid("Export source digest/size mismatch"));
            }
        }
        let export_id = if s(args, "export_id").is_empty() {
            format!(
                "{}-{}-{}",
                project_id,
                now(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| invalid("Invalid clock"))?
                    .as_nanos()
            )
        } else {
            safe_component(s(args, "export_id"), "export_id")?
        };
        let path = self.root.join("exports").join(format!("{export_id}.json"));
        let mut latest = BTreeMap::new();
        let mut omitted = Vec::new();
        for branch in self
            .branches
            .values()
            .filter(|b| b.project_id == project_id)
        {
            for file in document_paths(&self.root, project_id, &branch.id)? {
                let doc: DocumentRevision = read_json(&file)?;
                if doc.project_id != project_id
                    || doc.branch_id != branch.id
                    || digest(doc.content.as_bytes()) != doc.sha256
                {
                    return Err(invalid("Export document identity/digest mismatch"));
                }
                let key = (branch.id.clone(), doc.document_type.clone());
                if latest
                    .get(&key)
                    .is_none_or(|v: &DocumentRevision| v.revision < doc.revision)
                {
                    if let Some(old) = latest.insert(key, doc) {
                        omitted.push(old.id)
                    }
                } else {
                    omitted.push(doc.id)
                }
            }
        }
        if let Some(expected) = args.get("expected_documents") {
            let refs = expected
                .as_array()
                .ok_or_else(|| invalid("expected_documents must be an array"))?;
            if refs.len() != latest.len() {
                return Err(invalid("Export selection changed"));
            }
            for r in refs {
                if !latest.values().any(|d| {
                    r["id"] == d.id && r["revision"] == d.revision && r["sha256"] == d.sha256
                }) {
                    return Err(invalid("Export revision changed"));
                }
            }
        }
        let documents = latest.values().collect::<Vec<_>>();
        let standing=documents.iter().map(|d|json!({"id":d.id,"revision":d.revision,"sha256":d.sha256,"generation_finish_reason":d.generation_finish_reason,"gate_evaluation":self.evaluate_owned_gate(d),"review":project.gate_reviews.iter().rev().find(|r|r.document_id==d.id&&r.sha256==d.sha256),"execution_authority":false})).collect::<Vec<_>>();
        let proposal = args.get("proposal").cloned().unwrap_or(Value::Null);
        if !proposal.is_null()
            && (proposal["receiving_owner"]
                .as_str()
                .is_none_or(|s| s.trim().is_empty())
                || args.get("expected_documents").is_none())
        {
            return Err(invalid(
                "Proposal requires receiving_owner and exact expected_documents",
            ));
        }
        let payload = json!({"format":"qroc-native/1","manifest_schema":"qroc-current-export/1","project":project,"branches":self.branches.values().filter(|b| b.project_id == project_id).collect::<Vec<_>>(),"documents":documents,"standing":standing,"omitted_historical_document_ids":omitted,"proposal":proposal,"executed":false,"execution_authority":false,"notice":"Current authoring revisions only. Receiving owner must separately inspect collisions and authorize execution."});
        write_json_noreplace(&path, &payload)?;
        Ok(
            json!({"path":path,"format":"json","export_id":export_id,"sha256":digest(&fs::read(&path)?),"executed":false}),
        )
    }

    /// Materialize the revision store and return a location for a file-manager caller.
    /// The graphical caller must consume this location without displaying it.
    pub fn open_project_folder(&self, project_id: &str) -> Result<PathBuf> {
        let project = self
            .projects
            .get(project_id)
            .ok_or_else(|| invalid("Q-ROC project not found"))?;
        let branches = self
            .branches
            .values()
            .filter(|b| b.project_id == project_id)
            .collect::<Vec<_>>();
        let mut records = Vec::new();
        for branch in &branches {
            for path in document_paths(&self.root, project_id, &branch.id)? {
                crate::scope::validate_root(&path)?;
                let doc: DocumentRevision = read_json(&path)?;
                if doc.project_id != project_id
                    || doc.branch_id != branch.id
                    || digest(doc.content.as_bytes()) != doc.sha256
                {
                    return Err(invalid("Project folder revision owner or digest mismatch"));
                }
                records.push((branch.name.clone(), doc));
            }
        }
        let mut extra = Vec::new();
        let hardening = crate::harden::Hardening::open(&self.root)?.load(project_id)?;
        if !hardening.rounds.is_empty() {
            extra.push((
                "idea/Hardening.md".to_string(),
                crate::harden::readable(&hardening),
            ));
        }
        for branch in &branches {
            let suffix = if branches.len() > 1 {
                format!(" ({})", branch.name)
            } else {
                String::new()
            };
            extra.extend(crate::contracts::readable(
                &self.root, project_id, &branch.id, &suffix,
            )?);
        }
        if project.kind == "media" {
            extra.extend(crate::media::readable(&self.root, project_id)?);
        }
        crate::project_folders::materialize(&self.root, project, &records, &extra)
    }

    /// A project as stored, for the media library that keeps its own records beside it.
    pub fn project(&self, project_id: &str) -> Result<&Project> {
        self.projects
            .get(project_id)
            .ok_or_else(|| invalid("Q-ROC project not found"))
    }

    fn document_path(&self, args: &Value) -> Result<PathBuf> {
        let project_id = safe_component(s(args, "project_id"), "project_id")?;
        let branch_id = safe_component(s(args, "branch_id"), "branch_id")?;
        let document_type = normalized_document(s(args, "document_type"))?;
        let revision = args["revision"]
            .as_u64()
            .ok_or_else(|| invalid("revision is required"))?;
        let path = self
            .root
            .join("documents")
            .join(project_id)
            .join(branch_id)
            .join(format!("{document_type}-r{revision}.json"));
        if !path.starts_with(self.root.join("documents")) {
            return Err(invalid("Document path escaped Q-ROC root"));
        }
        Ok(path)
    }

    fn make_document(
        &mut self,
        project_id: &str,
        branch_id: &str,
        document_type: &str,
        revision: u32,
        content: String,
    ) -> Result<DocumentRevision> {
        let id = self.new_id("document");
        let sha256 = digest(content.as_bytes());
        Ok(DocumentRevision {
            id,
            project_id: project_id.into(),
            branch_id: branch_id.into(),
            document_type: document_type.into(),
            revision,
            rendered: content.clone(),
            content,
            source_revision: None,
            created_at: now(),
            sha256,
            provenance: None,
            generation_finish_reason: None,
            generation_inputs: None,
        })
    }

    fn write_document(&self, doc: &DocumentRevision) -> Result<()> {
        if !crate::harden::Hardening::open(&self.root)?.passed(&doc.project_id)? {
            return Err(invalid(
                "Hardening gate has not passed before document persistence",
            ));
        }
        self.persist_document(doc)
    }

    /// Persistence without the hardening check: only for material imported from an export,
    /// which already exists. Every authoring path goes through `write_document`.
    fn persist_document(&self, doc: &DocumentRevision) -> Result<()> {
        let dir = self
            .root
            .join("documents")
            .join(&doc.project_id)
            .join(&doc.branch_id);
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}-r{}.json", doc.document_type, doc.revision));
        if path.exists() {
            let existing: DocumentRevision = read_json(&path)?;
            if existing.sha256 == doc.sha256 && existing.content == doc.content {
                self.open_project_folder(&doc.project_id)?;
                return Ok(());
            }
            return Err(invalid(
                "Document revision already exists with different content",
            ));
        }
        let result = match write_json_noreplace(&path, doc) {
            Ok(()) => Ok(()),
            Err(error) if path.exists() => {
                let existing: DocumentRevision = read_json(&path)?;
                if existing.sha256 == doc.sha256 && existing.content == doc.content {
                    Ok(())
                } else {
                    Err(error)
                }
            }
            Err(error) => Err(error),
        };
        result?;
        self.open_project_folder(&doc.project_id)?;
        Ok(())
    }

    fn persist_job(&self, job: &JobRecord) -> Result<()> {
        write_json_atomic(
            &self.root.join("jobs").join(format!("{}.json", job.id)),
            job,
        )
    }
}

fn append_context(context: &mut String, omitted: &mut Vec<String>, label: String, body: String) {
    let section = format!("\n\n{label}\n{body}");
    if context.len().saturating_add(section.len()) <= MAX_GENERATION_CONTEXT_BYTES {
        context.push_str(&section);
    } else {
        omitted.push(label);
    }
}

/// Where and how far one generation request goes.
struct GatewayCall<'a> {
    root: &'a Path,
    endpoint: &'a str,
    timeout_seconds: u64,
    max_tokens: u32,
}

fn generate_from_gateway(
    project: &Project,
    branch: &Branch,
    document_type: &str,
    context: &str,
    cancel: &AtomicBool,
    call: GatewayCall<'_>,
) -> Result<GenerationResult> {
    let GatewayCall {
        root,
        endpoint,
        timeout_seconds,
        max_tokens,
    } = call;
    if cancel.load(Ordering::Relaxed) {
        return Err(invalid("Generation cancelled"));
    }
    let prompt = qroc_prompt(project, branch, document_type, context);
    let (value, model) = crate::routes::complete_at(
        root,
        endpoint,
        &json!({"model":branch.model,"messages":[{"role":"system","content":BASE_PERSONA},{"role":"user","content":prompt}],"stream":false,"max_tokens":max_tokens}),
        timeout_seconds,
        cancel,
    )?;
    let content = value["choices"][0]["message"]["content"]
        .as_str()
        .or_else(|| value["choices"][0]["text"].as_str())
        .ok_or_else(|| {
            invalid(format!(
                "Local gateway returned no document content: {}",
                value["error"].as_str().unwrap_or("unknown error")
            ))
        })?;
    let content = bounded_text(content, MAX_DOCUMENT_BYTES, "Gateway document")?;
    if content.trim().is_empty() {
        return Err(invalid("Local gateway returned an empty document"));
    }
    let finish_reason = value["choices"][0]["finish_reason"]
        .as_str()
        .unwrap_or("unknown")
        .to_owned();
    Ok(GenerationResult {
        content,
        finish_reason,
        model,
        route: value["route"].as_str().map(str::to_owned),
        honoured_max_tokens: value["honoured_max_tokens"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0),
    })
}

pub(crate) fn post_chat_completion(
    endpoint: &str,
    body: &serde_json::Value,
    timeout_seconds: u64,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<(serde_json::Value, String)> {
    if cancel.load(Ordering::Relaxed) {
        return Err(invalid("Generation cancelled"));
    }
    let (host, port) = parse_local_endpoint(endpoint)?;
    let body = serde_json::to_vec(body)?;
    let address = (host.as_str(), port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| invalid("Invalid local gateway address"))?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
    stream.set_read_timeout(Some(Duration::from_millis(250)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let request = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(request.as_bytes())?;
    stream.write_all(&body)?;
    let mut response = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_seconds);
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = stream.shutdown(Shutdown::Both);
            return Err(invalid("Generation cancelled"));
        }
        if std::time::Instant::now() >= deadline {
            let _ = stream.shutdown(Shutdown::Both);
            return Err(invalid("Local gateway generation timed out"));
        }
        let mut chunk = [0u8; 8192];
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                response.extend_from_slice(&chunk[..n]);
                if response.len() > MAX_DOCUMENT_BYTES {
                    return Err(invalid(
                        "Gateway response exceeded the Q-ROC document limit",
                    ));
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(error) => return Err(error.into()),
        }
    }
    let body_start = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .ok_or_else(|| invalid("Local gateway returned malformed HTTP"))?;
    let headers = std::str::from_utf8(&response[..body_start])
        .map_err(|_| invalid("Malformed gateway HTTP headers"))?;
    let mut lines = headers.split("\r\n");
    let status = lines
        .next()
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>();
    if status.len() < 2 || !matches!(status[0], "HTTP/1.0" | "HTTP/1.1") || status[1] != "200" {
        return Err(invalid("Gateway returned unsuccessful HTTP status"));
    }
    let mut content_length = None;
    for line in lines.filter(|s| !s.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| invalid("Malformed gateway HTTP header"))?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(invalid("Unsupported gateway transfer encoding"));
        }
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(invalid("Duplicate gateway content length"));
            }
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| invalid("Invalid gateway content length"))?,
            );
        }
    }
    if content_length.is_some_and(|n| n != response.len() - body_start) {
        return Err(invalid("Truncated or malformed gateway response length"));
    }
    let value: Value = serde_json::from_slice(&response[body_start..])?;
    let model = value["model"].as_str().unwrap_or("").to_owned();
    Ok((value, model))
}

fn qroc_prompt(project: &Project, branch: &Branch, document_type: &str, context: &str) -> String {
    let domain = domain_for(document_type);
    let native_template = document_template(document_type);
    let rubric_prompt = (document_type != "GATE_4")
        .then(|| crate::gate_rubric::rubric(document_type).ok().flatten())
        .flatten()
        .and_then(|rubric| crate::gate_rubric::prompt(document_type, &rubric));
    let closing = if rubric_prompt.is_some() {
        "Write the complete gate review and score every criterion from the supplied documents. Do not claim approval, evidence, or implementation results that are not in the supplied project material. Do not forward-reference documents that have not been supplied.\n\nQuestions for the project's owner: under Blocking Gaps, write one line for each fact or decision that the supplied material cannot settle and that only the person who owns this project can supply (for example which device he uses, who is accountable, or a choice between two of his own statements), in exactly this form: `[QUESTION] <criterion name> | <DOCUMENT_TYPE it came from> | <the question in plain words>`. Write `[QUESTION] none` when there is none. Never answer such a question yourself and never treat a proposal in the documents as his answer. Under Required Remediation, write one line for each content repair a writer can make without his answer, in exactly this form: `[REPAIR] <DOCUMENT_TYPE> | <criterion name> | <the repair>`. These lines do not change any score."
    } else {
        "Write a complete draft for this document. Do not claim a gate score, approval, evidence, or implementation result unless it is present in the supplied project material. Do not forward-reference documents that have not been supplied."
    };
    let planning_resolution = if rubric_prompt.is_none()
        && domain == "PRODUCT"
        && !document_type.starts_with("GATE_")
    {
        "\nPlanning resolution: Resolve every policy question and foundational constraint raised in the supplied idea, answers, dependencies or gate feedback. Use a decision register: item ID; exact decision or assumption; standing (source-stated, derived, or proposed); source locator; rationale; deterministic example and expected outcome; proposed accountable role; dependency and validation checkpoint. Where the source is silent, select a concrete source-compatible proposed assumption instead of leaving an unbounded menu of alternatives. Propagate the same register to downstream requirements, risks, architecture and validation cases. Proposed does not mean operator-approved, observed, implemented or feasible. Do not replace a supplied constraint with an easier one. If a constraint cannot be met without changing a source obligation or obtaining an operator-only fact, retain the incompatibility and write the exact question required. Distinguish a proposed accountable role from an actual assignment. For current workflow, separate the stated capture need from any proposed baseline workflow hypothesis and an exact prospective observation plan; never invent measurements, a phone identity, a person's assignment or completed tests. Planning can define decisions and future checks while actual-device feasibility and outcome evidence stay unproven. Reconcile newest completed dependency references; independent peers remain type references until the completed gate join provides their exact identities. Content reconciliation: Use one acceptance-baseline section listing all required peer document types, the exact supplied completed identities, and the completed join as the authority for peers still being regenerated; never present a historical peer manifest as current or invent a future identity. Enumerate the current PRD acceptance-criterion IDs and count them from the supplied PRD; explicitly supersede historical counts and map every current criterion exactly once, noting additions, removals and refinements. Propagate each supplied architecture-specific control into the corresponding requirement, risk and validation case with deterministic expected outcomes and traceable IDs; explicitly identify any architecture addition not yet supported by an upstream requirement. Keep unanswered operator questions, source incompatibilities, device identity, actual role assignments and observed workflow facts unresolved; content reconciliation cannot answer or assume them, nor promote a proposal to source authority.\n"
    } else {
        ""
    };
    let template = rubric_prompt.unwrap_or_else(|| {
        crate::registry::semantic_prompt(document_type)
            .ok()
            .flatten()
            .unwrap_or_else(|| native_template.into())
    });
    format!(
        "Q-ROC prompt provenance: promptBuilder.ts sha256 {PROMPT_BUILDER_SHA256}; promptCompiler.ts sha256 {PROMPT_COMPILER_SHA256}.\nProject: {}\nDomain: {domain}\nDocument: {document_type}\nBranch: {} ({})\n\nDocument-specific contract:\n{template}\n\nProject brain dump:\n{}\n\nPinned context (selected knowledge, verified attachments, and earlier cascade documents; omissions are explicit):\n{}\n\n{closing}{planning_resolution}",
        project.title, branch.name, branch.model, project.brain_dump, context
    )
}

fn document_template(document_type: &str) -> &'static str {
    match document_type {
        "STRATEGY_BRIEF" => {
            "Answer why this project exists. Required sections: Executive Summary; Problem Statement with evidence; Solution Vision; Target Users and Personas; SMART Success Metrics; Anti-Goals; Risks and Mitigations; Competitive Positioning."
        }
        "PRD" => {
            "Define what the product must do. Required sections: Overview; Goals and Objectives; User Stories; numbered Functional Requirements with acceptance criteria; Non-Functional Requirements; Out of Scope; Open Questions; Dependencies. Use MUST/SHOULD/MAY and priorities P0/P1/P2."
        }
        "TECHNICAL_ARCHITECTURE" => {
            "Define how the system works. Required sections: Context and Constraints; Component Architecture; Data Model; API Contracts; Security and Privacy; Reliability and Performance; Deployment; Observability; Alternatives and Rationale. Include Mermaid diagrams when useful."
        }
        "VALIDATION_PLAN" => {
            "Define how claims will be tested. Required sections: Hypotheses; Test Matrix; Participants or Fixtures; Procedure; Metrics and Thresholds; Risks and Biases; Decision Rules; Reproducible Evidence Capture."
        }
        "IMPLEMENTATION_PLAN" => {
            "Turn the approved cascade into executable work. Required sections: Scope; Milestones; Work Breakdown; Dependencies; Owners; Acceptance Criteria; Rollout and Rollback; Risks; Open Questions. Do not claim work is complete."
        }
        "ROADMAP" => {
            "Sequence work toward measurable outcomes. Required sections: Themes; Now/Next/Later horizons; Dependencies; Milestones; Exit Criteria; Capacity Assumptions; Risks; Explicit Non-Goals."
        }
        "GATE_1" | "GATE_2" | "GATE_3" | "GATE_4" | "GATE_5" | "GATE_6" | "GATE_7" | "GATE_8"
        | "GATE_9" | "GATE_10" => {
            "Produce a critical gate review. Required sections: Decision; Evidence Reviewed; Criteria Table with explicit score/max and feedback; Blocking Gaps; Risks; Required Remediation; Next Review Conditions. Never invent [SCORE] or approval."
        }
        "BUSINESS_PLAN" | "MARKET_ANALYSIS" | "COMPETITOR_ANALYSIS" => {
            "Ground business claims in supplied evidence. Required sections: Market or Customer Problem; Segment; Alternatives; Evidence and Confidence; Business Model; Risks; Assumptions; Validation Actions. Label hypotheses clearly."
        }
        "MESSAGING_FRAMEWORK" | "OUTBOUND_STRATEGY" | "INBOUND_STRATEGY" => {
            "Define audience-specific communication. Required sections: Audience; Positioning; Message Pillars; Proof Points; Objections; Channels; Experiments; Measurement; Compliance Notes. Do not manufacture testimonials or results."
        }
        "COMPLIANCE_PRIVACY" | "PR_POLICY" | "LICENSING" | "TERMS" | "CONTRACTS" => {
            "Draft a reviewable legal/compliance artifact. Required sections: Scope; Applicable Assumptions; Controls or Clauses; Data Handling; Exceptions; Open Questions; Required Professional Review. Do not claim legal approval."
        }
        _ => {
            "Produce an operational document with explicit purpose, scope, inputs, requirements, acceptance criteria, risks, open questions, and dependencies. Preserve the ordered cascade and label assumptions."
        }
    }
}

fn evaluate_gate(doc: &DocumentRevision) -> GateResult {
    if doc
        .generation_finish_reason
        .as_deref()
        .is_some_and(|reason| reason != "stop")
    {
        return GateResult {document_type:doc.document_type.clone(),status:"incomplete".into(),score:None,criteria:BTreeMap::new(),threshold:None,reason:"The provider returned an incomplete draft. No gate pass is inferred from its partial score text.".into(),evaluated_at:now(),semantic:None,questions:Vec::new(),repairs:Vec::new(),questions_note:None};
    }
    let threshold = crate::gate_rubric::bar(&doc.document_type).map(|(min, _)| min);
    let parsed = parse_gate_text(&doc.content);
    let (score, criteria) = match parsed {
        Ok(v) => v,
        Err(reason) => {
            return GateResult {
                document_type: doc.document_type.clone(),
                status: "failed".into(),
                score: None,
                criteria: BTreeMap::new(),
                threshold,
                reason: format!(
                    "The gate gave no score that can be read ({reason}), so it does not pass."
                ),
                evaluated_at: now(),
                semantic: None,
                questions: Vec::new(),
                repairs: Vec::new(),
                questions_note: None,
            };
        }
    };
    let criterion_floor =
        crate::gate_rubric::bar(&doc.document_type).map_or(70, |(_, floor)| floor);
    let criteria_ok = !criteria.is_empty()
        && score.is_some_and(|value| value <= 100)
        && criteria.values().map(|c| u64::from(c.score)).sum::<u64>() * 100
            == u64::from(score.unwrap_or(0))
                * criteria.values().map(|c| u64::from(c.max)).sum::<u64>()
        && criteria.values().all(|item| {
            item.max > 0
                && item.score <= item.max
                && item.score.saturating_mul(100) >= item.max.saturating_mul(criterion_floor)
        });
    // The verdict must score exactly the rubric's named criteria at their maxima; a
    // missing, extra or re-weighted criterion never passes (gate contract, section 1).
    let identity = threshold.and_then(|_| criterion_identity(&doc.document_type, &criteria));
    let criteria_ok = criteria_ok && identity.is_none();
    let status = match (threshold, score) {
        (Some(_), None) => "failed",
        (Some(min), Some(value)) if value >= min && criteria_ok => "passed",
        (Some(_), Some(_)) => "failed",
        _ => "not_applicable",
    };
    let reason = gate_reason(
        score,
        &criteria,
        threshold,
        criterion_floor,
        identity.as_deref(),
    );
    let (questions, repairs, unreadable) =
        parse_gate_questions(&doc.content, &doc.document_type, &criteria);
    let questions_note = questions_note(&questions, unreadable);
    GateResult {
        document_type: doc.document_type.clone(),
        status: status.into(),
        score,
        criteria,
        threshold,
        reason,
        evaluated_at: now(),
        semantic: None,
        questions,
        repairs,
        questions_note,
    }
}

/// The receipt's plain words about the owner's questions: how many, what they hold back,
/// and that none was answered for him.
fn questions_note(questions: &[GateQuestion], unreadable: usize) -> Option<String> {
    let mut parts = Vec::new();
    if !questions.is_empty() {
        let held = questions
            .iter()
            .map(|q| q.criterion.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!(
            "{} {} only the person can answer {} back {held}; no answer was invented.",
            questions.len(),
            if questions.len() == 1 {
                "question"
            } else {
                "questions"
            },
            if questions.len() == 1 {
                "holds"
            } else {
                "hold"
            }
        ));
    }
    if unreadable > 0 {
        parts.push(format!(
            "{unreadable} question or repair {} could not be read and {} not kept.",
            if unreadable == 1 { "line" } else { "lines" },
            if unreadable == 1 { "was" } else { "were" }
        ));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Reads the gate's `[QUESTION] criterion | DOCUMENT | text` and
/// `[REPAIR] DOCUMENT | criterion | text` lines. A line naming a criterion the gate did not
/// score, or a document the gate does not join, is not kept and is counted as unreadable.
fn parse_gate_questions(
    content: &str,
    gate: &str,
    criteria: &BTreeMap<String, CriterionResult>,
) -> (Vec<GateQuestion>, Vec<GateRepair>, usize) {
    let joined = crate::registry::product_inputs(gate).unwrap_or(&[]);
    let document = |text: &str| {
        let kind = text.trim().replace([' ', '-'], "_").to_ascii_uppercase();
        (kind != "IDEA"
            && (joined.is_empty() || joined.contains(&kind.as_str()))
            && normalized_document(&kind).is_ok())
        .then_some(kind)
    };
    let criterion = |text: &str| {
        let name = text.trim().trim_matches('*').to_ascii_lowercase();
        criteria.contains_key(&name).then_some(name)
    };
    let (mut questions, mut repairs, mut unreadable) = (Vec::new(), Vec::new(), 0);
    for line in content.lines() {
        let line = line
            .trim()
            .trim_start_matches(|c: char| {
                c.is_ascii_digit() || matches!(c, '-' | '*' | '.' | ')' | ' ')
            })
            .trim_matches('*')
            .trim();
        let upper = line.to_ascii_uppercase();
        let (is_question, rest) = if upper.starts_with("[QUESTION]") {
            (true, &line["[QUESTION]".len()..])
        } else if upper.starts_with("[REPAIR]") {
            (false, &line["[REPAIR]".len()..])
        } else {
            continue;
        };
        let rest = rest.trim().trim_start_matches(':').trim();
        if rest.eq_ignore_ascii_case("none") {
            continue;
        }
        let fields = rest.splitn(3, '|').map(str::trim).collect::<Vec<_>>();
        let [first, second, text] = fields[..] else {
            unreadable += 1;
            continue;
        };
        if text.is_empty() {
            unreadable += 1;
            continue;
        }
        if is_question {
            match (criterion(first), document(second)) {
                (Some(criterion), Some(document)) => {
                    let question = GateQuestion {
                        criterion,
                        document,
                        question: text.into(),
                    };
                    if !questions.contains(&question) {
                        questions.push(question);
                    }
                }
                _ => unreadable += 1,
            }
        } else {
            match (document(first), criterion(second)) {
                (Some(document), Some(criterion)) => repairs.push(GateRepair {
                    document,
                    criterion,
                    repair: text.into(),
                }),
                _ => unreadable += 1,
            }
        }
    }
    (questions, repairs, unreadable)
}

/// States in plain words what decided the gate: the total, a floor, or a missing score.
/// Plain words for how a verdict's criteria differ from the gate's rubric, or `None` when
/// they are exactly the rubric's names and maxima. An unreadable rubric is itself a mismatch.
fn criterion_identity(gate: &str, criteria: &BTreeMap<String, CriterionResult>) -> Option<String> {
    if criteria.is_empty() {
        return None;
    }
    let rubric = match crate::gate_rubric::rubric(gate) {
        Ok(Some(rubric)) => rubric,
        Ok(None) | Err(_) => {
            return Some(
                "The gate's rubric could not be read, so its criteria cannot be checked and it does not pass."
                    .into(),
            );
        }
    };
    let expected = rubric
        .criteria
        .iter()
        .map(|c| (c.name.to_ascii_lowercase(), c.max))
        .collect::<BTreeMap<_, _>>();
    let missing = expected
        .keys()
        .filter(|name| !criteria.contains_key(*name))
        .cloned()
        .collect::<Vec<_>>();
    let extra = criteria
        .keys()
        .filter(|name| !expected.contains_key(*name))
        .cloned()
        .collect::<Vec<_>>();
    let reweighted = criteria
        .iter()
        .filter_map(|(name, c)| {
            expected
                .get(name)
                .filter(|max| **max != c.max)
                .map(|max| format!("{name} is scored out of {} instead of {max}", c.max))
        })
        .collect::<Vec<_>>();
    if missing.is_empty() && extra.is_empty() && reweighted.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if !missing.is_empty() {
        parts.push(format!("it leaves out {}", missing.join(", ")));
    }
    if !extra.is_empty() {
        parts.push(format!(
            "it scores {}, which the rubric does not name",
            extra.join(", ")
        ));
    }
    parts.extend(reweighted);
    Some(format!(
        "The gate's criteria do not match its rubric: {}; it does not pass.",
        parts.join("; ")
    ))
}

fn gate_reason(
    score: Option<u32>,
    criteria: &BTreeMap<String, CriterionResult>,
    threshold: Option<u32>,
    criterion_floor: u32,
    identity: Option<&str>,
) -> String {
    let Some(score) = score else {
        return "The gate gave no score: no [SCORE] marker was found, so it does not pass and no score is inferred.".into();
    };
    if criteria.is_empty() {
        return "Score parsed from document text; no criterion breakdown was supplied, so the gate remains failed until explicit criteria are present.".into();
    }
    let mut parts = Vec::new();
    let sum: u64 = criteria.values().map(|c| u64::from(c.score)).sum();
    let max: u64 = criteria.values().map(|c| u64::from(c.max)).sum();
    if sum * 100 != u64::from(score) * max {
        parts.push(format!(
            "The criteria add up to {sum} of {max}, which does not match the score of {score}."
        ));
    }
    if let Some(bar) = threshold.filter(|bar| score < *bar) {
        parts.push(format!("Score {score} of 100 is below the bar of {bar}."));
    }
    for (name, item) in criteria {
        if item.score.saturating_mul(100) < item.max.saturating_mul(criterion_floor) {
            let floor = (item.max * criterion_floor).div_ceil(100);
            let mut label = name.clone();
            if let Some(first) = label.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            parts.push(format!(
                "{label} scored {} of {}, below its floor of {floor}.",
                item.score, item.max
            ));
        }
    }
    if let Some(identity) = identity {
        parts.push(identity.to_owned());
    }
    if !parts.is_empty() {
        return parts.join(" ");
    }
    match threshold {
        Some(bar) => format!(
            "Score {score} of 100 meets the bar of {bar}, and every criterion meets its floor."
        ),
        None => format!("Score {score} of 100; this document has no gate bar."),
    }
}

/// Native legacy score/criteria grammar, deliberately smaller than semantic gate profiles.
fn parse_gate_text(
    content: &str,
) -> std::result::Result<(Option<u32>, BTreeMap<String, CriterionResult>), String> {
    let mut score = None;
    let mut criteria = BTreeMap::new();
    // Criteria are read only from the score block: the bullet lines that directly
    // follow the [SCORE] line. Prose, headings and bold summaries elsewhere are ignored.
    let mut in_block = false;
    for line in content.lines() {
        let upper = line.to_ascii_uppercase();
        if upper.contains("[SCORE]") {
            if score.is_some() {
                return Err("Duplicate [SCORE] markers".into());
            }
            let candidate = line.trim().trim_matches('*').trim();
            let value = candidate
                .strip_prefix("[SCORE]")
                .ok_or("Malformed [SCORE] marker")?
                .trim()
                .trim_start_matches(':')
                .trim();
            let value = value.strip_suffix("/100").unwrap_or(value).trim();
            if value.is_empty() || !value.chars().all(|c| c.is_ascii_digit()) {
                return Err("Malformed gate score".into());
            }
            let n = value.parse::<u32>().map_err(|_| "Invalid gate score")?;
            if n > 100 {
                return Err("Gate score exceeds 100".into());
            }
            score = Some(n);
            in_block = true;
            continue;
        }
        if !in_block {
            continue;
        }
        let candidate = line.trim();
        if candidate.is_empty() && criteria.is_empty() {
            continue;
        }
        if !(candidate.starts_with('-') || candidate.starts_with('*')) {
            in_block = false;
            continue;
        }
        let candidate = candidate.trim_start_matches(['-', '*']).trim();
        // A bullet in the block without the "name: n/max" shape ends the block.
        let Some((name, values)) = candidate.split_once(':').filter(|(_, v)| v.contains('/'))
        else {
            in_block = false;
            continue;
        };
        let name = name.trim().trim_matches('*').to_ascii_lowercase();
        let (score_text, rest) = values.split_once('/').ok_or("Malformed criterion")?;
        let (max_text, feedback) = rest
            .split_once('-')
            .ok_or("Criterion feedback is required")?;
        let score = score_text
            .trim()
            .parse::<u32>()
            .map_err(|_| "Malformed criterion score")?;
        let max = max_text
            .trim()
            .parse::<u32>()
            .map_err(|_| "Malformed criterion maximum")?;
        if name.is_empty() || max == 0 || score > max || feedback.trim().is_empty() {
            return Err("Invalid criterion name/range/feedback".into());
        }
        if criteria
            .insert(
                name,
                CriterionResult {
                    score,
                    max,
                    feedback: feedback.trim().into(),
                },
            )
            .is_some()
        {
            return Err("Duplicate gate criterion".into());
        }
    }
    Ok((score, criteria))
}

pub(crate) fn parse_local_endpoint(endpoint: &str) -> Result<(String, u16)> {
    let (host, port) = endpoint
        .rsplit_once(':')
        .ok_or_else(|| invalid("Q-ROC gateway must be host:port"))?;
    if host != "127.0.0.1" && host != "localhost" && host != "[::1]" {
        return Err(invalid(
            "Q-ROC generation is local-only; gateway host must be loopback",
        ));
    }
    Ok((
        host.trim_matches(['[', ']']).into(),
        port.parse().map_err(|_| invalid("Invalid gateway port"))?,
    ))
}

pub(crate) fn document_order_map() -> Value {
    json!({"PRODUCT":PRODUCT_DOCUMENTS,"BUSINESS":BUSINESS_DOCUMENTS,"PRESENCE":PRESENCE_DOCUMENTS,"LEGAL":LEGAL_DOCUMENTS})
}
pub(crate) fn document_order(document_type: &str) -> usize {
    PRODUCT_DOCUMENTS
        .iter()
        .chain(BUSINESS_DOCUMENTS.iter())
        .chain(PRESENCE_DOCUMENTS.iter())
        .chain(LEGAL_DOCUMENTS.iter())
        .position(|item| *item == document_type)
        .unwrap_or(usize::MAX)
}
pub(crate) fn domain_for(document_type: &str) -> &str {
    if PRODUCT_DOCUMENTS.contains(&document_type) {
        "PRODUCT"
    } else if BUSINESS_DOCUMENTS.contains(&document_type) {
        "BUSINESS"
    } else if PRESENCE_DOCUMENTS.contains(&document_type) {
        "PRESENCE"
    } else {
        "LEGAL"
    }
}
pub(crate) fn normalized_document(value: &str) -> Result<String> {
    let value = value.trim().to_ascii_uppercase();
    if value.is_empty()
        || ![
            PRODUCT_DOCUMENTS.as_slice(),
            BUSINESS_DOCUMENTS.as_slice(),
            PRESENCE_DOCUMENTS.as_slice(),
            LEGAL_DOCUMENTS.as_slice(),
        ]
        .iter()
        .any(|group| group.contains(&value.as_str()))
    {
        return Err(invalid("Unknown Q-ROC document type"));
    }
    Ok(value)
}
fn document_paths(root: &Path, project_id: &str, branch_id: &str) -> Result<Vec<PathBuf>> {
    let project = safe_component(project_id, "project_id")?;
    let branch = safe_component(branch_id, "branch_id")?;
    let dir = root.join("documents").join(project).join(branch);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    Ok(fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|v| v.path()))
        .filter(|p| p.extension().and_then(|v| v.to_str()) == Some("json"))
        .collect())
}

fn find_document_by_sha(
    root: &Path,
    project_id: &str,
    branch_id: &str,
    document_type: &str,
    sha256: &str,
) -> Result<Option<DocumentRevision>> {
    for path in document_paths(root, project_id, branch_id)? {
        let document: DocumentRevision = read_json(&path).map_err(|error| {
            invalid(format!(
                "Could not inspect Q-ROC document {} while reconciling generation: {error}",
                path.display()
            ))
        })?;
        if document.document_type == document_type && document.sha256 == sha256 {
            return Ok(Some(document));
        }
    }
    Ok(None)
}

fn next_revision(
    root: &Path,
    project_id: &str,
    branch_id: &str,
    document_type: &str,
) -> Result<u32> {
    Ok(document_paths(root, project_id, branch_id)?
        .into_iter()
        .filter_map(|p| read_json::<DocumentRevision>(&p).ok())
        .filter(|d| d.document_type == document_type)
        .map(|d| d.revision)
        .max()
        .unwrap_or(0)
        .saturating_add(1))
}

fn current_revision(
    root: &Path,
    project_id: &str,
    branch_id: &str,
    document_type: &str,
) -> Result<u32> {
    Ok(next_revision(root, project_id, branch_id, document_type)?.saturating_sub(1))
}

fn parse_sources(args: &Value) -> Result<Vec<SourceRef>> {
    let Some(items) = args["sources"].as_array() else {
        return Ok(Vec::new());
    };
    if items.len() > 64 {
        return Err(invalid("At most 64 sources may be attached"));
    }
    items
        .iter()
        .map(|item| {
            let path = Path::new(s(item, "path"));
            if !path.is_absolute() {
                return Err(invalid("Attached source paths must be absolute"));
            }
            let metadata = fs::symlink_metadata(path)?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(invalid(
                    "Attached source must be a regular file, not a symlink",
                ));
            }
            if metadata.len() > MAX_SOURCE_BYTES {
                return Err(invalid("Attached source exceeds 16 MiB"));
            }
            let mut file = File::open(path)?;
            let mut hasher = Sha256::new();
            let mut buf = [0u8; 64 * 1024];
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            Ok(SourceRef {
                id: s(item, "id").to_owned(),
                name: s(item, "name").to_owned(),
                path: path.to_string_lossy().into_owned(),
                sha256: format!("{:x}", hasher.finalize()),
                size: metadata.len(),
            })
        })
        .collect()
}

fn collect_import_files(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.is_absolute() {
        return Err(invalid("Q-ROC import path must be absolute"));
    }
    let metadata = fs::symlink_metadata(root)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid("Q-ROC import path must be a real directory"));
    }
    let mut files = Vec::new();
    let mut stack = vec![(root.to_owned(), 0_u8)];
    while let Some((directory, depth)) = stack.pop() {
        if depth > 8 {
            return Err(invalid(
                "Q-ROC import folder exceeds the directory-depth bound",
            ));
        }
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.components().any(|component| {
                component
                    .as_os_str()
                    .to_string_lossy()
                    .starts_with("__DISPOSAL__")
            }) {
                return Err(invalid(
                    "Q-ROC import cannot read disposal or superseded folders",
                ));
            }
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                return Err(invalid("Q-ROC import folder cannot contain symlinks"));
            }
            if metadata.is_dir() {
                stack.push((path, depth + 1));
            } else if metadata.is_file() {
                let extension = path.extension().and_then(|value| value.to_str());
                if !matches!(extension, Some("md") | Some("markdown") | Some("txt")) {
                    continue;
                }
                files.push(path);
                if files.len() > 500 {
                    return Err(invalid("Q-ROC import folder exceeds the 500-file bound"));
                }
            }
        }
    }
    files.sort();
    Ok(files)
}

fn imported_document_type(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?.to_ascii_uppercase();
    let normalized = stem
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() {
                value
            } else {
                '_'
            }
        })
        .collect::<String>();
    let mut candidates = PRODUCT_DOCUMENTS
        .iter()
        .chain(BUSINESS_DOCUMENTS.iter())
        .chain(PRESENCE_DOCUMENTS.iter())
        .chain(LEGAL_DOCUMENTS.iter())
        .filter(|document| normalized.contains(**document))
        .copied()
        .collect::<Vec<_>>();
    if candidates.is_empty() && normalized.contains("TAD") {
        candidates.push("TECHNICAL_ARCHITECTURE");
    }
    candidates
        .into_iter()
        .max_by_key(|value| value.len())
        .map(str::to_owned)
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.file_type().is_symlink()
    {
        return Err(invalid("Q-ROC source path cannot be a symlink"));
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid("System clock is before the Unix epoch"))?
        .as_nanos();
    let temporary = path.with_file_name(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("source"),
        std::process::id(),
        nonce
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn safe_component(value: &str, name: &str) -> Result<String> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(|c| c.is_control())
    {
        return Err(invalid(format!("Invalid {name}")));
    }
    Ok(value.to_owned())
}
fn bounded_text(value: &str, max: usize, name: &str) -> Result<String> {
    if value.len() > max {
        return Err(invalid(format!("{name} exceeds {max} bytes")));
    }
    Ok(value.to_owned())
}
fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid("Q-ROC record must be a regular file"));
    }
    if metadata.len() > MAX_RECORD_BYTES {
        return Err(invalid("Q-ROC record exceeds the storage bound"));
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.file_type().is_symlink()
    {
        return Err(invalid("Q-ROC record path cannot be a symlink"));
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid("System clock is before the Unix epoch"))?
        .as_nanos();
    let tmp = path.with_file_name(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("record"),
        std::process::id(),
        nonce
    ));
    let mut file = OpenOptions::new().create_new(true).write(true).open(&tmp)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    fs::rename(&tmp, path)?;
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn write_json_noreplace<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_bytes_noreplace(path, &bytes)
}

fn write_bytes_noreplace(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.file_type().is_symlink()
    {
        return Err(invalid("Q-ROC record path cannot be a symlink"));
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid("System clock is before the Unix epoch"))?
        .as_nanos();
    let temporary = path.with_file_name(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("record"),
        std::process::id(),
        nonce
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    // hard_link gives immutable revisions no-overwrite publication semantics:
    // an existing target (including a symlink) cannot be replaced by a race.
    let result = fs::hard_link(&temporary, path);
    let _ = fs::remove_file(&temporary);
    result?;
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn s<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
fn invalid(value: impl Into<String>) -> Error {
    Error::Invalid(value.into())
}

fn default_generation_timeout_seconds() -> u64 {
    120
}
fn generation_timeout_seconds(value: Option<&str>) -> Result<u64> {
    match value {
        None => Ok(default_generation_timeout_seconds()),
        Some(text) => text
            .parse::<u64>()
            .ok()
            .filter(|n| (15..=900).contains(n))
            .ok_or_else(|| invalid("QROC_TIMEOUT_SECONDS must be an integer from 15 to 900")),
    }
}
#[cfg(test)]
mod deadline_tests {
    #[test]
    fn explicit_local_generation_deadline_is_bounded_and_default_is_preserved() {
        assert_eq!(super::generation_timeout_seconds(None).unwrap(), 120);
        for value in [15, 600, 900] {
            assert_eq!(
                super::generation_timeout_seconds(Some(&value.to_string())).unwrap(),
                value
            );
        }
        for value in ["0", "14", "901", "-1", "forever", ""] {
            assert!(super::generation_timeout_seconds(Some(value)).is_err());
        }
    }
}

/// The output limit a finished job was really held to: what its reply said the route honoured;
/// for a job that predates that, the route's own receipt when `QROC_SHIM_RECEIPTS` names the
/// route shim's receipts file (matched by route and the job's run window, length-stopped and
/// honoured); otherwise the limit QROC asked for.
fn honoured_limit(job: &Value) -> u64 {
    honoured_limit_in(
        job,
        std::env::var_os("QROC_SHIM_RECEIPTS")
            .as_deref()
            .map(Path::new),
    )
}
fn honoured_limit_in(job: &Value, receipts: Option<&Path>) -> u64 {
    let asked = job["max_tokens"].as_u64().unwrap_or(0);
    if let Some(n) = job["honoured_max_tokens"].as_u64() {
        return n;
    }
    let Some(path) = receipts else {
        return asked;
    };
    let route = job["pinned_inputs"]["route"].as_str().unwrap_or_default();
    let (Some(started), Some(finished)) = (job["started_at"].as_i64(), job["finished_at"].as_i64())
    else {
        return asked;
    };
    let window = (started.saturating_mul(1000) - 1000)..=(finished.saturating_mul(1000) + 1000);
    let Ok(text) = fs::read_to_string(path) else {
        return asked;
    };
    text.lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|r| {
            r["route"] == route
                && r["finish_reason"] == "length"
                && r["max_tokens_honoured"] == true
                && r["ts"].as_i64().is_some_and(|ts| window.contains(&ts))
        })
        .find_map(|r| r["max_tokens"].as_u64())
        .unwrap_or(asked)
}

/// The most a generation may ask for: 2048 on the local gateway; on a paid route the limit the
/// shared API layer's route table sets for it, or 16384 when the layer does not know the route.
fn generation_token_ceiling(root: &Path) -> Result<u32> {
    if crate::registry::provider_name_at(root)? == "local-gateway" {
        return Ok(2048);
    }
    Ok(route_token_ceiling(layer_output_tokens(root)))
}
fn route_token_ceiling(layer_limit: Option<u64>) -> u32 {
    layer_limit
        .filter(|n| *n > 0)
        .map_or(16384, |n| n.min(u64::from(u32::MAX)) as u32)
}
/// The output limit the shared API layer's route table sets for the selected route, when it
/// knows that route: the route's own label, else the provider's chat route for the model (an
/// OpenAI key route reads `openai:<model>`), else a route serving the same model (the smallest
/// limit when several do), else OpenRouter's entry for the model.
fn layer_output_tokens(root: &Path) -> Option<u64> {
    layer_output_tokens_in(root, &stratum_api::Layer::open().ok()?)
}
fn layer_output_tokens_in(root: &Path, layer: &stratum_api::Layer) -> Option<u64> {
    let selected = crate::routes::selected_at(root).ok()??;
    if let Ok(route) = layer.route(&selected.model) {
        return Some(route.max_output);
    }
    let model = ["apikey:", "anthropic:", "openai:", "openrouter:"]
        .iter()
        .find_map(|p| selected.model.strip_prefix(p))
        .unwrap_or(&selected.model);
    let provider = if selected.model.starts_with("anthropic:") {
        Some("anthropic")
    } else if selected.model.starts_with("apikey:") || selected.model.starts_with("openai:") {
        Some("openai")
    } else {
        None
    };
    if let Some(route) = provider.and_then(|p| layer.route(&format!("{p}:{model}")).ok()) {
        return Some(route.max_output);
    }
    let same_model = stratum_api::chat::routes(&layer.routes_path())
        .ok()?
        .into_iter()
        .filter(|r| r.model == model)
        .map(|r| r.max_output)
        .min();
    same_model.or_else(|| {
        (selected.kind == crate::routes::Kind::Openrouter)
            .then(|| layer.route(&format!("openrouter:{model}")).ok())
            .flatten()
            .map(|r| r.max_output)
    })
}
fn generation_max_tokens(root: &Path, value: Option<&str>) -> Result<u32> {
    let ceiling = generation_token_ceiling(root)?;
    // A declared QROC_MAX_TOKENS wins; without one, the shared layer's limit for the route.
    if value.is_none()
        && let Some(limit) = layer_output_tokens(root).filter(|n| *n > 0)
    {
        return Ok(limit.min(u64::from(ceiling)) as u32);
    }
    let value = value.ok_or_else(|| {
        invalid(format!(
            "QROC_MAX_TOKENS must be explicitly declared (1..{ceiling}); the shared API layer sets no output limit for this route"
        ))
    })?;
    let error = || {
        invalid(format!(
            "QROC_MAX_TOKENS must be an integer from 1 to {ceiling}"
        ))
    };
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(error());
    }
    let n = value.parse::<u32>().map_err(|_| error())?;
    if !(1..=ceiling).contains(&n) {
        return Err(error());
    }
    Ok(n)
}

#[derive(Debug, PartialEq, Eq)]
enum BatchReadiness {
    Ready,
    Waiting,
    Blocked,
}
fn batch_readiness(states: &[Option<&str>]) -> BatchReadiness {
    if states.iter().any(|state| {
        !matches!(
            state,
            Some("succeeded" | "running" | "queued" | "cancelling")
        )
    }) {
        BatchReadiness::Blocked
    } else if states.iter().any(|state| *state != Some("succeeded")) {
        BatchReadiness::Waiting
    } else {
        BatchReadiness::Ready
    }
}
#[cfg(test)]
mod layer_output_tests {
    use super::*;
    fn scratch(name: &str) -> PathBuf {
        let base = std::env::var_os("QROC_TEST_ROOT").expect("explicit private QROC_TEST_ROOT");
        let dir = PathBuf::from(base).join(format!("layer-output-{name}-{}", std::process::id()));
        fs::create_dir_all(dir.join("config")).unwrap();
        dir
    }
    fn select(root: &Path, kind: &str, model: &str) {
        let route = if kind == "openrouter" {
            json!({"name":"chosen","kind":kind,"endpoint":"https://openrouter.ai/api","model":model,"key_ref":"OPENROUTER_API_KEY"})
        } else {
            json!({"name":"chosen","kind":kind,"endpoint":"127.0.0.1:9","model":model})
        };
        fs::write(
            root.join("routes.json"),
            json!({"version":crate::routes::TABLE_VERSION,"selected":"chosen","routes":[route]})
                .to_string(),
        )
        .unwrap();
    }
    #[test]
    fn the_layer_route_sets_the_default_and_a_declared_limit_wins() {
        let dir = scratch("routes");
        fs::write(
            dir.join("config/routes.json"),
            json!({"routes":{
                "openai:gpt-6.1-sol":{"provider":"openai","model":"gpt-6.1-sol","max_output_tokens":16000},
                "anthropic:claude-opus-5-5":{"provider":"anthropic","model":"claude-opus-5-5","max_output_tokens":12000},
                "openrouter:*":{"provider":"openrouter","max_output_tokens":9000}}})
            .to_string(),
        )
        .unwrap();
        let layer = stratum_api::Layer::at(dir.join("home"), dir.join("config"));
        let root = dir.join("store");
        fs::create_dir_all(&root).unwrap();
        assert_eq!(
            layer_output_tokens_in(&root, &layer),
            None,
            "no route selected"
        );
        select(&root, "relay", "apikey:gpt-6.1-sol");
        assert_eq!(layer_output_tokens_in(&root, &layer), Some(16000));
        select(&root, "relay", "anthropic:claude-opus-5-5");
        assert_eq!(layer_output_tokens_in(&root, &layer), Some(12000));
        select(&root, "openrouter", "qwen/qwen3.8-27b:free");
        assert_eq!(layer_output_tokens_in(&root, &layer), Some(9000));
        select(&root, "relay", "apikey:unknown-model");
        assert_eq!(layer_output_tokens_in(&root, &layer), None);
        assert_eq!(generation_max_tokens(&root, Some("1024")).unwrap(), 1024);
    }
    #[test]
    fn the_honoured_limit_comes_from_the_reply_then_the_shim_receipt_then_the_ask() {
        let job = json!({"max_tokens":128000,"started_at":1000,"finished_at":1600,
            "pinned_inputs":{"route":"anthropic:claude-opus-5-5"}});
        assert_eq!(honoured_limit_in(&job, None), 128000, "no receipt: the ask");
        let mut said = job.clone();
        said["honoured_max_tokens"] = json!(64000);
        assert_eq!(
            honoured_limit_in(&said, None),
            64000,
            "the reply's word first"
        );
        let dir = scratch("receipts");
        let path = dir.join("receipts.jsonl");
        let row = |ts: i64, route: &str, finish: &str, max: u64| {
            json!({"ts":ts,"route":route,"finish_reason":finish,"max_tokens":max,"max_tokens_honoured":true}).to_string()
        };
        fs::write(
            &path,
            [
                row(900_000, "anthropic:claude-opus-5-5", "length", 32000),
                row(1_200_000, "anthropic:claude-opus-5-5", "stop", 16000),
                row(1_300_000, "openai:gpt-6.1-sol", "length", 16000),
                row(1_400_000, "anthropic:claude-opus-5-5", "length", 64000),
            ]
            .join("\n"),
        )
        .unwrap();
        assert_eq!(
            honoured_limit_in(&job, Some(&path)),
            64000,
            "this job's own length-stopped receipt"
        );
        let elsewhen = json!({"max_tokens":128000,"started_at":5000,"finished_at":5600,
            "pinned_inputs":{"route":"anthropic:claude-opus-5-5"}});
        assert_eq!(
            honoured_limit_in(&elsewhen, Some(&path)),
            128000,
            "no receipt in its window"
        );
    }

    #[test]
    fn a_paid_route_may_ask_for_its_layer_limit_beyond_16384() {
        assert_eq!(route_token_ceiling(Some(32000)), 32000, "Opus on the layer");
        assert_eq!(
            route_token_ceiling(Some(16000)),
            16000,
            "OpenAI on the layer"
        );
        assert_eq!(
            route_token_ceiling(None),
            16384,
            "a route the layer does not know"
        );
        assert_eq!(route_token_ceiling(Some(0)), 16384);
        let dir = scratch("ceiling");
        fs::write(
            dir.join("config/routes.json"),
            json!({"routes":{"anthropic:claude-opus-5-5":{"provider":"anthropic","model":"claude-opus-5-5","max_output_tokens":32000}}})
                .to_string(),
        )
        .unwrap();
        let layer = stratum_api::Layer::at(dir.join("home"), dir.join("config"));
        let root = dir.join("store");
        fs::create_dir_all(&root).unwrap();
        select(&root, "relay", "anthropic:claude-opus-5-5");
        assert_eq!(
            route_token_ceiling(layer_output_tokens_in(&root, &layer)),
            32000
        );
        // The OpenAI key route reads the chat route, not a smaller same-model entry beside it.
        fs::write(
            dir.join("config/routes.json"),
            json!({"routes":{
                "openai:gpt-6.1-sol":{"provider":"openai","model":"gpt-6.1-sol","max_output_tokens":32000},
                "responses:openai":{"provider":"openai","model":"gpt-6.1-sol","max_output_tokens":16000}}})
            .to_string(),
        )
        .unwrap();
        select(&root, "relay", "apikey:gpt-6.1-sol");
        assert_eq!(layer_output_tokens_in(&root, &layer), Some(32000));
    }
}

#[cfg(test)]
mod graph_batch_tests {
    use super::*;
    #[test]
    fn planning_resolution_never_instructs_gate_judges() {
        let project: Project = serde_json::from_value(json!({
            "id":"planning-fixture", "title":"Fixture", "brain_dump":"Supplied need",
            "created_at":0, "updated_at":0
        }))
        .expect("static project fixture");
        let branch: Branch = serde_json::from_value(json!({
            "id":"planning-branch", "project_id":project.id, "name":"Fixture",
            "model":"fixture", "provider":"fixture", "status":"active", "created_at":0
        }))
        .expect("static branch fixture");
        assert!(
            qroc_prompt(&project, &branch, "PRD", "source context")
                .contains("Planning resolution:")
        );
        for gate in ["GATE_1", "GATE_2", "GATE_3", "GATE_4"] {
            assert!(
                !qroc_prompt(&project, &branch, gate, "source context")
                    .contains("Planning resolution:"),
                "{gate}"
            );
        }
    }
    #[test]
    fn dependency_readiness_preserves_failures_and_partial_results() {
        assert_eq!(batch_readiness(&[]), BatchReadiness::Ready);
        assert_eq!(
            batch_readiness(&[Some("succeeded"), Some("running")]),
            BatchReadiness::Waiting
        );
        for state in [
            Some("partial"),
            Some("failed"),
            Some("blocked"),
            Some("cancelled"),
            None,
        ] {
            assert_eq!(
                batch_readiness(&[Some("queued"), state]),
                BatchReadiness::Blocked
            );
        }
    }
    #[test]
    fn failed_gate_blocks_ready_batch_without_a_provider_request() {
        let root = std::path::PathBuf::from(
            std::env::var_os("QROC_TEST_ROOT").expect("explicit private QROC_TEST_ROOT"),
        )
        .join(format!(
            "gate-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let mut store = QrocStore::open(&root).expect("private test store");
        let project = store
            .command(
                "qroc-project-create",
                &json!({"title":"Gate fixture","brain_dump":"Private fixture"}),
            )
            .expect("project");
        let branch = store
            .command(
                "qroc-branch-create",
                &json!({"project_id":project["id"],"name":"Fixture"}),
            )
            .expect("branch");
        // Test fixture: a passed hardening record, so the gate fixture below can be written.
        let idea = "Private fixture";
        fs::create_dir_all(root.join("hardening")).expect("hardening dir");
        fs::write(root.join("hardening").join(format!("{}.json", s(&project, "id"))), json!({"schema":"qroc-hardening/1","project":project["id"],"rounds":[{"round":1,"at":0,"idea_text":idea,"idea_sha256":digest(idea.as_bytes()),"score":{"total":100,"idea":100,"config":0,"missing":[]},"verdict":"pass","sent_back":false,"questions":null,"answers":null}],"route":null}).to_string()).expect("hardening fixture");
        store.command("qroc-document-save", &json!({"project_id":project["id"],"branch_id":branch["id"],"document_type":"GATE_1","content":"[SCORE]: 1\n- Evidence: 1/100 - private failing fixture"})).expect("gate fixture");
        let gate: JobRecord = serde_json::from_value(json!({"id":"gate-source","project_id":project["id"],"branch_id":branch["id"],"document_type":"GATE_1","state":"succeeded","model":"no-model","started_at":0,"batch_id":"batch","batch_width":3})).expect("job fixture");
        let mut next = gate.clone();
        next.id = "next".into();
        next.document_type = "ROADMAP".into();
        next.state = "queued".into();
        next.dependency_jobs = vec![gate.id.clone()];
        store.jobs.insert(gate.id.clone(), gate);
        store.jobs.insert(next.id.clone(), next);
        store.poll_jobs().expect("evaluate local gate");
        assert_eq!(store.jobs["next"].state, "blocked");
        assert!(
            store.jobs["next"]
                .error
                .as_deref()
                .is_some_and(|s| s.contains("gate did not pass"))
        );
        assert!(store.running.is_empty());
    }
}
