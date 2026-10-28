//! Durable, at-most-once checkpoint forks. A lost worker is unknown, never replayed.
use crate::{lock_file, Result};
use fs2::FileExt;
use runloom_core::{LeaseProof, Store, Work};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BranchRequest {
    pub workspace: String,
    pub work: String,
    pub checkpoint: String,
    pub request_id: String,
    pub destination: String,
    pub executable: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchJob {
    #[serde(flatten)]
    pub request: BranchRequest,
    pub source: Work,
    pub state: String,
    pub branch: String,
    pub result: Option<Work>,
    pub error: Option<String>,
}
fn path(root: &Path, id: &str) -> Result<PathBuf> {
    let parsed = uuid::Uuid::parse_str(id)?;
    if parsed.to_string() != id {
        return Err("canonical request UUID required".into());
    }
    Ok(root.join("branches").join(format!("{id}.json")))
}
fn save(root: &Path, job: &BranchJob) -> Result<()> {
    let path = path(root, &job.request.request_id)?;
    let tmp = path.with_extension("tmp");
    let mut f = fs::File::create(&tmp)?;
    f.write_all(&serde_json::to_vec(job)?)?;
    f.sync_all()?;
    fs::rename(tmp, &path)?;
    fs::File::open(path.parent().ok_or("missing parent")?)?.sync_all()?;
    Ok(())
}
fn read(root: &Path, id: &str) -> Result<BranchJob> {
    Ok(serde_json::from_slice(&fs::read(path(root, id)?)?)?)
}
pub fn status(root: &Path, workspace: &str, work: &str, id: &str) -> Result<BranchJob> {
    let mut job = read(root, id)?;
    if job.request.workspace != workspace || job.request.work != work {
        return Err("branch scope mismatch".into());
    }
    if matches!(job.state.as_str(), "queued" | "creating") && !crate::worker_alive(root, id)? {
        job.state = "unknown".into();
    }
    Ok(job)
}
pub fn admit(
    root: &Path,
    host: &str,
    request: BranchRequest,
    proof: LeaseProof,
) -> Result<BranchJob> {
    let record = path(root, &request.request_id)?;
    // Request UUIDs are global to the node, whereas checkpoint reservations are
    // scoped to overlapping work/native identities. Preserve deduplication when
    // unrelated sources submit the same UUID through concurrent connections.
    let admission = lock_file(&root.join(format!("{}.branch-admission.lock", request.request_id)))?;
    admission.lock_exclusive()?;
    let mut store = Store::open(root.join("state.db"))?;
    let mut created = false;
    let job = store.with_idle_work(&request.workspace, &request.work, &proof, |source| {
        if source.identity.host != host {
            return Err("wrong branch host".into());
        }
        if record.exists() {
            let existing = read(root, &request.request_id)?;
            if existing.request != request {
                return Err("branch request ID conflict".into());
            }
            return Ok(existing);
        }
        if !matches!(source.identity.provider.as_str(), "codex" | "claude-code") {
            return Err("native checkpoint fork unavailable for provider".into());
        }
        if !Path::new(&request.executable).is_absolute()
            || !Path::new(&request.executable).is_file()
        {
            return Err("provider executable must be an existing absolute file".into());
        }
        if request.destination.len() > 4096 || request.destination.chars().any(char::is_control) {
            return Err("invalid branch destination".into());
        }
        let dest = Path::new(&request.destination);
        if !dest.is_absolute()
            || fs::symlink_metadata(dest).is_ok()
            || dest
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err("branch destination must be a new absolute directory".into());
        }
        // Resolve the parent now, so the new cwd has a single unambiguous spelling.
        if fs::canonicalize(dest.parent().ok_or("destination parent missing")?)?
            .join(dest.file_name().ok_or("destination name missing")?)
            != dest
        {
            return Err("use canonical destination parent".into());
        }
        if dest.starts_with(fs::canonicalize(&source.identity.directory)?)
            || dest.starts_with(root)
            || dest.starts_with(fs::canonicalize(&source.identity.config_root)?)
        {
            return Err(
                "branch must be outside the source workspace and private metadata roots".into(),
            );
        }
        let manifest = runloom_checkpoint::inspect(&root.join("checkpoints"), &request.checkpoint)?;
        if manifest.work.id != source.id
            || manifest.work.identity != source.identity
            || manifest.native_session.is_none()
        {
            return Err("checkpoint must contain this work's native transcript".into());
        }
        fs::create_dir_all(root.join("branches"))?;
        fs::set_permissions(root.join("branches"), fs::Permissions::from_mode(0o700))?;
        let job = BranchJob {
            request: request.clone(),
            source,
            state: "queued".into(),
            branch: format!("runloom/{}", request.request_id),
            result: None,
            error: None,
        };
        save(root, &job)?;
        created = true;
        Ok(job)
    })?;
    if created {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("--root")
            .arg(root)
            .arg("fork-worker")
            .arg("--request-id")
            .arg(&request.request_id)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: setsid is async-signal-safe; worker owns its lifetime after SSH/node disconnect.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command.spawn()?;
    }
    Ok(job)
}
pub fn run(root: &Path, id: &str) -> Result<()> {
    let lock = lock_file(&root.join(format!("{id}.lock")))?;
    lock.try_lock_exclusive()?;
    let mut job = read(root, id)?;
    if job.state != "queued" {
        return Err("fork worker already claimed; cannot replay".into());
    }
    job.state = "creating".into();
    save(root, &job)?;
    let mut store = Store::open(root.join("state.db"))?;
    let result = store.with_reserved_destination(
        &job.request.workspace,
        &job.source.id,
        Path::new(&job.request.destination),
        || perform(root, &job),
    );
    match result {
        Ok(work) => {
            job.state = "ready".into();
            job.result = Some(work);
        }
        Err(error) => {
            // Native CLI may have succeeded before a lost response. Never auto-retry.
            job.state = "needs_inspection".into();
            job.error = Some(error.to_string());
        }
    }
    save(root, &job)
}
fn perform(root: &Path, job: &BranchJob) -> Result<Work> {
    let r = &job.request;
    let backing = root
        .join("branches")
        .join(format!("{}.git-store", r.request_id));
    runloom_checkpoint::restore_worktree(
        &root.join("checkpoints"),
        &r.checkpoint,
        Path::new(&r.destination),
        &backing,
        &job.branch,
    )?;
    let mut child = Command::new("/usr/bin/python3")
        .args(["-c", include_str!("../support/fork-native.py")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("missing helper stdin")?
        .write_all(&serde_json::to_vec(job)?)?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!(
            "native fork failed; inspect retained branch artifacts: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let result: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let native = result["native_id"]
        .as_str()
        .ok_or("native fork returned no identity")?;
    uuid::Uuid::parse_str(native)?;
    if native == job.source.identity.native_id {
        return Err("source identity reused".into());
    }
    let mut identity = job.source.identity.clone();
    identity.native_id = native.into();
    identity.directory = r.destination.clone();
    identity.title = format!(
        "{} · branch",
        identity.title.chars().take(200).collect::<String>()
    );
    // Persist the provider result before registration for recovery after a crash.
    let receipt = root
        .join("branches")
        .join(format!("{}.native.json", r.request_id));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(receipt)?;
    file.write_all(&serde_json::to_vec(&identity)?)?;
    file.sync_all()?;
    let mut store = Store::open(root.join("state.db"))?;
    let work = store.create_work(identity)?;
    store.append_event(
        &r.workspace,
        &work.id,
        &r.request_id,
        "checkpoint.forked",
        serde_json::json!({
            "parent_work":r.work,"checkpoint":r.checkpoint,"branch":job.branch,"backing":backing,
            "restoration":"native_transcript","source_head_preserved":false
        }),
    )?;
    Ok(work)
}

pub fn list(root: &Path, workspace: &str, work: &str) -> Result<Vec<BranchJob>> {
    let folder = root.join("branches");
    if !folder.exists() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for (count, entry) in fs::read_dir(folder)?.enumerate() {
        if count >= 10000 {
            return Err("branch list limit reached".into());
        }
        let p = entry?.path();
        let Some(id) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if p.extension().and_then(|s| s.to_str()) != Some("json")
            || uuid::Uuid::parse_str(id).is_err()
        {
            continue;
        }
        let job = read(root, id)?;
        if job.request.workspace == workspace && job.request.work == work {
            result.push(status(root, workspace, work, id)?);
        }
    }
    Ok(result)
}
