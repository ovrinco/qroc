//! Revise from gate: on the operator's press only, a failed gate's joined documents are
//! regenerated with the gate's criterion feedback as an added input, then the gate is
//! regenerated and evaluated. A call holds at most `MAX_ROUNDS` rounds; a pass stops early.
//! Every round is the operator's own press (design record entry 42): a failed round leaves
//! the call failed and waiting for his next press, and a call whose rounds are used up stays
//! failed. Thresholds stay the engine's own; this loop never touches them.
//!
//! Questions only the owner can answer are kept with the gate's record, open until he
//! answers or withdraws one. No round is spent on a document whose shortfall is such a
//! question, and a press whose every remaining shortfall is his question is refused.
//! When a later review asks the same question in new words, the earlier wording is marked
//! superseded by the id of the wording that replaced it; its text is kept as it was.
//!
//! One record per gate keeps every call with its rounds. Progress is driven by
//! `poll_jobs`, so the jobs are ordinary generation jobs with their usual receipts.
use super::*;

pub(super) const MAX_ROUNDS: u64 = 2;
const MAX_FEEDBACK_GATE_TEXT: usize = 16 * 1024;
/// Share of words two wordings must have in common to count as the same question asked again.
const SAME_QUESTION_OVERLAP: f64 = 0.35;

fn terminal(state: &str) -> bool {
    !matches!(state, "running" | "queued" | "cancelling")
}

impl QrocStore {
    fn revise_path(&self, project: &str, branch: &str, gate: &str) -> Result<PathBuf> {
        Ok(self
            .root
            .join("gate-revisions")
            .join(safe_component(project, "Project")?)
            .join(safe_component(branch, "Branch")?)
            .join(format!("{gate}.json")))
    }

    fn revise_record(&self, project: &str, branch: &str, gate: &str) -> Result<Value> {
        let path = self.revise_path(project, branch, gate)?;
        if path.exists() {
            read_json(&path)
        } else {
            Ok(
                json!({"schema":"qroc-gate-revise/1","project_id":project,"branch_id":branch,"gate":gate,"calls":[]}),
            )
        }
    }

    fn save_revise_record(&self, record: &Value) -> Result<()> {
        let path = self.revise_path(
            s(record, "project_id"),
            s(record, "branch_id"),
            s(record, "gate"),
        )?;
        fs::create_dir_all(path.parent().expect("record has a parent"))?;
        write_json_atomic(&path, record)
    }

    fn revise_owner(&self, args: &Value) -> Result<(String, String, String)> {
        let project = s(args, "project_id");
        let branch = s(args, "branch_id");
        if !self.projects.contains_key(project) {
            return Err(invalid("Q-ROC project not found"));
        }
        if self
            .branches
            .get(branch)
            .is_none_or(|b| b.project_id != project)
        {
            return Err(invalid("Q-ROC branch not found"));
        }
        let gate = normalized_document(s(args, "gate"))?;
        if !gate.starts_with("GATE_") || crate::registry::product_inputs(&gate).is_none() {
            return Err(invalid(
                "Only Gate 1, Gate 2 and Gate 3 can be revised from their feedback",
            ));
        }
        Ok((project.into(), branch.into(), gate))
    }

    pub(super) fn gate_revise_get(&self, args: &Value) -> Result<Value> {
        let (project, branch, gate) = self.revise_owner(args)?;
        let record = self.revise_record(&project, &branch, &gate)?;
        let latest = record["calls"].as_array().and_then(|c| c.last()).cloned();
        let running = latest.as_ref().is_some_and(|c| c["state"] == "running");
        let waiting = latest.as_ref().is_some_and(|c| c["awaiting_press"] == true);
        let open = open_questions(&record);
        Ok(
            json!({"record":record,"latest":latest,"running":running,"awaiting_press":waiting,
            "max_rounds":MAX_ROUNDS,"questions":record["questions"].as_array().cloned().unwrap_or_default(),
            "open_questions":open.len()}),
        )
    }

    /// His answer to, or withdrawal of, one stored question. Only his action calls this;
    /// a proposal in a document is never recorded here. The question is named by its id, or,
    /// on a gate whose questions are not stored yet, by its words as the latest review asks it;
    /// the record first takes in that review's questions and retires their earlier wordings.
    pub(super) fn gate_question_answer(&mut self, args: &Value) -> Result<Value> {
        let (project, branch, gate) = self.revise_owner(args)?;
        let mut record = self.revise_record(&project, &branch, &gate)?;
        if let Ok(document) = self.semantic_current_document(&project, &branch, &gate) {
            let evaluation = self.evaluate_owned_gate(&document);
            reconcile_questions(&mut record, &evaluation, u64::from(document.revision));
        }
        let id = match s(args, "question_id") {
            "" => record["questions"]
                .as_array()
                .and_then(|list| {
                    list.iter().find(|q| {
                        q["standing"] == "open"
                            && q["question"] == s(args, "question")
                            && ["document", "criterion"]
                                .iter()
                                .all(|k| s(args, k).is_empty() || q[*k] == s(args, k))
                    })
                })
                .map(|q| s(q, "id").to_owned())
                .unwrap_or_default(),
            id => id.to_owned(),
        };
        let id = id.as_str();
        let withdraw = args["withdraw"].as_bool().unwrap_or(false);
        let answer = if withdraw {
            String::new()
        } else {
            let text = bounded_text(s(args, "answer"), MAX_BODY / 8, "Answer")?;
            if text.trim().is_empty() {
                return Err(invalid("Write the answer, or withdraw the question"));
            }
            text
        };
        let question = record["questions"]
            .as_array_mut()
            .and_then(|q| q.iter_mut().find(|q| q["id"] == id))
            .ok_or_else(|| invalid("Gate question not found"))?;
        if question["standing"] != "open" {
            return Err(invalid("This question is no longer open"));
        }
        question["standing"] = json!(if withdraw { "withdrawn" } else { "answered" });
        question["answer"] = if withdraw { Value::Null } else { json!(answer) };
        question["settled_at"] = json!(now());
        let question = question.clone();
        self.save_revise_record(&record)?;
        Ok(json!({"question":question,"open_questions":open_questions(&record).len()}))
    }

    pub(super) fn gate_revise_start(&mut self, args: &Value) -> Result<Value> {
        let (project, branch, gate) = self.revise_owner(args)?;
        let mut record = self.revise_record(&project, &branch, &gate)?;
        if let Some(latest) = record["calls"].as_array().and_then(|c| c.last())
            && matches!(latest["state"].as_str(), Some("running" | "blocked"))
        {
            return self.resume_gate_call(&mut record);
        }
        let document = self
            .semantic_current_document(&project, &branch, &gate)
            .map_err(|_| {
                invalid(
                    "This gate has not been evaluated yet, so there is no feedback to revise from",
                )
            })?;
        let evaluation = self.evaluate_owned_gate(&document);
        if reconcile_questions(&mut record, &evaluation, u64::from(document.revision)) {
            self.save_revise_record(&record)?;
        }
        match evaluation.status.as_str() {
            "failed" => {}
            "passed" => return Err(invalid("This gate has passed; there is nothing to revise")),
            _ => {
                return Err(invalid(
                    "This gate's last evaluation is incomplete, not failed; generate the gate again before revising from it",
                ));
            }
        }
        if self.jobs.values().any(|j| {
            j.branch_id == branch && matches!(j.state.as_str(), "running" | "queued" | "cancelling")
        }) {
            return Err(invalid(
                "Inspect or cancel the existing generation in this branch before revising from the gate",
            ));
        }
        // Explicit supplementary planning inputs belong to this press, not to the
        // project's original idea. Persist them across rounds and recovery.
        let context = bounded_text(s(args, "context"), MAX_BODY, "Revise context")?;
        let questions = record["questions"].as_array().cloned().unwrap_or_default();
        // His press continues a call whose failed round waits for him, on the gate that
        // round produced and with the call's own supplementary input.
        if let Some(latest) = record["calls"].as_array().and_then(|c| c.last())
            && latest["state"] == "failed"
            && latest["awaiting_press"] == true
            && latest["rounds"].as_array().map_or(0, Vec::len) < MAX_ROUNDS as usize
            && latest["last"]["gate_revision"] == json!(document.revision)
            && (context.is_empty() || latest["context_sha256"] == json!(digest(context.as_bytes())))
        {
            let mut call = latest.clone();
            call["state"] = json!("running");
            call["awaiting_press"] = json!(false);
            call["finished_at"] = Value::Null;
            call["error"] = Value::Null;
            self.start_round(
                &mut call,
                &project,
                &branch,
                &gate,
                &document,
                &evaluation,
                &questions,
            )?;
            *record["calls"]
                .as_array_mut()
                .and_then(|c| c.last_mut())
                .ok_or_else(|| invalid("Revise call disappeared"))? = call.clone();
            self.save_revise_record(&record)?;
            return Ok(json!({"call":call,"running":true,"max_rounds":MAX_ROUNDS}));
        }
        let number = record["calls"].as_array().map_or(0, Vec::len) + 1;
        let mut call = json!({"call":number,"state":"running","started_at":now(),"finished_at":null,
            "max_rounds":MAX_ROUNDS,"threshold":evaluation.threshold,"serial":args["serial"].as_bool().unwrap_or(false),
            "start":{"gate_revision":document.revision,"score":evaluation.score,"status":evaluation.status},
            "context":context,"context_sha256":digest(context.as_bytes()),
            "rounds":[],"last":null,"error":null,"awaiting_press":false});
        self.start_round(
            &mut call,
            &project,
            &branch,
            &gate,
            &document,
            &evaluation,
            &questions,
        )?;
        record["calls"]
            .as_array_mut()
            .ok_or_else(|| invalid("Malformed revise record"))?
            .push(call.clone());
        self.save_revise_record(&record)?;
        Ok(json!({"call":call,"running":true,"max_rounds":MAX_ROUNDS}))
    }

    /// Explicit recovery only. Preserve succeeded job/document identities and the
    /// same call/round limit; cancellation, conflicts and other partial output are fences.
    /// A writer the provider stopped at its output limit is issued again at the route's
    /// current limit once that limit has risen; its partial draft stays as a kept revision,
    /// never the document the round reads.
    fn resume_gate_call(&mut self, record: &mut Value) -> Result<Value> {
        self.validate_pipeline_budget()?;
        let limit =
            generation_max_tokens(&self.root, std::env::var("QROC_MAX_TOKENS").ok().as_deref())?;
        let mut call = record["calls"]
            .as_array()
            .and_then(|c| c.last())
            .cloned()
            .ok_or_else(|| invalid("Revise call missing"))?;
        let round_index = call["rounds"]
            .as_array()
            .map_or(0, Vec::len)
            .checked_sub(1)
            .ok_or_else(|| invalid("Revise round missing"))?;
        let round = &call["rounds"][round_index];
        let mut ids = round["documents"]
            .as_array()
            .ok_or_else(|| invalid("Revise documents missing"))?
            .iter()
            .map(|d| s(d, "job_id").to_owned())
            .collect::<Vec<_>>();
        ids.push(s(round, "gate_job_id").to_owned());
        let siblings = ids
            .iter()
            .filter_map(|id| self.jobs.get(id))
            .map(|job| (job.document_type.clone(), job.id.clone()))
            .collect::<BTreeMap<_, _>>();
        // Validate the entire checkpoint before mutating any job.
        for id in &ids {
            let job = self
                .jobs
                .get(id)
                .ok_or_else(|| invalid("Revise job missing"))?;
            if job.state == "succeeded" {
                let document = self.semantic_current_document(
                    &job.project_id,
                    &job.branch_id,
                    &job.document_type,
                )?;
                if digest(document.content.as_bytes()) != document.sha256
                    || job.document_id.as_deref() != Some(document.id.as_str())
                    || job.output_sha256.as_deref() != Some(document.sha256.as_str())
                {
                    return Err(invalid(
                        "Completed revise checkpoint changed; inspect retained document",
                    ));
                }
            } else if job.state == "partial"
                && job.generation_finish_reason.as_deref() == Some("length")
            {
                // The real limit is the one the route honoured, which may be less than asked.
                let real = honoured_limit(&serde_json::to_value(job)?);
                if u64::from(limit) <= real {
                    return Err(invalid(format!(
                        "{} stopped at its output limit of {real} and the route's limit is still {limit}; raise the route's limit before resuming",
                        job.document_type
                    )));
                }
                // One re-issue per real limit: asking the same limit again after an earlier
                // attempt asked it and was held to the same real limit would only repeat it.
                if job.max_tokens == limit
                    && job.attempts.iter().any(|a| {
                        a["state"] == "partial"
                            && a["generation_finish_reason"] == "length"
                            && a["max_tokens"].as_u64() == Some(u64::from(limit))
                            && honoured_limit(a) == real
                    })
                {
                    return Err(invalid(format!(
                        "{} was already issued again and stopped again at the route's real limit of {real}; the route honours no more than that",
                        job.document_type
                    )));
                }
            } else if !(matches!(job.state.as_str(), "running" | "queued" | "interrupted")
                || (job.state == "blocked"
                    && job
                        .error
                        .as_deref()
                        .is_some_and(|e| e.starts_with("Cascade stopped:") || e == "Batch stopped: required document or gate did not complete successfully"))
                || (job.state == "failed"
                    && job.error.as_deref().is_some_and(|e| {
                        e.contains("timed out")
                            || e.contains("Connection")
                            || e.contains("connection")
                            || e.contains("deadline")
                            || e.contains("closed before") || e == "Gateway returned unsuccessful HTTP status"
                    })))
            {
                return Err(invalid(format!(
                    "Revise checkpoint {} requires inspection: {}",
                    id, job.state
                )));
            }
        }
        let mut previous = None;
        for id in ids {
            let mut job = self
                .jobs
                .get(&id)
                .cloned()
                .ok_or_else(|| invalid("Revise job disappeared"))?;
            if !matches!(job.state.as_str(), "succeeded" | "running" | "queued") {
                let mut attempt = serde_json::to_value(&job)?;
                attempt
                    .as_object_mut()
                    .ok_or_else(|| invalid("Malformed job attempt"))?
                    .remove("attempts");
                job.attempts.push(attempt);
                job.state = "queued".into();
                // Every re-issued job asks for the route's limit as it stands now.
                job.max_tokens = limit;
                job.honoured_max_tokens = None;
                job.generation_finish_reason = None;
                job.output_sha256 = None;
                job.document_id = None;
                job.after_job = if job.batch_id.is_some() {
                    None
                } else {
                    previous.clone()
                };
                if call["serial"] != true && job.document_type == "VALIDATION_PLAN" {
                    for kind in ["PRE_MORTEM", "TECHNICAL_ARCHITECTURE"] {
                        if let Some(dependency) = siblings.get(kind)
                            && !job.dependency_jobs.contains(dependency)
                        {
                            job.dependency_jobs.push(dependency.clone());
                        }
                    }
                }
                job.error = None;
                job.finished_at = None;
                job.timeout_seconds = self.generation_wait_seconds()?;
                self.persist_job(&job)?;
                self.jobs.insert(id.clone(), job);
            }
            previous = Some(id);
        }
        call["state"] = json!("running");
        call["error"] = Value::Null;
        call["finished_at"] = Value::Null;
        call["rounds"][round_index]["status"] = json!("running");
        call["rounds"][round_index]["reason"] = Value::Null;
        *record["calls"]
            .as_array_mut()
            .and_then(|c| c.last_mut())
            .ok_or_else(|| invalid("Revise call disappeared"))? = call.clone();
        self.save_revise_record(record)?;
        let first = call["rounds"][round_index]["documents"]
            .as_array()
            .and_then(|d| d.first())
            .map(|d| s(d, "job_id").to_owned());
        if let Some(id) = first
            && self.jobs.get(&id).is_some_and(|j| {
                j.state == "queued" && j.after_job.is_none() && j.batch_id.is_none()
            })
        {
            self.launch_job(&id)?;
        }
        Ok(json!({"call":call,"running":true,"resumed":true,"max_rounds":MAX_ROUNDS}))
    }

    /// Queue the round's writers as one chain, then its gate.
    /// Each call is one press of his; nothing else starts a round.
    #[allow(clippy::too_many_arguments)]
    fn start_round(
        &mut self,
        call: &mut Value,
        project: &str,
        branch: &str,
        gate: &str,
        answered: &DocumentRevision,
        evaluation: &GateResult,
        questions: &[Value],
    ) -> Result<()> {
        let round = call["rounds"].as_array().map_or(0, Vec::len) as u64 + 1;
        let joined = crate::registry::product_inputs(gate).unwrap_or(&[]);
        let open = questions
            .iter()
            .filter(|q| q["standing"] == "open")
            .cloned()
            .collect::<Vec<_>>();
        let context = bounded_text(s(call, "context"), MAX_BODY, "Revise context")?;
        let criterion_documents = criterion_documents(gate, joined);
        let (revised, held) = round_plan(
            joined,
            evaluation,
            &open,
            &criterion_documents,
            !context.trim().is_empty(),
        )?;
        let revised = chain_order(&revised);
        let feedback = feedback_text(answered, evaluation, questions);
        if call.get("context_sha256").is_some()
            && call["context_sha256"] != digest(context.as_bytes())
        {
            return Err(invalid("Revise supplementary input digest changed"));
        }
        // Persist the round's starting revisions for independent workers. Dependent
        // siblings are rebound at consumption after their jobs have committed.
        let mut baseline = BTreeMap::new();
        let mut baseline_bytes = "\n\nPersisted baseline for this revise round. These are existing documents, not new requirements. Reconcile the gate feedback with this shared baseline; retain requirements and distinguish unsettled proposals.\n".len();
        for kind in ["PRD", "TECHNICAL_ARCHITECTURE"].into_iter().chain(
            crate::registry::product_inputs(gate)
                .unwrap_or(&[])
                .iter()
                .copied(),
        ) {
            if kind == "IDEA" || baseline.contains_key(kind) {
                continue;
            }
            match self.semantic_current_document(project, branch, kind) {
                Ok(mut document) => {
                    // A baseline is content and identity, never a recursive copy of
                    // earlier generation provenance. Those receipts stay on disk.
                    document.generation_inputs = None;
                    baseline_bytes = baseline_bytes.saturating_add(
                        format!(
                            "\nBaseline {kind} revision {} id {} sha256 {}\n{}\n",
                            document.revision, document.id, document.sha256, document.content
                        )
                        .len(),
                    );
                    if baseline_bytes > MAX_BODY / 2 {
                        return Err(invalid(
                            "Revise baseline exceeds bounded context; documents retained",
                        ));
                    }
                    baseline.insert(kind.to_owned(), document);
                }
                Err(_)
                    if !crate::registry::product_inputs(gate)
                        .unwrap_or(&[])
                        .contains(&kind) => {}
                Err(error) => return Err(error),
            }
        }
        let marker = json!({"gate":gate,"call":call["call"],"round":round,"of":MAX_ROUNDS,
            "answers_gate_revision":answered.revision,"answers_gate_id":answered.id,"baseline":baseline});
        let mut documents = Vec::new();
        let batch_id = self.new_id("revise-batch");
        let mut ids = BTreeMap::new();
        for (position, kind) in revised.iter().enumerate() {
            let mut tag = marker.clone();
            tag["feedback"] = json!(format!(
                "{feedback}{}",
                repair_text(kind, &revised[..position], evaluation)
            ));
            let owned = owned_repairs(kind, evaluation);
            tag["repairs"] = json!(owned);
            let job = self.generate_start_after(
                &json!({"project_id":project,"branch_id":branch,"document_type":kind,"context":context,"gate_revise":tag}),
                Some(batch_id.clone()),
            )?;
            ids.insert(kind.to_string(), s(&job, "id").to_owned());
            documents.push(
                json!({"document_type":kind,"job_id":job["id"],"revision":null,"repairs":owned}),
            );
        }
        let job = self.generate_start_after(
            &json!({"project_id":project,"branch_id":branch,"document_type":gate,"context":context,"gate_revise":marker}),
            Some(batch_id.clone()),
        )?;
        ids.insert(gate.to_owned(), s(&job, "id").to_owned());
        let mut previous: Option<String> = None;
        for kind in revised.iter().copied().chain(std::iter::once(gate)) {
            let id = ids
                .get(kind)
                .ok_or_else(|| invalid("Revise job identity missing"))?;
            let mut queued = self
                .jobs
                .get(id)
                .cloned()
                .ok_or_else(|| invalid("Revise job missing"))?;
            queued.batch_id = Some(batch_id.clone());
            // One chain: each writer starts after the previous one commits, so every
            // writer of the round reads its siblings' current revisions, this round's
            // included. Parallel chains rewrote each side of a conflict blind to the other.
            queued.batch_width = 1;
            queued.after_job = None;
            queued.dependency_jobs = previous.clone().into_iter().collect();
            self.persist_job(&queued)?;
            self.jobs.insert(id.clone(), queued);
            previous = Some(id.clone());
        }
        call["rounds"]
            .as_array_mut()
            .ok_or_else(|| invalid("Malformed revise call"))?
            .push(
                json!({"round":round,"answers_gate_revision":answered.revision,"feedback":feedback,
                "documents":documents,"held_documents":held,"gate_job_id":job["id"],"gate_revision":null,"score":null,
                "status":"running","reason":null,"pressed_at":now()}),
            );
        Ok(())
    }

    /// Called at the end of every poll: settles finished rounds and starts the next one.
    pub(super) fn advance_gate_revisions(&mut self) -> Result<()> {
        let dir = self.root.join("gate-revisions");
        if !dir.exists() {
            return Ok(());
        }
        let mut paths = Vec::new();
        for project in fs::read_dir(&dir)? {
            for branch in fs::read_dir(project?.path())? {
                for file in fs::read_dir(branch?.path())? {
                    paths.push(file?.path());
                }
            }
        }
        for path in paths {
            let mut record: Value = read_json(&path)?;
            let Some(call) = record["calls"].as_array_mut().and_then(|c| c.last_mut()) else {
                continue;
            };
            if call["state"] != "running" {
                continue;
            }
            let mut call = call.clone();
            let before = call.clone();
            if let Some(round) = call["rounds"].as_array_mut().and_then(|r| r.last_mut()) {
                for doc in round["documents"].as_array_mut().into_iter().flatten() {
                    if let Some(job) = self.jobs.get(s(doc, "job_id")) {
                        doc["state"] = json!(job.state);
                        if job.state == "succeeded" {
                            let current = self.semantic_current_document(
                                &job.project_id,
                                &job.branch_id,
                                &job.document_type,
                            )?;
                            doc["revision"] = json!(current.revision);
                            doc["sha256"] = json!(current.sha256);
                        }
                    }
                }
            }
            if self.advance_call(&record, &mut call)? || call != before {
                *record["calls"]
                    .as_array_mut()
                    .and_then(|c| c.last_mut())
                    .expect("call exists") = call;
                if let Some(evaluation) = self.settled_gate(&record)? {
                    reconcile_questions(&mut record, &evaluation.1, evaluation.0);
                }
                self.save_revise_record(&record)?;
            }
        }
        Ok(())
    }

    fn advance_call(&mut self, record: &Value, call: &mut Value) -> Result<bool> {
        let (project, branch, gate) = (
            s(record, "project_id").to_owned(),
            s(record, "branch_id").to_owned(),
            s(record, "gate").to_owned(),
        );
        let index = call["rounds"].as_array().map_or(0, Vec::len);
        if index == 0 {
            return Ok(false);
        }
        let round = &call["rounds"][index - 1];
        let Some(gate_job) = self.jobs.get(s(round, "gate_job_id")).cloned() else {
            return Ok(false);
        };
        if !terminal(&gate_job.state) {
            return Ok(false);
        }
        let mut settled = round.clone();
        for doc in settled["documents"].as_array_mut().into_iter().flatten() {
            if let Some(job) = self.jobs.get(s(doc, "job_id")) {
                doc["state"] = json!(job.state);
                if let Ok(current) =
                    self.semantic_current_document(&project, &branch, &job.document_type)
                    && job.document_id.as_deref() == Some(current.id.as_str())
                {
                    doc["revision"] = json!(current.revision);
                }
            }
        }
        let stopped = settled["documents"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["state"] != "succeeded")
            .map(|d| (s(d, "document_type").to_owned(), s(d, "job_id").to_owned()))
            .or_else(|| {
                (gate_job.state != "succeeded").then(|| (gate.clone(), gate_job.id.clone()))
            });
        if let Some((kind, job_id)) = stopped {
            let job = self.jobs.get(&job_id);
            settled["status"] = json!("blocked");
            settled["reason"] = json!(format!(
                "{kind} did not regenerate ({}): {}",
                job.map_or("missing", |j| j.state.as_str()),
                job.and_then(|j| j.error.clone()).unwrap_or_default()
            ));
            call["state"] = json!("blocked");
            call["error"] = settled["reason"].clone();
        } else {
            let document = self.semantic_current_document(&project, &branch, &gate)?;
            if gate_job.document_id.as_deref() != Some(document.id.as_str()) {
                return Err(invalid(
                    "The regenerated gate is not the branch's current gate",
                ));
            }
            let evaluation = self.evaluate_owned_gate(&document);
            settled["gate_revision"] = json!(document.revision);
            settled["score"] = json!(evaluation.score);
            settled["status"] = json!(evaluation.status);
            settled["reason"] = json!(evaluation.reason);
            call["last"] = json!({"gate_revision":document.revision,"score":evaluation.score,
                "status":evaluation.status,"threshold":evaluation.threshold,
                "reason":evaluation.reason,"criteria":evaluation.criteria,
                "questions":evaluation.questions,"questions_note":evaluation.questions_note});
            // The next round is his press, never this poll's (design record entry 42).
            // A failed round with rounds left waits for him; an exhausted call stays failed.
            call["awaiting_press"] =
                json!(evaluation.status == "failed" && (index as u64) < MAX_ROUNDS);
            call["state"] = json!(match evaluation.status.as_str() {
                "passed" => "passed",
                "failed" => "failed",
                _ => "blocked",
            });
        }
        call["rounds"][index - 1] = settled;
        if call["state"] != "running" {
            call["finished_at"] = json!(now());
        }
        Ok(true)
    }

    /// The gate revision and verdict the latest call settled on, if it settled on one.
    fn settled_gate(&self, record: &Value) -> Result<Option<(u64, GateResult)>> {
        let Some(revision) = record["calls"]
            .as_array()
            .and_then(|c| c.last())
            .and_then(|c| c["last"]["gate_revision"].as_u64())
        else {
            return Ok(None);
        };
        let document = self.semantic_current_document(
            s(record, "project_id"),
            s(record, "branch_id"),
            s(record, "gate"),
        )?;
        if u64::from(document.revision) != revision {
            return Ok(None);
        }
        let evaluation = self.evaluate_owned_gate(&document);
        Ok(Some((revision, evaluation)))
    }

    /// The round and state of the latest revise call touching this node: the gate itself or
    /// a document it joins. `None` when the node was never revised from a gate.
    pub(super) fn graph_revise(&self, project: &str, branch: &str, kind: &str) -> Result<Value> {
        let gate = if kind.starts_with("GATE_") {
            Some(kind)
        } else {
            ["GATE_1", "GATE_2", "GATE_3"].into_iter().find(|g| {
                crate::registry::product_inputs(g).is_some_and(|inputs| inputs.contains(&kind))
            })
        };
        let Some(gate) = gate else {
            return Ok(Value::Null);
        };
        let path = self.revise_path(project, branch, gate)?;
        if !path.exists() {
            return Ok(Value::Null);
        }
        let record: Value = read_json(&path)?;
        let Some(call) = record["calls"].as_array().and_then(|c| c.last()) else {
            return Ok(Value::Null);
        };
        let round = call["rounds"].as_array().map_or(0, Vec::len);
        Ok(
            json!({"gate":gate,"call":call["call"],"round":round,"of":MAX_ROUNDS,
            "label":format!("round {round} of {MAX_ROUNDS}"),"state":call["state"],
            "score":call["last"]["score"],"threshold":call["threshold"]}),
        )
    }
}

/// Questions of the record still waiting for his answer.
fn open_questions(record: &Value) -> Vec<Value> {
    record["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|q| q["standing"] == "open")
        .cloned()
        .collect()
}

/// Adds the verdict's questions the record does not hold yet, each open until he settles it.
/// A question he already answered or withdrew is never reopened by a later verdict.
fn merge_questions(record: &mut Value, evaluation: &GateResult, gate_revision: u64) -> bool {
    if evaluation.questions.is_empty() {
        return false;
    }
    if !record["questions"].is_array() {
        record["questions"] = json!([]);
    }
    let list = record["questions"].as_array_mut().expect("questions list");
    let mut added = false;
    for q in &evaluation.questions {
        if list.iter().any(|known| {
            known["criterion"] == q.criterion.as_str()
                && known["document"] == q.document.as_str()
                && known["question"] == q.question.as_str()
        }) {
            continue;
        }
        let id = format!("question-{}", list.len() + 1);
        list.push(json!({"id":id,"criterion":q.criterion,"document":q.document,"question":q.question,
            "standing":"open","raised_by_gate_revision":gate_revision,"answer":null,"settled_at":null}));
        added = true;
    }
    added
}

/// Takes in a verdict's questions, then retires the earlier wordings it asks again.
fn reconcile_questions(record: &mut Value, evaluation: &GateResult, gate_revision: u64) -> bool {
    let added = merge_questions(record, evaluation, gate_revision);
    supersede_questions(record, evaluation, gate_revision) || added
}

/// An open question raised by an earlier gate revision, and not asked by this verdict, is
/// superseded by the verdict's question that asks the same thing in other words: the one
/// sharing most of its words, clearly ahead of any other. A question with no such match
/// stays open; its text is never changed.
fn supersede_questions(record: &mut Value, evaluation: &GateResult, gate_revision: u64) -> bool {
    fn words(text: &str) -> std::collections::BTreeSet<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    }
    fn overlap(
        a: &std::collections::BTreeSet<String>,
        b: &std::collections::BTreeSet<String>,
    ) -> f64 {
        let union = a.union(b).count();
        if union == 0 {
            0.0
        } else {
            a.intersection(b).count() as f64 / union as f64
        }
    }
    let Some(list) = record["questions"].as_array_mut() else {
        return false;
    };
    let current = evaluation
        .questions
        .iter()
        .filter_map(|q| {
            list.iter().find(|known| {
                known["criterion"] == q.criterion.as_str()
                    && known["document"] == q.document.as_str()
                    && known["question"] == q.question.as_str()
            })
        })
        .map(|known| (s(known, "id").to_owned(), words(s(known, "question"))))
        .collect::<Vec<_>>();
    if current.is_empty() {
        return false;
    }
    let mut changed = false;
    for q in list.iter_mut() {
        let earlier = q["raised_by_gate_revision"]
            .as_u64()
            .is_some_and(|r| r < gate_revision);
        if q["standing"] != "open"
            || !earlier
            || current.iter().any(|(id, _)| q["id"] == id.as_str())
        {
            continue;
        }
        let own = words(s(q, "question"));
        let mut scored = current
            .iter()
            .map(|(id, w)| (overlap(&own, w), id))
            .collect::<Vec<_>>();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let (best, by) = scored[0];
        let runner_up = scored.get(1).map_or(0.0, |s| s.0);
        if best >= SAME_QUESTION_OVERLAP && runner_up <= best / 2.0 {
            q["standing"] = json!("superseded");
            q["superseded_by"] = json!(by);
            q["superseded_by_gate_revision"] = json!(gate_revision);
            q["settled_at"] = json!(now());
            changed = true;
        }
    }
    changed
}

/// Which joined documents a round regenerates, and which it holds for his answer.
/// Without open questions every joined document is revised, as before. With them, a round
/// revises the documents the gate named for content repair (or, when it named none, those
/// not waiting on a question), and is refused when every remaining shortfall is his question.
/// The joined documents each rubric criterion judges, by criterion name in lower case: the
/// document its source names, or every joined document for a criterion across them all.
fn criterion_documents(gate: &str, joined: &[&str]) -> BTreeMap<String, Vec<String>> {
    let Ok(Some(rubric)) = crate::gate_rubric::rubric(gate) else {
        return BTreeMap::new();
    };
    rubric
        .criteria
        .iter()
        .map(|c| {
            let named = joined
                .iter()
                .filter(|k| **k != "IDEA" && c.source.contains(**k))
                .map(|k| k.to_string())
                .collect::<Vec<_>>();
            let documents = if named.is_empty() {
                joined
                    .iter()
                    .filter(|k| **k != "IDEA")
                    .map(|k| k.to_string())
                    .collect()
            } else {
                named
            };
            (c.name.to_ascii_lowercase(), documents)
        })
        .collect()
}

fn round_plan<'a>(
    joined: &[&'a str],
    evaluation: &GateResult,
    questions: &[Value],
    criterion_documents: &BTreeMap<String, Vec<String>>,
    supplementary: bool,
) -> Result<(Vec<&'a str>, Vec<Value>)> {
    let joined = joined
        .iter()
        .copied()
        .filter(|k| *k != "IDEA")
        .collect::<Vec<_>>();
    if questions.is_empty() {
        return Ok((joined, Vec::new()));
    }
    let held_criteria = questions
        .iter()
        .filter_map(|q| q["criterion"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let question_documents = questions
        .iter()
        .filter_map(|q| q["document"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    // A repair line is the gate's own word that a writer can fix it without him.
    let repaired = evaluation
        .repairs
        .iter()
        .map(|r| r.document.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let fixable = !repaired.is_empty()
        || evaluation
            .criteria
            .iter()
            .any(|(name, c)| c.score < c.max && !held_criteria.contains(name.as_str()));
    // Every document the verdict faults is revised: those its repair lines name, and those a
    // short criterion judges that no open question holds. A round carrying new supplementary
    // input revises every document not waiting on a question, since the input reaches them all.
    let faulted = evaluation
        .criteria
        .iter()
        .filter(|(name, c)| c.score < c.max && !held_criteria.contains(name.as_str()))
        .flat_map(|(name, _)| criterion_documents.get(name).into_iter().flatten())
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let revised = joined
        .iter()
        .copied()
        .filter(|k| {
            repaired.contains(k)
                || (!question_documents.contains(k)
                    && (supplementary || repaired.is_empty() || faulted.contains(k)))
        })
        .collect::<Vec<_>>();
    if !fixable || revised.is_empty() {
        let list = questions
            .iter()
            .map(|q| format!("{} (blocks {})", s(q, "question"), s(q, "criterion")))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(invalid(format!(
            "No round was started: every remaining shortfall waits on {} only the person can answer: {list}",
            if questions.len() == 1 {
                "a question"
            } else {
                "questions"
            }
        )));
    }
    let held = joined
        .iter()
        .copied()
        .filter(|k| !revised.contains(k) && question_documents.contains(k))
        .map(|k| {
            let ids = questions
                .iter()
                .filter(|q| q["document"] == *k)
                .map(|q| q["id"].clone())
                .collect::<Vec<_>>();
            json!({"document_type":k,"questions":ids})
        })
        .collect();
    Ok((revised, held))
}

/// Inputs a revise writer must follow within its round: its registry inputs, and for
/// validation the risk and architecture documents it reconciles.
fn round_inputs(kind: &str) -> Vec<&'static str> {
    let mut inputs = crate::registry::product_inputs(kind)
        .unwrap_or(&[])
        .to_vec();
    if kind == "VALIDATION_PLAN" {
        inputs.extend(["PRE_MORTEM", "TECHNICAL_ARCHITECTURE"]);
    }
    inputs
}

/// The round's writers in one order: a document comes after every revised document it
/// reads, otherwise in the gate's own join order.
fn chain_order<'a>(revised: &[&'a str]) -> Vec<&'a str> {
    let mut remaining = revised.to_vec();
    let mut order = Vec::new();
    while !remaining.is_empty() {
        let next = remaining
            .iter()
            .position(|k| {
                round_inputs(k)
                    .iter()
                    .all(|input| !remaining.contains(input) || input == k)
            })
            .unwrap_or(0);
        order.push(remaining.remove(next));
    }
    order
}

fn owned_repairs(kind: &str, evaluation: &GateResult) -> Vec<Value> {
    evaluation
        .repairs
        .iter()
        .filter(|r| r.document == kind)
        .map(|r| json!({"criterion":r.criterion,"repair":r.repair}))
        .collect()
}

/// The gate's repair lines as each writer needs them: the ones it owns, and the ones its
/// siblings own, with who has already written in this round.
fn repair_text(kind: &str, written: &[&str], evaluation: &GateResult) -> String {
    let mut text = String::new();
    let (own, others): (Vec<_>, Vec<_>) =
        evaluation.repairs.iter().partition(|r| r.document == kind);
    if !own.is_empty() {
        text.push_str(&format!(
            "Repairs the gate assigned to {kind}. This document owns them; make each one here, agreeing with the current sibling revisions supplied below:\n"
        ));
        for r in own {
            text.push_str(&format!("- {}: {}\n", r.criterion, r.repair));
        }
    }
    if !others.is_empty() {
        text.push_str("Repairs the gate assigned to other documents. Their owners make them; do not move this document's side of these to meet them unless the line asks it of this document:\n");
        for r in others {
            let when = if written.contains(&r.document.as_str()) {
                "revised earlier in this round; its current revision is supplied"
            } else {
                "revised later in this round"
            };
            text.push_str(&format!(
                "- {} ({}, {when}): {}\n",
                r.document, r.criterion, r.repair
            ));
        }
    }
    text
}

fn feedback_text(gate: &DocumentRevision, evaluation: &GateResult, questions: &[Value]) -> String {
    let score = evaluation
        .score
        .map_or("no readable score".into(), |v| format!("{v}/100"));
    let threshold = evaluation
        .threshold
        .map_or("its bar".into(), |v| format!("{v}/100"));
    let mut text = format!(
        "{} revision {} did not pass: {score}, needs {threshold}. Revise this document so it answers the gate's criterion feedback below; keep what the gate did not fault.\n",
        gate.document_type, gate.revision
    );
    if evaluation.criteria.is_empty() {
        let mut end = gate.content.len().min(MAX_FEEDBACK_GATE_TEXT);
        while !gate.content.is_char_boundary(end) {
            end -= 1;
        }
        text.push_str(&format!(
            "The gate's criteria could not be read ({}). The gate's review follows.\n{}",
            evaluation.reason,
            &gate.content[..end]
        ));
    } else {
        for (name, c) in &evaluation.criteria {
            text.push_str(&format!(
                "- {name}: {}/{} - {}\n",
                c.score, c.max, c.feedback
            ));
        }
    }
    let open = questions.iter().filter(|q| q["standing"] == "open");
    let answered = questions.iter().filter(|q| q["standing"] == "answered");
    if questions.iter().any(|q| q["standing"] == "open") {
        text.push_str("Questions only the project's owner can answer. They stay open: never answer, assume or propose an answer to them as his.\n");
        for q in open {
            text.push_str(&format!(
                "- {} (holds back {}, from {})\n",
                s(q, "question"),
                s(q, "criterion"),
                s(q, "document")
            ));
        }
    }
    if questions.iter().any(|q| q["standing"] == "answered") {
        text.push_str("The owner's own answers to earlier gate questions (his decisions; cite them as source-stated):\n");
        for q in answered {
            text.push_str(&format!(
                "- {} Answer: {}\n",
                s(q, "question"),
                s(q, "answer")
            ));
        }
    }
    text
}

#[cfg(test)]
mod round_plan_tests {
    use super::*;

    const JOINED: [&str; 5] = [
        "STRATEGY_BRIEF",
        "PRE_MORTEM",
        "PRD",
        "VALIDATION_PLAN",
        "TECHNICAL_ARCHITECTURE",
    ];

    /// The Gate 1 verdict a round started from on 30 Oct (Zavent Sheets, revision 3, 87): six
    /// repair lines, none naming the pre-mortem, and consistency across all five short by one.
    fn verdict(consistency: u32) -> GateResult {
        let criteria = [
            ("problem and users", 13, 15),
            ("requirement testability", 21, 25),
            ("risk coverage", 15, 15),
            ("validation readiness", 16, 20),
            ("architecture fit", 13, 15),
            ("cross-document consistency", consistency, 10),
        ]
        .into_iter()
        .map(|(name, score, max)| {
            (
                name.to_owned(),
                CriterionResult {
                    score,
                    max,
                    feedback: "fixture".into(),
                },
            )
        })
        .collect();
        let repair = |document: &str, criterion: &str| GateRepair {
            document: document.into(),
            criterion: criterion.into(),
            repair: "fixture repair".into(),
        };
        GateResult {
            document_type: "GATE_1".into(),
            status: "failed".into(),
            score: Some(87),
            criteria,
            threshold: Some(90),
            reason: "fixture".into(),
            evaluated_at: 0,
            semantic: None,
            questions: Vec::new(),
            repairs: vec![
                repair("VALIDATION_PLAN", "validation readiness"),
                repair("PRD", "requirement testability"),
                repair("VALIDATION_PLAN", "validation readiness"),
                repair("TECHNICAL_ARCHITECTURE", "architecture fit"),
                repair("STRATEGY_BRIEF", "problem and users"),
                repair("VALIDATION_PLAN", "cross-document consistency"),
            ],
            questions_note: None,
        }
    }

    fn open_questions() -> Vec<Value> {
        vec![
            json!({"id":"question-11","criterion":"validation readiness","document":"VALIDATION_PLAN","standing":"open","question":"q"}),
            json!({"id":"question-17","criterion":"problem and users","document":"PRD","standing":"open","question":"q"}),
        ]
    }

    fn plan(consistency: u32, supplementary: bool) -> Vec<&'static str> {
        let documents = criterion_documents("GATE_1", &JOINED);
        assert_eq!(
            documents["cross-document consistency"].len(),
            5,
            "consistency judges all five"
        );
        assert_eq!(documents["risk coverage"], ["PRE_MORTEM"]);
        round_plan(
            &JOINED,
            &verdict(consistency),
            &open_questions(),
            &documents,
            supplementary,
        )
        .unwrap()
        .0
    }

    #[test]
    fn a_document_the_verdict_faults_outside_its_repair_lines_is_revised() {
        // Before: the repair-named four only, and the pre-mortem kept as it was.
        assert_eq!(
            plan(9, false),
            [
                "STRATEGY_BRIEF",
                "PRE_MORTEM",
                "PRD",
                "VALIDATION_PLAN",
                "TECHNICAL_ARCHITECTURE"
            ]
        );
    }

    #[test]
    fn new_supplementary_input_reaches_every_document_not_held() {
        assert_eq!(plan(10, true).len(), 5);
        assert!(plan(10, true).contains(&"PRE_MORTEM"));
    }

    #[test]
    fn a_document_nothing_faults_is_not_spent_on() {
        assert!(!plan(10, false).contains(&"PRE_MORTEM"));
        assert_eq!(plan(10, false).len(), 4);
    }
}
