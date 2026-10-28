//! Durable task intake. Creating metadata never starts inference.
use crate::{lock_file, Result};
use fs2::FileExt;
use runloom_core::{CommandSpec, NewWork, Store, Work};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, os::unix::fs::PermissionsExt, path::Path};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Manual,
    AcceptEdits,
    Dangerous,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskSpec {
    pub workspace: String,
    pub request_id: String,
    pub directory: String,
    pub title: String,
    pub design: String,
    pub acceptance: String,
    pub permission: Permission,
    pub create_directory: bool,
    pub executable: String,
    pub config_root: String,
}
fn validate(s: &TaskSpec) -> Result<()> {
    if uuid::Uuid::parse_str(&s.request_id)?.to_string() != s.request_id {
        return Err("canonical task UUID required".into());
    }
    for value in [
        &s.workspace,
        &s.title,
        &s.directory,
        &s.executable,
        &s.config_root,
    ] {
        if value.trim().is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
            return Err("invalid task field".into());
        }
    }
    if s.design.trim().is_empty()
        || s.acceptance.trim().is_empty()
        || s.design.len() + s.acceptance.len() > 24000
        || s.design.contains('\0')
        || s.acceptance.contains('\0')
    {
        return Err("design and acceptance required; combined limit 24000 UTF-8 bytes".into());
    }
    for path in [&s.directory, &s.executable, &s.config_root] {
        if !Path::new(path).is_absolute() {
            return Err("absolute task paths required".into());
        }
    }
    if !Path::new(&s.executable).is_file() || !Path::new(&s.config_root).is_dir() {
        return Err("Claude executable/profile unavailable".into());
    }
    let dir = Path::new(&s.directory);
    let parent = dir.parent().ok_or("project parent missing")?;
    if fs::canonicalize(parent)?.join(dir.file_name().ok_or("project name missing")?) != dir {
        return Err("canonical project path required".into());
    }
    if dir.exists() && (!dir.is_dir() || fs::symlink_metadata(dir)?.file_type().is_symlink()) {
        return Err("project must be a real directory".into());
    }
    if !dir.exists() && !s.create_directory {
        return Err("project does not exist; explicitly select create directory".into());
    }
    Ok(())
}
pub fn create(root: &Path, host: &str, spec: TaskSpec, dangerous_ack: bool) -> Result<Work> {
    validate(&spec)?;
    if spec.permission == Permission::Dangerous && !dangerous_ack {
        return Err("dangerous permission requires explicit task acknowledgement".into());
    }
    let folder = root.join("tasks");
    fs::create_dir_all(&folder)?;
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o700))?;
    let lock = lock_file(&folder.join(format!("{}.lock", spec.request_id)))?;
    lock.lock_exclusive()?;
    let path = folder.join(format!("{}.json", spec.request_id));
    if path.exists() {
        let previous: TaskSpec = serde_json::from_slice(&fs::read(&path)?)?;
        if previous != spec {
            return Err("task request ID conflict; original specification is immutable".into());
        }
    } else {
        // An intake UUID may not adopt an existing provider transcript.
        let projects = Path::new(&spec.config_root).join("projects");
        if projects.exists() {
            for (count, entry) in fs::read_dir(projects)?.enumerate() {
                if count >= 50000 {
                    return Err("profile lookup limit reached".into());
                }
                if entry?
                    .path()
                    .join(format!("{}.jsonl", spec.request_id))
                    .exists()
                {
                    return Err("task ID already belongs to a native session".into());
                }
            }
        }
        let tmp = path.with_extension("tmp");
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&serde_json::to_vec(&spec)?)?;
        f.sync_all()?;
        fs::rename(tmp, &path)?;
        fs::File::open(&folder)?.sync_all()?;
    }
    if !Path::new(&spec.directory).exists() {
        fs::create_dir(&spec.directory)?;
        fs::set_permissions(&spec.directory, fs::Permissions::from_mode(0o700))?;
    }
    let identity = NewWork {
        workspace: spec.workspace.clone(),
        host: host.into(),
        provider: "claude-code".into(),
        config_root: spec.config_root.clone(),
        native_id: spec.request_id.clone(),
        directory: spec.directory.clone(),
        title: spec.title.clone(),
    };
    let mut store = Store::open(root.join("state.db"))?;
    let work = if let Some(work) = store.list(&spec.workspace)?.into_iter().find(|w| {
        w.identity.native_id == spec.request_id
            && w.identity.provider == "claude-code"
            && w.identity.config_root == spec.config_root
    }) {
        if work.identity != identity {
            return Err("task identity conflicts with registered work".into());
        }
        work
    } else {
        store.create_work(identity)?
    };
    store.append_event(&spec.workspace,&work.id,&spec.request_id,"task.created",serde_json::json!({"title":spec.title,"permission":spec.permission,"acceptance":spec.acceptance,"state":"prepared"}))?;
    Ok(work)
}
pub fn info(root: &Path, work: &Work) -> Result<Option<TaskSpec>> {
    if work.identity.provider != "claude-code"
        || uuid::Uuid::parse_str(&work.identity.native_id).is_err()
    {
        return Ok(None);
    }
    let path = root
        .join("tasks")
        .join(format!("{}.json", work.identity.native_id));
    if !path.exists() {
        return Ok(None);
    }
    let spec: TaskSpec = serde_json::from_slice(&fs::read(path)?)?;
    if spec.workspace != work.identity.workspace
        || spec.directory != work.identity.directory
        || spec.config_root != work.identity.config_root
    {
        return Err("task scope/identity mismatch".into());
    }
    Ok(Some(spec))
}
pub fn plan(work: &Work, spec: &TaskSpec, new: bool, prompt: Option<&str>) -> Result<CommandSpec> {
    let mut command = runloom_core::resume_plan(work, &spec.executable, None, false)?;
    let mode = match spec.permission {
        Permission::Manual => "manual",
        Permission::AcceptEdits => "acceptEdits",
        Permission::Dangerous => "bypassPermissions",
    };
    command.args = vec![
        "--permission-mode".into(),
        mode.into(),
        if new {
            "--session-id".into()
        } else {
            "--resume".into()
        },
        work.identity.native_id.clone(),
    ];
    if spec.permission == Permission::Dangerous {
        command.args.push("--dangerously-skip-permissions".into());
        if new {
            command.args.extend([
                "--print".into(),
                "--verbose".into(),
                "--output-format".into(),
                "stream-json".into(),
            ]);
        }
    }
    let initial=format!("Implement the following design in the current project. Continue through implementation and verification without stopping merely to propose a plan. Follow the selected permission mode. If blocked by missing credentials, quota, unavailable dependencies or an essential decision, explain the blocker; do not claim completion. Do not deploy, publish, push, send messages, or modify unrelated projects unless the design explicitly authorizes it. At the end report completed criteria, actual test commands/results, changed files, and remaining issues. A successful process exit alone is not acceptance.\n\nDESIGN:\n{}\n\nACCEPTANCE CRITERIA:\n{}",spec.design,spec.acceptance);
    if let Some(prompt) = if new { Some(initial.as_str()) } else { prompt } {
        if prompt.len() > 30000 || prompt.contains('\0') {
            return Err("task prompt too large or invalid".into());
        }
        command.args.extend(["--".into(), prompt.into()]);
    }
    Ok(command)
}
