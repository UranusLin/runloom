mod branch;
mod input;
mod protocol;
mod pty_worker;
mod server;
mod task;
mod worker;
use clap::{Parser, Subcommand};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
#[derive(Parser)]
#[command(version, about = "Runloom private Unix node and SSH stdio bridge")]
struct Cli {
    #[arg(long)]
    root: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Foreground service. Use a service manager for unattended startup.
    Serve {
        #[arg(long)]
        host: String,
        #[arg(long)]
        allow_batch: bool,
        #[arg(long)]
        allow_interactive: bool,
    },
    /// Forward one JSON request from stdin to the local private socket.
    Request,
    #[command(hide = true)]
    ForkWorker {
        #[arg(long)]
        request_id: String,
    },
    #[command(hide = true)]
    Worker {
        #[arg(long)]
        execution: String,
    },
}
fn private_root(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        fs::DirBuilder::new().recursive(true).create(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    let metadata = fs::symlink_metadata(path)?;
    // UID comparison only; no privileged operation is performed.
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err("node root must be an owned, non-symlink directory with mode 0700".into());
    }
    Ok(fs::canonicalize(path)?)
}
fn lock_file(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?)
}
fn worker_lock(root: &Path, id: &str) -> Result<File> {
    if id.len() != 36 || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err("invalid execution ID".into());
    }
    lock_file(&root.join(format!("{id}.lock")))
}
fn worker_alive(root: &Path, id: &str) -> Result<bool> {
    let lock = worker_lock(root, id)?;
    match lock.try_lock_exclusive() {
        Ok(()) => Ok(false),
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(true),
        Err(e) => Err(e.into()),
    }
}
fn run() -> Result<()> {
    let cli = Cli::parse();
    let root = private_root(&cli.root)?;
    match cli.command {
        Command::Serve {
            host,
            allow_batch,
            allow_interactive,
        } => server::serve(&root, &host, allow_batch, allow_interactive),
        Command::ForkWorker { request_id } => branch::run(&root, &request_id),
        Command::Request => protocol::bridge(&root),
        Command::Worker { execution } => worker::run(&root, &execution),
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("runloom-node: {error}");
        std::process::exit(1);
    }
}
