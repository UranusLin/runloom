use crate::{worker_lock, Result};
use fs2::FileExt;
use runloom_core::Store;
use serde_json::json;
use std::{
    io::Read,
    os::unix::{io::AsRawFd, process::ExitStatusExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};
const OUTPUT_LIMIT: usize = 1024 * 1024;
// SQLite contention is temporary, not a reason to drop a child's output pipe.
// Hold this bounded chunk and apply pipe backpressure until it can be persisted.
fn retry_busy<T>(
    mut operation: impl FnMut() -> std::result::Result<T, runloom_core::Error>,
) -> std::result::Result<T, String> {
    loop {
        match operation() {
            Err(error) if error.is_busy() => {
                thread::sleep(std::time::Duration::from_millis(25));
            }
            other => return other.map_err(|error| error.to_string()),
        }
    }
}
pub(crate) fn capture(
    mut input: impl Read,
    db: PathBuf,
    workspace: String,
    work: String,
    execution: String,
    stream: &str,
) -> std::result::Result<(), String> {
    let mut store = retry_busy(|| Store::open(&db))?;
    let mut buffer = [0; 4096];
    let mut saved = 0;
    let mut chunk = 0;
    let mut truncated = false;
    loop {
        let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        let count = n.min(OUTPUT_LIMIT - saved);
        if count > 0 {
            retry_busy(|| {
                store.append_event(
                    &workspace,
                    &work,
                    &format!("{execution}:{stream}:{chunk}"),
                    "execution.output",
                    json!({"execution":execution,"stream":stream,"bytes":&buffer[..count]}),
                )
            })?;
            saved += count;
            chunk += 1;
        }
        if count < n && !truncated {
            retry_busy(|| {
                store.append_event(
                    &workspace,
                    &work,
                    &format!("{execution}:{stream}:truncated"),
                    "execution.output_truncated",
                    json!({"execution":execution,"stream":stream,"limit":OUTPUT_LIMIT}),
                )
            })?;
            truncated = true;
        }
        // Continue draining after cap so a verbose child cannot block indefinitely.
    }
    Ok(())
}
pub fn run(root: &Path, id: &str) -> Result<()> {
    let lock = worker_lock(root, id)?;
    lock.try_lock_exclusive()?;
    let db = root.join("state.db");
    let mut store = Store::open(&db)?;
    let execution = store.execution(id)?;
    let workspace = store.execution_workspace(id)?;
    store.claim_execution(id)?;
    if execution.command.tty {
        let result = crate::pty_worker::run(root, &execution, &workspace);
        if let Err(ref error) = result {
            store.finish_execution(id, None, Some(&error.to_string()))?;
        }
        return result;
    }
    let result = (|| -> Result<()> {
        let mut child = Command::new(&execution.command.executable)
            .args(&execution.command.args)
            .envs(&execution.command.env)
            .current_dir(&execution.command.directory)
            .stdin(if execution.command.interactive {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let done = Arc::new(AtomicBool::new(false));
        let input_thread = if execution.command.interactive {
            let stdin = child.stdin.take().ok_or("missing stdin")?;
            let fd = stdin.as_raw_fd();
            match crate::input::listen(root, id, Box::new(stdin), fd, done.clone()) {
                Ok(handle) => Some(handle),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
            }
        } else {
            None
        };
        let mut readers = Vec::new();
        for (stream, input) in [
            (
                "stdout",
                Box::new(child.stdout.take().ok_or("missing stdout")?) as Box<dyn Read + Send>,
            ),
            (
                "stderr",
                Box::new(child.stderr.take().ok_or("missing stderr")?) as Box<dyn Read + Send>,
            ),
        ] {
            let (db, workspace, work, execution) = (
                db.clone(),
                workspace.clone(),
                execution.work_id.clone(),
                id.to_owned(),
            );
            readers.push(thread::spawn(move || {
                capture(input, db, workspace, work, execution, stream)
            }));
        }
        let status = child.wait();
        done.store(true, Ordering::SeqCst);
        if let Some(handle) = input_thread {
            let _ = handle.join();
        }
        let status = status?;
        for reader in readers {
            reader
                .join()
                .map_err(|_| "output reader panicked")?
                .map_err(|e| format!("output capture failed: {e}"))?;
        }
        store.append_event(
            &workspace,
            &execution.work_id,
            &format!("{id}:exit"),
            "execution.exited",
            json!({"execution":id,"code":status.code(),"signal":status.signal()}),
        )?;
        store.finish_execution(id, status.code(), None)?;
        Ok(())
    })();
    if let Err(error) = result {
        store.finish_execution(id, None, Some(&error.to_string()))?;
        return Err(error);
    }
    Ok(())
}
