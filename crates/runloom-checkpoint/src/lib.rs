//! Immutable, filtered Git workspace snapshots. Restore never overwrites a directory.
mod git;
use runloom_core::Work;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    os::unix::fs::{symlink, PermissionsExt},
    path::{Component, Path},
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const MAX_FILE: usize = 8 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub hash: String,
    pub mode: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub version: u32,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub created_ms: u64,
    pub work: Work,
    pub source_head: String,
    pub git_version: String,
    pub base: BTreeMap<String, Entry>,
    pub index: BTreeMap<String, Entry>,
    pub working: BTreeMap<String, Entry>,
    pub excluded: Vec<String>,
    pub native_session: Option<Entry>,
    #[serde(default)]
    pub native_dependencies: BTreeMap<String, Entry>,
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn hash_valid(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn safe_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.contains(['\0', '\n', '\r', '\\'])
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || path
            .split('/')
            .any(|p| p.eq_ignore_ascii_case(".git") || p.eq_ignore_ascii_case(".runloom"))
    {
        return Err("unsafe snapshot path".into());
    }
    Ok(())
}
fn allowed(path: &str) -> bool {
    !path.split('/').any(|p| {
        let p = p.to_lowercase();
        p.starts_with(".env")
            || matches!(
                p.as_str(),
                ".ssh"
                    | ".codex"
                    | ".claude"
                    | "auth.json"
                    | "credentials.json"
                    | "token.json"
                    | "id_rsa"
                    | "id_ed25519"
            )
            || p.ends_with(".pem")
            || p.ends_with(".key")
    })
}
fn safe_link(path: &str, bytes: &[u8]) -> Result<()> {
    let target = std::str::from_utf8(bytes)?;
    if target.is_empty() || Path::new(target).is_absolute() {
        return Err("absolute or empty symlink unsupported".into());
    }
    let mut depth = Path::new(path)
        .parent()
        .map(|p| p.components().count())
        .unwrap_or(0) as i64;
    for c in Path::new(target).components() {
        match c {
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return Err("symlink escapes snapshot".into());
                }
            }
            Component::Normal(s) => {
                if matches!(s.to_str(), Some(".git" | ".runloom")) {
                    return Err("symlink targets internal metadata".into());
                }
                depth += 1;
            }
            Component::CurDir => (),
            _ => return Err("invalid symlink".into()),
        }
    }
    Ok(())
}
fn put(objects: &Path, bytes: &[u8]) -> Result<String> {
    if bytes.len() > MAX_FILE {
        return Err("object exceeds 8 MiB".into());
    }
    fs::create_dir_all(objects)?;
    let hash = digest(bytes);
    let path = objects.join(&hash);
    if path.exists() {
        read_object(objects, &hash)?;
        return Ok(hash);
    }
    let mut temp = tempfile::NamedTempFile::new_in(objects)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    match temp.persist_noclobber(&path) {
        Ok(_) => (),
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
            read_object(objects, &hash)?;
        }
        Err(e) => return Err(e.into()),
    }
    Ok(hash)
}
fn read_object(objects: &Path, hash: &str) -> Result<Vec<u8>> {
    if !hash_valid(hash) {
        return Err("invalid object hash".into());
    }
    let path = objects.join(hash);
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_file() || meta.len() > MAX_FILE as u64 {
        return Err("invalid object file".into());
    }
    let bytes = fs::read(path)?;
    if digest(&bytes) != hash {
        return Err("corrupt object".into());
    }
    Ok(bytes)
}
fn working(
    repo: &Path,
    objects: &Path,
    paths: &BTreeSet<String>,
) -> Result<BTreeMap<String, Entry>> {
    let mut result = BTreeMap::new();
    let mut total = 0;
    for path in paths {
        let mut parent = repo.to_path_buf();
        let components: Vec<_> = Path::new(path).components().collect();
        for component in &components[..components.len() - 1] {
            parent.push(component);
            if parent.exists() && !fs::symlink_metadata(&parent)?.is_dir() {
                return Err("working parent is not a real directory".into());
            }
        }
        let file = repo.join(path);
        let meta = match fs::symlink_metadata(&file) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        let (bytes, mode) = if meta.file_type().is_symlink() {
            let value = fs::read_link(&file)?;
            let bytes = value.to_str().ok_or("non UTF8 link")?.as_bytes().to_vec();
            safe_link(path, &bytes)?;
            (bytes, "120000")
        } else if meta.is_file() {
            if meta.len() > MAX_FILE as u64 {
                return Err("file exceeds limit".into());
            }
            (
                fs::read(file)?,
                if meta.permissions().mode() & 0o111 != 0 {
                    "100755"
                } else {
                    "100644"
                },
            )
        } else {
            return Err("non-file working entry".into());
        };
        total += bytes.len();
        if total > 64 * 1024 * 1024 {
            return Err("working tree exceeds limit".into());
        }
        let hash = put(objects, &bytes)?;
        result.insert(
            path.clone(),
            Entry {
                hash,
                mode: mode.into(),
            },
        );
    }
    Ok(result)
}
pub fn capture(
    storage: &Path,
    work: Work,
    untracked: &[String],
    native: Option<&Path>,
) -> Result<String> {
    capture_with_parent(storage, work, untracked, native, None)
}
pub fn capture_with_parent(
    storage: &Path,
    work: Work,
    untracked: &[String],
    native: Option<&Path>,
    parent: Option<&str>,
) -> Result<String> {
    if let Some(parent) = parent {
        let previous = inspect(storage, parent)?;
        if previous.work.id != work.id || previous.work.identity != work.identity {
            return Err("parent checkpoint belongs to another work".into());
        }
    }
    let repo = fs::canonicalize(&work.identity.directory)?;
    let top = String::from_utf8(git::git(&repo, &["rev-parse", "--show-toplevel"], None)?)?;
    if fs::canonicalize(top.trim())? != repo {
        return Err("checkpoint requires repository root".into());
    }
    let objects = storage.join("objects");
    let mut excluded = Vec::new();
    let head = String::from_utf8(git::git(&repo, &["rev-parse", "HEAD"], None)?)?
        .trim()
        .to_owned();
    let base = git::layer(&repo, &objects, false, &mut excluded)?;
    let index = git::layer(&repo, &objects, true, &mut excluded)?;
    let mut paths: BTreeSet<_> = base.keys().chain(index.keys()).cloned().collect();
    for path in untracked {
        safe_path(path)?;
        if !allowed(path) {
            return Err("requested untracked path is excluded as sensitive".into());
        }
        paths.insert(path.clone());
    }
    let first = working(&repo, &objects, &paths)?;
    let native_session = if let Some(path) = native {
        let path = fs::canonicalize(path)?;
        let config = fs::canonicalize(&work.identity.config_root)?;
        if !path.starts_with(config)
            || path.extension().and_then(|s| s.to_str()) != Some("jsonl")
            || !path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.contains(&work.identity.native_id))
        {
            return Err("session path does not match registered identity".into());
        }
        let meta = fs::metadata(&path)?;
        if meta.len() > MAX_FILE as u64 {
            return Err("native session exceeds limit".into());
        }
        let before = fs::read(&path)?;
        let hash = put(&objects, &before)?;
        if before != fs::read(&path)? {
            return Err("native session changed during snapshot".into());
        }
        Some(Entry {
            hash,
            mode: "100644".into(),
        })
    } else {
        None
    };
    let native_dependencies = capture_dependencies(&objects, &work, native_session.as_ref())?;
    if first != working(&repo, &objects, &paths)?
        || index != git::layer(&repo, &objects, true, &mut Vec::new())?
        || head != String::from_utf8(git::git(&repo, &["rev-parse", "HEAD"], None)?)?.trim()
    {
        return Err("source changed during snapshot".into());
    }
    excluded.sort();
    excluded.dedup();
    let manifest = Manifest {
        version: 1,
        parent: parent.map(str::to_owned),
        created_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis()
            .try_into()?,
        work,
        source_head: head,
        git_version: String::from_utf8(git::git(&repo, &["--version"], None)?)?
            .trim()
            .into(),
        base,
        index,
        working: first,
        excluded,
        native_session,
        native_dependencies,
    };
    let data = serde_json::to_vec(&manifest)?;
    let manifests = storage.join("manifests");
    put(&manifests, &data)
}
pub fn inspect(storage: &Path, id: &str) -> Result<Manifest> {
    let data = read_object(&storage.join("manifests"), id)?;
    let manifest: Manifest = serde_json::from_slice(&data)?;
    if manifest.version != 1 {
        return Err("unsupported checkpoint version".into());
    }
    Ok(manifest)
}
pub fn restore(storage: &Path, id: &str, destination: &Path) -> Result<Manifest> {
    let manifest = inspect(storage, id)?;
    let objects = storage.join("objects");
    // Validate ALL artifacts before reserving a destination; never overwrite existing data.
    for layer in [&manifest.base, &manifest.index, &manifest.working] {
        let mut folded = BTreeSet::new();
        for (path, entry) in layer {
            safe_path(path)?;
            if !allowed(path) || !folded.insert(path.to_lowercase()) {
                return Err("excluded/case-colliding path".into());
            }
            for parent in Path::new(path).ancestors().skip(1) {
                if let Some(parent) = parent.to_str() {
                    if layer.contains_key(parent) {
                        return Err("file/directory collision".into());
                    }
                }
            }
            let bytes = read_object(&objects, &entry.hash)?;
            match entry.mode.as_str() {
                "100644" | "100755" => (),
                "120000" => safe_link(path, &bytes)?,
                _ => return Err("invalid mode".into()),
            }
        }
    }
    if let Some(native) = &manifest.native_session {
        read_object(&objects, &native.hash)?;
    }
    for (id, entry) in &manifest.native_dependencies {
        dependency_id(id)?;
        read_object(&objects, &entry.hash)?;
    }
    fs::create_dir(destination)?;
    fs::set_permissions(destination, fs::Permissions::from_mode(0o700))?;
    git::git(destination, &["init", "-q"], None)?;
    fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(destination.join(".git/info/exclude"))?
        .write_all(b"\n/.runloom/\n")?;
    git::install_index(destination, &manifest.base, &objects)?;
    let tree = String::from_utf8(git::git(destination, &["write-tree"], None)?)?;
    let commit = String::from_utf8(git::git(
        destination,
        &["commit-tree", tree.trim()],
        Some(b"Runloom filtered snapshot base\n"),
    )?)?;
    git::git(destination, &["update-ref", "HEAD", commit.trim()], None)?;
    git::install_index(destination, &manifest.index, &objects)?;
    install_working(destination, &manifest, &objects)?;
    Ok(manifest)
}

fn install_working(destination: &Path, manifest: &Manifest, objects: &Path) -> Result<()> {
    for (path, entry) in &manifest.working {
        let file = destination.join(path);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = read_object(objects, &entry.hash)?;
        if entry.mode == "120000" {
            symlink(std::str::from_utf8(&bytes)?, &file)?;
        } else {
            fs::write(&file, bytes)?;
            fs::set_permissions(
                file,
                fs::Permissions::from_mode(if entry.mode == "100755" { 0o700 } else { 0o600 }),
            )?;
        }
    }
    let metadata = destination.join(".runloom");
    fs::create_dir(&metadata)?;
    fs::write(
        metadata.join("checkpoint.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    if let Some(native) = &manifest.native_session {
        fs::write(
            metadata.join("session.jsonl"),
            read_object(objects, &native.hash)?,
        )?;
    }
    if !manifest.native_dependencies.is_empty() {
        let folder = metadata.join("native-dependencies");
        fs::create_dir(&folder)?;
        for (id, entry) in &manifest.native_dependencies {
            dependency_id(id)?;
            fs::write(
                folder.join(format!("{id}.jsonl")),
                read_object(objects, &entry.hash)?,
            )?;
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct CheckpointSummary {
    pub id: String,
    pub parent: Option<String>,
    pub created_ms: u64,
    pub native_session: bool,
}
pub fn list(storage: &Path, workspace: &str, work: &str) -> Result<Vec<CheckpointSummary>> {
    let folder = storage.join("manifests");
    if !folder.exists() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for (count, entry) in fs::read_dir(folder)?.enumerate() {
        if count >= 10000 {
            return Err("manifest listing limit reached".into());
        }
        let entry = entry?;
        let id = entry
            .file_name()
            .into_string()
            .map_err(|_| "invalid manifest filename")?;
        if !hash_valid(&id) {
            continue;
        }
        let m = inspect(storage, &id)?;
        if m.work.id == work && m.work.identity.workspace == workspace {
            result.push(CheckpointSummary {
                id,
                parent: m.parent,
                created_ms: m.created_ms,
                native_session: m.native_session.is_some(),
            });
        }
        if result.len() > 1000 {
            return Err("too many checkpoints; pagination needed".into());
        }
    }
    result.sort_by(|a, b| a.created_ms.cmp(&b.created_ms).then(a.id.cmp(&b.id)));
    Ok(result)
}

/// A linked Git worktree backed by a private filtered repository. No source refs/index
/// are touched. The backing repository must be retained for the worktree lifetime.
pub fn restore_worktree(
    storage: &Path,
    id: &str,
    destination: &Path,
    backing: &Path,
    branch: &str,
) -> Result<Manifest> {
    if !destination.is_absolute()
        || destination.try_exists()?
        || fs::symlink_metadata(destination).is_ok()
    {
        return Err("worktree destination must be a new absolute path".into());
    }
    git::git(storage, &["check-ref-format", "--branch", branch], None)?;
    let manifest = restore(storage, id, backing)?;
    git::git(
        backing,
        &[
            "worktree",
            "add",
            "--no-checkout",
            "-b",
            branch,
            destination.to_str().ok_or("non UTF8 destination")?,
            "HEAD",
        ],
        None,
    )?;
    fs::set_permissions(destination, fs::Permissions::from_mode(0o700))?;
    git::install_index(destination, &manifest.index, &storage.join("objects"))?;
    install_working(destination, &manifest, &storage.join("objects"))?;
    Ok(manifest)
}

/// Bounded identity lookup, never chooses by cwd, recency, or file size.
pub fn discover_native(work: &Work) -> Result<std::path::PathBuf> {
    let root = fs::canonicalize(&work.identity.config_root)?;
    let folders: &[&str] = match work.identity.provider.as_str() {
        "codex" => &["sessions", "archived_sessions"],
        "claude-code" => &["projects"],
        _ => return Err("native discovery unavailable for provider".into()),
    };
    let mut pending: Vec<_> = folders.iter().map(|name| (root.join(name), 0)).collect();
    let mut found = Vec::new();
    let mut visited = 0;
    while let Some((folder, depth)) = pending.pop() {
        if !folder.exists() {
            continue;
        }
        if !fs::symlink_metadata(&folder)?.is_dir() {
            return Err("session directory must not be a symlink".into());
        }
        for entry in fs::read_dir(folder)? {
            visited += 1;
            if visited > 50000 {
                return Err("native lookup limit reached; specify exact transcript path".into());
            }
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() && depth < 4 {
                pending.push((entry.path(), depth + 1));
            } else if kind.is_file() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                let matched = if work.identity.provider == "codex" {
                    name.starts_with("rollout-")
                        && name.ends_with(&format!("-{}.jsonl", work.identity.native_id))
                } else {
                    name == format!("{}.jsonl", work.identity.native_id)
                };
                if matched {
                    found.push(entry.path());
                }
            }
        }
    }
    if found.len() != 1 {
        return Err(
            "native transcript lookup must match exactly one identity; specify a path".into(),
        );
    }
    Ok(found.remove(0))
}

fn dependency_id(id: &str) -> Result<()> {
    if id.len() != 36
        || !id.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
    {
        return Err("invalid native dependency identity".into());
    }
    Ok(())
}
fn capture_dependencies(
    objects: &Path,
    work: &Work,
    native: Option<&Entry>,
) -> Result<BTreeMap<String, Entry>> {
    let mut result = BTreeMap::new();
    if work.identity.provider != "codex" || native.is_none() {
        return Ok(result);
    }
    let mut bytes = read_object(objects, &native.ok_or("native missing")?.hash)?;
    let mut seen = BTreeSet::from([work.identity.native_id.clone()]);
    let mut sources = Vec::new();
    let mut total = bytes.len();
    loop {
        let first = bytes
            .split(|b| *b == b'\n')
            .next()
            .ok_or("empty native session")?;
        let meta: serde_json::Value = serde_json::from_slice(first)?;
        let Some(id) = meta["payload"]["history_base"]["thread_id"].as_str() else {
            break;
        };
        dependency_id(id)?;
        if !seen.insert(id.to_string()) || seen.len() > 32 {
            return Err("native dependency cycle/depth limit".into());
        }
        let mut parent = work.clone();
        parent.identity.native_id = id.into();
        let path = discover_native(&parent)?;
        if fs::metadata(&path)?.len() > MAX_FILE as u64 {
            return Err("native dependency exceeds limit".into());
        }
        let next = fs::read(&path)?;
        total += next.len();
        if total > 64 * 1024 * 1024 {
            return Err("native dependency bundle exceeds limit".into());
        }
        let hash = put(objects, &next)?;
        result.insert(
            id.to_owned(),
            Entry {
                hash: hash.clone(),
                mode: "100644".into(),
            },
        );
        sources.push((path, hash));
        bytes = next;
    }
    for (path, hash) in sources {
        if digest(&fs::read(path)?) != hash {
            return Err("native dependency changed during capture".into());
        }
    }
    Ok(result)
}

#[cfg(test)]
mod digest_compatibility {
    #[test]
    fn stable_sha256_object_addresses() {
        assert_eq!(
            super::digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            super::digest(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
