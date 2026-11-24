//! Readable projection only: immutable revision records remain authoritative.
use crate::qroc::{DocumentRevision, Project, normalized_document};
use crate::{Error, Result};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

fn name(value: &str) -> Result<&str> {
    if value.trim().is_empty()
        || value.len() > 180
        || value == "."
        || value == ".."
        || value.starts_with('.')
        || value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
    {
        return Err(Error::Invalid(
            "Folder names must be readable, bounded names without separators".into(),
        ));
    }
    Ok(value)
}
fn title(kind: &str) -> String {
    kind.split('_')
        .map(|word| {
            if matches!(word, "PRD" | "PR" | "ADS") {
                return word.to_owned();
            }
            let mut chars = word.chars();
            chars
                .next()
                .map(|c| format!("{c}{}", chars.as_str().to_ascii_lowercase()))
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn write(root: &Path, relative: &str, content: &str) -> Result<()> {
    let path = root.join(relative);
    crate::scope::validate_root(&path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

/// The domain-to-folder map, written at the top of every project folder.
fn domains_text() -> String {
    let mut text = "# Domains and their folders\n\nEach domain's documents are kept in its own folder. Earlier revisions are in the folder's `revisions/`, gate reviews in its `gates/`.\n\n| Domain | Folder | Purpose |\n|---|---|---|\n".to_owned();
    for domain in crate::registry::DOMAIN_ORDER {
        text.push_str(&format!(
            "| {} | `{}/` | {} |\n",
            title(domain),
            crate::registry::domain_folder(domain),
            crate::registry::domain_purpose(domain)
        ));
    }
    text.push_str("\n`idea/` holds the idea and its hardening; `work-table/` the work table; `media/` the project's media.\n");
    text
}

pub(crate) fn materialize(
    root: &Path,
    project: &Project,
    records: &[(String, DocumentRevision)],
    extra: &[(String, String)],
) -> Result<PathBuf> {
    let base = root.join("project-folders");
    let folder = base.join(name(&project.title)?);
    crate::scope::validate_root(&folder)?;
    fs::create_dir_all(&base)?;
    // Owner markers prevent two projects with identical names from overwriting one another.
    let owner = folder.join(".qroc-owner");
    crate::scope::validate_root(&owner)?;
    if folder.exists() && (!owner.is_file() || fs::read_to_string(&owner)? != project.id) {
        return Err(Error::Invalid(
            "Project folder name is already owned; choose a distinct project name".into(),
        ));
    }
    let mut groups: BTreeMap<(String, String, String), Vec<&DocumentRevision>> = BTreeMap::new();
    for (branch, doc) in records {
        normalized_document(&doc.document_type)?;
        groups
            .entry((
                doc.branch_id.clone(),
                branch.clone(),
                doc.document_type.clone(),
            ))
            .or_default()
            .push(doc);
    }
    let mut files = BTreeMap::new();
    for ((branch_id, branch, kind), mut docs) in groups {
        docs.sort_by_key(|doc| doc.revision);
        let doc_title = title(&kind);
        let multiple = records
            .iter()
            .any(|(_, d)| d.branch_id != branch_id && d.document_type == kind);
        let filename = if multiple {
            format!("{doc_title} ({})", name(&branch)?)
        } else {
            doc_title
        };
        // Every document sits in the folder of its own domain, so each domain traces to one folder.
        let domain = crate::registry::domain_folder(crate::registry::document_domain(&kind));
        let subdir = if kind.starts_with("GATE_") {
            "gates/"
        } else if kind == "CONTRACTS" {
            "contracts/"
        } else {
            ""
        };
        let last = docs.len().saturating_sub(1);
        for (index, doc) in docs.into_iter().enumerate() {
            let path = if index == last {
                format!("{domain}/{subdir}{filename}.md")
            } else {
                format!(
                    "{domain}/revisions/{filename} - revision {}.md",
                    doc.revision
                )
            };
            if files.insert(path, doc.content.clone()).is_some() {
                return Err(Error::Invalid(
                    "Readable document names collide; choose distinct branch names".into(),
                ));
            }
        }
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Invalid("System clock precedes epoch".into()))?
        .as_nanos();
    let stage = base.join(format!(".stage-{}-{stamp}", project.id));
    crate::scope::validate_root(&stage)?;
    fs::create_dir(&stage)?;
    let folders = crate::registry::DOMAIN_ORDER
        .iter()
        .map(|d| crate::registry::domain_folder(d));
    for domain in ["idea"]
        .into_iter()
        .chain(folders)
        .chain(["work-table", "media"])
    {
        fs::create_dir(stage.join(domain))?;
    }
    for domain in [
        "product/gates",
        "product/contracts",
        "product/specs",
        "business/advantages",
    ] {
        fs::create_dir_all(stage.join(domain))?;
    }
    write(&stage, ".qroc-owner", &project.id)?;
    write(&stage, "Domains.md", &domains_text())?;
    write(&stage, "idea/Idea.md", &project.brain_dump)?;
    // Records kept beside the revision store: the hardening rounds and the Product tail.
    // media/ may hold the person's own files (it is the default media store): its readable texts
    // are written into the carried-forward folder at the swap, never over a file of theirs.
    let mut media = BTreeMap::new();
    for (path, content) in extra {
        if let Some(rest) = path.strip_prefix("media/") {
            media.insert(rest.to_owned(), content.clone());
            continue;
        }
        if files.insert(path.clone(), content.clone()).is_some() {
            return Err(Error::Invalid(
                "Readable document names collide; choose distinct names".into(),
            ));
        }
    }
    for (path, content) in files {
        write(&stage, &path, &content)?;
    }
    let carried = folder.join("media");
    if carried.is_dir() {
        let written = written_media(&carried)?;
        for name in media.keys() {
            let target = carried.join(name);
            if fs::symlink_metadata(&target).is_ok() && !written.contains(name) {
                return Err(Error::Invalid(
                    "The media folder holds a file of the same name as a project text; rename it first".into(),
                ));
            }
        }
    }
    // Keep previous projections instead of deleting user-visible files or historical bytes.
    // The folder itself stays; each Q-ROC entry is swapped on its own. media/ may be the person's
    // media store, so it is never renamed or archived: its texts are written in place, file by file.
    // The work table and what came back are not revision records: they stay where they are.
    let archive = base.join(format!(".previous-{}-{stamp}", project.id));
    fs::create_dir_all(&folder)?;
    let mut entries = fs::read_dir(&stage)?
        .map(|e| Ok(e?.file_name()))
        .collect::<Result<Vec<_>>>()?;
    entries.sort();
    let mut swapped = Vec::new();
    for entry in entries {
        let (live, staged) = (folder.join(&entry), stage.join(&entry));
        if entry == "media" || (entry == "work-table" && live.is_dir()) {
            fs::remove_dir(&staged)?;
            continue;
        }
        let result = (|| -> Result<()> {
            if fs::symlink_metadata(&live).is_ok() {
                fs::create_dir_all(&archive)?;
                fs::rename(&live, archive.join(&entry))?;
            }
            fs::rename(&staged, &live)?;
            Ok(())
        })();
        if let Err(error) = result {
            // Put back what was swapped so far; the person sees the previous projection.
            for done in swapped.iter().rev().chain(std::iter::once(&entry)) {
                let (live, old) = (folder.join(done), archive.join(done));
                if old.exists() {
                    if live.exists() {
                        fs::rename(&live, stage.join(done))?;
                    }
                    fs::rename(&old, &live)?;
                }
            }
            return Err(error);
        }
        swapped.push(entry);
    }
    fs::remove_dir(&stage)?;
    let media_dir = folder.join("media");
    crate::scope::validate_root(&media_dir)?;
    fs::create_dir_all(&media_dir)?;
    if !media.is_empty() {
        write_media(&media_dir, &media)?;
    }
    Ok(folder)
}

const WRITTEN: &str = ".qroc-written";
fn written_media(dir: &Path) -> Result<Vec<String>> {
    let marker = dir.join(WRITTEN);
    crate::scope::validate_root(&marker)?;
    Ok(if marker.is_file() {
        fs::read_to_string(marker)?
            .lines()
            .map(str::to_owned)
            .collect()
    } else {
        Vec::new()
    })
}
/// Writes one file by a temporary beside it and a rename of that one file.
fn write_one(dir: &Path, relative: &str, content: &str) -> Result<()> {
    let path = dir.join(relative);
    crate::scope::validate_root(&path)?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("Media text has no folder".into()))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".qroc-tmp-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("text")
    ));
    crate::scope::validate_root(&temp)?;
    fs::write(&temp, content)?;
    fs::rename(temp, path)?;
    Ok(())
}
/// Writes the readable media texts and the list of names Q-ROC owns there.
fn write_media(dir: &Path, media: &BTreeMap<String, String>) -> Result<()> {
    for (name, content) in media {
        write_one(dir, name, content)?;
    }
    let mut names = written_media(dir)?;
    names.extend(media.keys().cloned());
    names.sort();
    names.dedup();
    write_one(dir, WRITTEN, &(names.join("\n") + "\n"))
}
