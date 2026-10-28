use crate::{protocol, Result};
use runloom_core::{InputRequest, ResizeRequest, Store};
use serde_json::json;
use std::{
    fs,
    io::Write,
    os::unix::{
        fs::PermissionsExt,
        io::RawFd,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
pub fn path(root: &Path, id: &str) -> PathBuf {
    root.join(format!("{id}.sock"))
}
pub fn forward(root: &Path, input: &InputRequest) -> Result<serde_json::Value> {
    forward_value(root, &input.execution, &serde_json::to_value(input)?)
}
pub fn forward_resize(root: &Path, resize: &ResizeRequest) -> Result<serde_json::Value> {
    forward_value(root, &resize.execution, &serde_json::to_value(resize)?)
}
fn forward_value(
    root: &Path,
    execution: &str,
    value: &serde_json::Value,
) -> Result<serde_json::Value> {
    let mut stream = UnixStream::connect(path(root, execution))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    writeln!(stream, "{}", serde_json::to_string(value)?)?;
    let bytes = protocol::read_frame(stream, 65536)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    if value["ok"] != true {
        return Err(value["error"]
            .as_str()
            .unwrap_or("input rejected")
            .to_owned()
            .into());
    }
    Ok(value)
}
fn bounded_write(writer: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    let end = Instant::now() + Duration::from_secs(1);
    let mut sent = 0;
    while sent < bytes.len() {
        match writer.write(&bytes[sent..]) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(n) => sent += n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                if Instant::now() >= end {
                    return Err(std::io::ErrorKind::TimedOut.into());
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
pub fn nonblocking(fd: RawFd) -> Result<()> {
    // fcntl operates on the owned pipe/PTY handle and preserves its existing flags.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}
pub fn listen(
    root: &Path,
    id: &str,
    mut stdin: Box<dyn Write + Send>,
    fd: RawFd,
    done: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>> {
    nonblocking(fd)?;
    let socket = path(root, id);
    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let db = root.join("state.db");
    let execution = id.to_owned();
    Ok(thread::spawn(move || {
        while !done.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                    let result = (|| -> Result<bool> {
                        let bytes = protocol::read_frame(&stream, 32768)?;
                        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
                        if value.get("cols").is_some() {
                            let resize: ResizeRequest = serde_json::from_value(value)?;
                            if resize.execution != execution {
                                return Err("wrong execution socket".into());
                            }
                            let mut store = Store::open(&db)?;
                            store.resize_terminal(&resize, || {
                                let size = libc::winsize {
                                    ws_row: resize.rows,
                                    ws_col: resize.cols,
                                    ws_xpixel: 0,
                                    ws_ypixel: 0,
                                };
                                // fd belongs to this worker's live PTY; the DB lock fences takeover.
                                if unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &size) } < 0 {
                                    return Err(std::io::Error::last_os_error());
                                }
                                Ok(())
                            })?;
                            return Ok(true);
                        }
                        let input: InputRequest = serde_json::from_slice(&bytes)?;
                        if input.execution != execution {
                            return Err("wrong execution socket".into());
                        }
                        let mut store = Store::open(&db)?;
                        let fresh = store.prepare_input(&input)?;
                        if fresh {
                            store
                                .deliver_input(&input, |bytes| bounded_write(&mut stdin, bytes))?;
                        }
                        Ok(fresh)
                    })();
                    let response = match result {
                        Ok(fresh) => json!({"ok":true,"state":"delivered","newly_delivered":fresh}),
                        Err(error) => json!({"ok":false,"error":error.to_string()}),
                    };
                    let _ = writeln!(stream, "{response}");
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20))
                }
                Err(_) => break,
            }
        }
        let _ = fs::remove_file(socket);
    }))
}
