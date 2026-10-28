use crate::{allowed, put, safe_path, Entry, Result, MAX_FILE};
use std::{
    collections::BTreeMap,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
pub(crate) fn git(repo: &Path, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut command = Command::new("/usr/bin/git");
    command
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-c")
        .arg("core.autocrlf=false")
        .arg("-C")
        .arg(repo)
        .args(args);
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(key);
    }
    command
        .env("GIT_AUTHOR_NAME", "Runloom")
        .env("GIT_AUTHOR_EMAIL", "snapshot@localhost")
        .env("GIT_COMMITTER_NAME", "Runloom")
        .env("GIT_COMMITTER_EMAIL", "snapshot@localhost");
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    if let Some(input) = input {
        child
            .stdin
            .take()
            .ok_or("git stdin missing")?
            .write_all(input)?;
    }
    let result = child.wait_with_output()?;
    if !result.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(result.stdout)
}
pub(crate) fn layer(
    repo: &Path,
    objects: &Path,
    index: bool,
    excluded: &mut Vec<String>,
) -> Result<BTreeMap<String, Entry>> {
    let raw = if index {
        git(repo, &["ls-files", "--stage", "-z"], None)?
    } else {
        git(repo, &["ls-tree", "-r", "-z", "HEAD"], None)?
    };
    let mut result = BTreeMap::new();
    let mut total = 0usize;
    for line in raw.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        let line = std::str::from_utf8(line)?;
        let (meta, path) = line.split_once('\t').ok_or("invalid Git record")?;
        safe_path(path)?;
        if !allowed(path) {
            excluded.push(path.into());
            continue;
        }
        let fields: Vec<_> = meta.split_whitespace().collect();
        if fields.len() != 3 {
            return Err("invalid Git fields".into());
        }
        let (mode, oid) = if index {
            if fields[2] != "0" {
                return Err("unresolved index conflicts".into());
            }
            (fields[0], fields[1])
        } else {
            if fields[1] != "blob" {
                return Err("submodules unsupported".into());
            }
            (fields[0], fields[2])
        };
        if !matches!(mode, "100644" | "100755" | "120000") {
            return Err("unsupported Git mode".into());
        }
        let size: String = String::from_utf8(git(repo, &["cat-file", "-s", oid], None)?)?;
        let size: usize = size.trim().parse()?;
        if size > MAX_FILE {
            return Err("file exceeds snapshot limit".into());
        }
        total += size;
        if total > 64 * 1024 * 1024 {
            return Err("layer exceeds 64 MiB".into());
        }
        let bytes = git(repo, &["cat-file", "blob", oid], None)?;
        let hash = put(objects, &bytes)?;
        if mode == "120000" {
            crate::safe_link(path, &bytes)?;
        }
        result.insert(
            path.into(),
            Entry {
                hash,
                mode: mode.into(),
            },
        );
    }
    Ok(result)
}
pub(crate) fn install_index(
    repo: &Path,
    layer: &BTreeMap<String, Entry>,
    objects: &Path,
) -> Result<()> {
    git(repo, &["read-tree", "--empty"], None)?;
    let mut records = Vec::new();
    for (path, entry) in layer {
        let bytes = crate::read_object(objects, &entry.hash)?;
        let oid = String::from_utf8(git(repo, &["hash-object", "-w", "--stdin"], Some(&bytes))?)?;
        records.extend_from_slice(format!("{} {}\t{}\0", entry.mode, oid.trim(), path).as_bytes());
    }
    git(
        repo,
        &["update-index", "-z", "--index-info"],
        Some(&records),
    )?;
    Ok(())
}
