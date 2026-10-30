use crate::{
    lock_file,
    protocol::{self, Operation, Request},
    worker_alive, Result,
};
use fs2::FileExt;
use runloom_core::Store;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::UnixListener,
        process::CommandExt,
    },
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
fn dispatch(
    root: &Path,
    host: &str,
    allow_batch: bool,
    allow_interactive: bool,
    request: Request,
) -> Result<Value> {
    if request.version != 1 {
        return Err("unsupported protocol version".into());
    }
    let mut store = Store::open(root.join("state.db"))?;
    match request.operation {
        Operation::RecoveryPlan { workspace, work } => Ok(serde_json::to_value(
            store.recovery_plan(&workspace, &work)?,
        )?),
        Operation::RecoveryInspect { workspace, work } => Ok(serde_json::to_value(
            store.recovery_inspect(&workspace, &work)?,
        )?),
        Operation::CheckpointFork { request, proof } => {
            if !allow_interactive {
                return Err("interactive branches disabled".into());
            }
            Ok(serde_json::to_value(crate::branch::admit(
                root, host, request, proof,
            )?)?)
        }
        Operation::BranchList { workspace, work } => Ok(serde_json::to_value(
            crate::branch::list(root, &workspace, &work)?,
        )?),
        Operation::BranchStatus {
            workspace,
            work,
            request_id,
        } => Ok(serde_json::to_value(crate::branch::status(
            root,
            &workspace,
            &work,
            &request_id,
        )?)?),
        Operation::TaskCreate {
            spec,
            dangerous_ack,
        } => {
            if !allow_interactive {
                return Err("task execution disabled".into());
            }
            Ok(serde_json::to_value(crate::task::create(
                root,
                host,
                spec,
                dangerous_ack,
            )?)?)
        }
        Operation::TaskInfo { workspace, work } => {
            let selected = store
                .list(&workspace)?
                .into_iter()
                .find(|w| w.id == work)
                .ok_or("work not found")?;
            Ok(serde_json::to_value(crate::task::info(root, &selected)?)?)
        }
        Operation::TaskStart {
            workspace,
            work,
            proof,
        } => {
            let selected = store
                .list(&workspace)?
                .into_iter()
                .find(|w| w.id == work)
                .ok_or("work not found")?;
            let spec = crate::task::info(root, &selected)?.ok_or("not a prepared task")?;
            let command = crate::task::plan(&selected, &spec, true, None)?;
            start(
                root,
                host,
                (allow_batch, allow_interactive),
                StartRequest {
                    workspace,
                    work,
                    proof: Some(proof),
                    request_id: format!("task-launch-{}", spec.request_id),
                    command,
                },
            )
        }
        Operation::Ping => Ok(
            json!({"host":host,"batch_enabled":allow_batch,"interactive_input":allow_interactive,"checkpoint_fork":true,"task_create":true,"recovery_inspect":true,"recovery_plan":true,"schema_version":runloom_core::SCHEMA_VERSION,"package_version":env!("CARGO_PKG_VERSION")}),
        ),
        Operation::InputStatus {
            workspace,
            work,
            execution,
            request_id,
        } => Ok(json!({"state":store.input_status(&workspace,&work,&execution,&request_id)?})),
        Operation::LeaseStatus { workspace, work } => Ok(serde_json::to_value(
            store.lease_status(&workspace, &work)?,
        )?),
        Operation::Acquire {
            workspace,
            work,
            holder,
            ttl,
            takeover_epoch,
        } => Ok(serde_json::to_value(store.acquire_lease(
            &workspace,
            &work,
            &holder,
            ttl,
            takeover_epoch,
        )?)?),
        Operation::Renew {
            workspace,
            work,
            proof,
            ttl,
        } => {
            store.renew_lease(&workspace, &work, &proof, ttl)?;
            Ok(json!({"renewed":true}))
        }
        Operation::Release {
            workspace,
            work,
            proof,
        } => {
            store.release_lease(&workspace, &work, &proof)?;
            Ok(json!({"released":true}))
        }
        Operation::Resize { resize } => {
            if !allow_interactive {
                return Err("interactive input disabled".into());
            }
            store.scoped_execution(&resize.workspace, &resize.work, &resize.execution)?;
            crate::input::forward_resize(root, &resize)
        }
        Operation::Input { input } => {
            if !allow_interactive {
                return Err("interactive input disabled".into());
            }
            store.scoped_execution(&input.workspace, &input.work, &input.execution)?;
            if store
                .input_status(
                    &input.workspace,
                    &input.work,
                    &input.execution,
                    &input.request_id,
                )?
                .is_some()
                && !store.prepare_input(&input)?
            {
                return Ok(json!({"ok":true,"state":"delivered","newly_delivered":false}));
            }
            crate::input::forward(root, &input)
        }
        Operation::Resume {
            workspace,
            work,
            proof,
            request_id,
            executable,
            prompt,
            headless,
        } => {
            let registered = store
                .list(&workspace)?
                .into_iter()
                .find(|w| w.id == work)
                .ok_or("work not found")?;
            let command = if let Some(spec) = crate::task::info(root, &registered)? {
                if headless {
                    return Err("managed tasks use the interactive terminal".into());
                }
                crate::task::plan(&registered, &spec, false, prompt.as_deref())?
            } else {
                runloom_core::resume_plan(&registered, &executable, prompt.as_deref(), headless)?
            };
            start(
                root,
                host,
                (allow_batch, allow_interactive),
                StartRequest {
                    workspace,
                    work,
                    request_id,
                    command,
                    proof: Some(proof),
                },
            )
        }
        Operation::ResumePlan {
            workspace,
            work,
            executable,
            prompt,
            headless,
        } => {
            let work = store
                .list(&workspace)?
                .into_iter()
                .find(|w| w.id == work)
                .ok_or("work not found")?;
            if work.identity.host != host {
                return Err("wrong execution host".into());
            }
            let command = if let Some(spec) = crate::task::info(root, &work)? {
                if headless {
                    return Err("managed tasks use the interactive terminal".into());
                }
                crate::task::plan(&work, &spec, false, prompt.as_deref())?
            } else {
                runloom_core::resume_plan(&work, &executable, prompt.as_deref(), headless)?
            };
            Ok(serde_json::to_value(command)?)
        }
        Operation::CheckpointList { workspace, work } => {
            if !store.list(&workspace)?.iter().any(|w| w.id == work) {
                return Err("work not found".into());
            }
            Ok(serde_json::to_value(runloom_checkpoint::list(
                &root.join("checkpoints"),
                &workspace,
                &work,
            )?)?)
        }
        Operation::CheckpointCapture {
            workspace,
            work,
            proof,
            untracked,
            native_session,
            auto_native,
            parent,
        } => {
            let storage = root.join("checkpoints");
            let id = store.with_idle_work(&workspace, &work, &proof, |selected| {
                if selected.identity.host != host {
                    return Err("wrong checkpoint host".into());
                }
                let discovered = if auto_native && native_session.is_none() {
                    Some(runloom_checkpoint::discover_native(&selected)?)
                } else {
                    None
                };
                runloom_checkpoint::capture_with_parent(
                    &storage,
                    selected,
                    &untracked,
                    native_session
                        .as_deref()
                        .map(Path::new)
                        .or(discovered.as_deref()),
                    parent.as_deref(),
                )
            })?;
            Ok(json!({"checkpoint":id}))
        }
        Operation::CheckpointInspect {
            workspace,
            work,
            checkpoint,
        } => {
            if !store.list(&workspace)?.iter().any(|w| w.id == work) {
                return Err("work not found".into());
            }
            let manifest = runloom_checkpoint::inspect(&root.join("checkpoints"), &checkpoint)?;
            if manifest.work.id != work || manifest.work.identity.workspace != workspace {
                return Err("checkpoint identity mismatch".into());
            }
            Ok(serde_json::to_value(manifest)?)
        }
        Operation::CheckpointRestore {
            workspace,
            work,
            proof,
            checkpoint,
            destination,
        } => {
            if !Path::new(&destination).is_absolute() {
                return Err("absolute restore path required".into());
            }
            let storage = root.join("checkpoints");
            let manifest = store.with_idle_work_paths(
                &workspace,
                &work,
                &proof,
                &[Path::new(&destination)],
                |selected| {
                    let manifest = runloom_checkpoint::inspect(&storage, &checkpoint)?;
                    if manifest.work.id != selected.id
                        || manifest.work.identity != selected.identity
                    {
                        return Err("checkpoint identity mismatch".into());
                    }
                    runloom_checkpoint::restore(&storage, &checkpoint, Path::new(&destination))
                },
            )?;
            Ok(
                json!({"destination":destination,"source_head":manifest.source_head,"native_session_exported":manifest.native_session.is_some()}),
            )
        }
        Operation::Register { work } => {
            if work.host != host {
                return Err("work host does not match node".into());
            }
            Ok(serde_json::to_value(store.create_work(work)?)?)
        }
        Operation::List { workspace } => Ok(serde_json::to_value(store.list(&workspace)?)?),
        Operation::Start {
            workspace,
            work,
            request_id,
            command,
            proof,
        } => start(
            root,
            host,
            (allow_batch, allow_interactive),
            StartRequest {
                workspace,
                work,
                request_id,
                command,
                proof,
            },
        ),

        Operation::LatestExecution { workspace, work } => {
            let mut execution = store.latest_execution(&workspace, &work)?;
            if let Some(ref mut execution) = execution {
                if matches!(execution.state.as_str(), "queued" | "running")
                    && !worker_alive(root, &execution.id)?
                {
                    execution.state = "unknown".into();
                }
            }
            Ok(serde_json::to_value(execution)?)
        }
        Operation::Status {
            workspace,
            work,
            execution,
        } => {
            let mut execution = store.scoped_execution(&workspace, &work, &execution)?;
            if matches!(execution.state.as_str(), "queued" | "running")
                && !worker_alive(root, &execution.id)?
            {
                execution.state = "unknown".into();
                execution.error=Some("No live supervisor confirmed; may be starting or interrupted. Never automatically rerun.".into());
            }
            Ok(serde_json::to_value(execution)?)
        }
        Operation::Events {
            workspace,
            work,
            after,
            limit,
        } => {
            if limit > 100 {
                return Err("protocol event page limit is 100".into());
            }
            Ok(serde_json::to_value(
                store.events(&workspace, &work, after, limit)?,
            )?)
        }
    }
}

struct StartRequest {
    workspace: String,
    work: String,
    request_id: String,
    command: runloom_core::CommandSpec,
    proof: Option<runloom_core::LeaseProof>,
}
fn start(root: &Path, host: &str, flags: (bool, bool), request: StartRequest) -> Result<Value> {
    let (allow_batch, allow_interactive) = flags;
    let StartRequest {
        workspace,
        work,
        request_id,
        command,
        proof,
    } = request;
    let mut store = Store::open(root.join("state.db"))?;
    if (command.interactive && !allow_interactive) || (!command.interactive && !allow_batch) {
        return Err("batch execution disabled; interactive agent control awaits M3".into());
    }
    let registered = store
        .list(&workspace)?
        .into_iter()
        .find(|w| w.id == work)
        .ok_or("work not found")?;
    if registered.identity.host != host
        || fs::canonicalize(&registered.identity.directory)?
            != fs::canonicalize(&command.directory)?
    {
        return Err("execution host/directory mismatch".into());
    }
    let (execution, created) =
        store.admit_guarded(&workspace, &work, &request_id, command, proof.as_ref())?;
    if created {
        let child = Command::new(std::env::current_exe()?)
            .arg("--root")
            .arg(root)
            .arg("worker")
            .arg("--execution")
            .arg(&execution.id)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn();
        match child {
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(error) => {
                store.finish_execution(
                    &execution.id,
                    None,
                    Some(&format!("worker launch failed: {error}")),
                )?;
                return Err(error.into());
            }
        }
    }
    Ok(json!({"execution":execution,"newly_admitted":created}))
}

// A permit is released even if request handling unwinds. Limit slow clients and
// long filesystem operations without serializing lease/input/status traffic.
struct RequestPermit(Arc<AtomicUsize>);
impl Drop for RequestPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn serve(root: &Path, host: &str, allow_batch: bool, allow_interactive: bool) -> Result<()> {
    if host.trim().is_empty() {
        return Err("host is required".into());
    }
    let lock = lock_file(&root.join("node.lock"))?;
    lock.try_lock_exclusive()?;
    Store::open(root.join("state.db"))?;
    let socket = root.join("node.sock");
    if let Ok(meta) = fs::symlink_metadata(&socket) {
        if !meta.file_type().is_socket() {
            return Err("socket path occupied by non-socket".into());
        }
        fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    println!("{}", json!({"ready":true,"protocol":1,"host":host}));
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let mut stream = stream?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        if active
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < 32).then_some(n + 1)
            })
            .is_err()
        {
            let _ = writeln!(
                stream,
                "{}",
                json!({"version":1,"ok":false,"error":"node request capacity reached; retry read-only calls later"})
            );
            continue;
        }
        let permit = RequestPermit(active.clone());
        let root = root.to_path_buf();
        let host = host.to_owned();
        let spawned = std::thread::Builder::new()
            .name("runloom-request".into())
            .spawn(move || {
                let _permit = permit;
                let result = (|| {
                    let bytes = protocol::read_frame(&stream, 65536)?;
                    dispatch(
                        &root,
                        &host,
                        allow_batch,
                        allow_interactive,
                        serde_json::from_slice(&bytes)?,
                    )
                })();
                let response = match result {
                    Ok(value) => json!({"version":1,"ok":true,"result":value}),
                    Err(error) => json!({"version":1,"ok":false,"error":error.to_string()}),
                };
                // A lost response never retries the accepted operation.
                let _ = writeln!(stream, "{response}");
            });
        if let Err(error) = spawned {
            eprintln!("request thread unavailable: {error}");
        }
    }
    Ok(())
}
