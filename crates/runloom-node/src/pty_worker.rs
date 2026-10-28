use crate::{input, worker::capture, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use runloom_core::{Execution, Store};
use serde_json::json;
use std::{
    io::{ErrorKind, Read},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
struct Reader {
    inner: Box<dyn Read + Send>,
    done: Arc<AtomicBool>,
}
impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            match self.inner.read(buf) {
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    if self.done.load(Ordering::SeqCst) {
                        return Ok(0);
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) if e.raw_os_error() == Some(libc::EIO) => return Ok(0),
                other => return other,
            }
        }
    }
}
pub fn run(root: &Path, execution: &Execution, workspace: &str) -> Result<()> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 36,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;
    let mut command = CommandBuilder::new(&execution.command.executable);
    command.args(&execution.command.args);
    command.cwd(&execution.command.directory);
    for (key, value) in &execution.command.env {
        command.env(key, value);
    }
    command.env("TERM", "xterm-256color");
    let done = Arc::new(AtomicBool::new(false));
    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let fd = pair
        .master
        .as_raw_fd()
        .ok_or("PTY file descriptor unavailable")?;
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|e| e.to_string())?;
    drop(pair.slave);
    let input_thread = match input::listen(root, &execution.id, writer, fd, done.clone()) {
        Ok(thread) => thread,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let (db, workspace, work, id) = (
        root.join("state.db"),
        workspace.to_owned(),
        execution.work_id.clone(),
        execution.id.clone(),
    );
    let read_done = done.clone();
    let read_db = db.clone();
    let read_workspace = workspace.clone();
    let read_id = id.clone();
    let read_work = work.clone();
    let output = thread::spawn(move || {
        capture(
            Reader {
                inner: reader,
                done: read_done.clone(),
            },
            read_db,
            read_workspace,
            read_work,
            read_id,
            "terminal",
        )
    });
    let status = child.wait();
    done.store(true, Ordering::SeqCst);
    let _ = input_thread.join();
    let status = status?;
    output
        .join()
        .map_err(|_| "PTY reader panicked")?
        .map_err(|e| format!("PTY capture failed: {e}"))?;
    let mut store = Store::open(db)?;
    store.append_event(
        &workspace,
        &work,
        &format!("{id}:exit"),
        "execution.exited",
        json!({"execution":id,"code":status.exit_code(),"status":status.to_string()}),
    )?;
    store.finish_execution(
        &id,
        Some(
            status
                .exit_code()
                .try_into()
                .map_err(|_| "exit code overflow")?,
        ),
        None,
    )?;
    Ok(())
}
