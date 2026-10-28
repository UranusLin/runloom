use runloom_core::{NewWork, Work};
use std::{fs, path::Path, process::Command};
fn git(path: &Path, args: &[&str]) -> String {
    let out = Command::new("/usr/bin/git")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
fn work(repo: &Path) -> Work {
    Work {
        id: "fixture".into(),
        state: "registered".into(),
        identity: NewWork {
            workspace: "p".into(),
            host: "h".into(),
            provider: "fixture".into(),
            config_root: "/fixture".into(),
            native_id: "id".into(),
            directory: repo.to_str().unwrap().into(),
            title: "fixture".into(),
        },
    }
}
fn init(repo: &Path) {
    fs::create_dir(repo).unwrap();
    git(repo, &["init", "-q"]);
    git(repo, &["config", "user.email", "fixture@localhost"]);
    git(repo, &["config", "user.name", "Fixture"]);
}
#[test]
fn preserves_staged_unstaged_deleted_untracked_and_excludes_secrets() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init(&repo);
    fs::write(repo.join("a.txt"), "base\n").unwrap();
    fs::write(repo.join("gone.txt"), "gone").unwrap();
    fs::write(repo.join(".env"), "SECRET_MUST_NOT_BE_STORED").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    fs::write(repo.join("a.txt"), "staged\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    fs::write(repo.join("a.txt"), "working\n").unwrap();
    fs::remove_file(repo.join("gone.txt")).unwrap();
    fs::write(repo.join("new.txt"), "chosen new").unwrap();
    fs::write(repo.join("not-selected.txt"), "omit").unwrap();
    let storage = temp.path().join("store");
    let id = runloom_checkpoint::capture(&storage, work(&repo), &["new.txt".into()], None).unwrap();
    let dest = temp.path().join("restored");
    let manifest = runloom_checkpoint::restore(&storage, &id, &dest).unwrap();
    assert_eq!(fs::read_to_string(dest.join("a.txt")).unwrap(), "working\n");
    assert_eq!(git(&dest, &["show", ":a.txt"]), "staged\n");
    assert_eq!(git(&dest, &["show", "HEAD:a.txt"]), "base\n");
    assert!(!dest.join("gone.txt").exists());
    assert!(!dest.join(".env").exists());
    assert!(!dest.join("not-selected.txt").exists());
    assert_eq!(
        fs::read_to_string(dest.join("new.txt")).unwrap(),
        "chosen new"
    );
    assert!(manifest.excluded.contains(&".env".into()));
    for entry in fs::read_dir(storage.join("objects")).unwrap() {
        assert!(
            !String::from_utf8_lossy(&fs::read(entry.unwrap().path()).unwrap())
                .contains("SECRET_MUST_NOT_BE_STORED")
        );
    }
    assert!(runloom_checkpoint::restore(&storage, &id, &dest).is_err());
    assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "working\n");
}
#[test]
fn corruption_and_unsafe_links_fail_before_destination_creation() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init(&repo);
    fs::write(repo.join("a"), "content").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let storage = temp.path().join("store");
    let id = runloom_checkpoint::capture(&storage, work(&repo), &[], None).unwrap();
    let manifest = runloom_checkpoint::inspect(&storage, &id).unwrap();
    fs::write(
        storage.join("objects").join(&manifest.working["a"].hash),
        "corrupt",
    )
    .unwrap();
    let dest = temp.path().join("restore");
    assert!(runloom_checkpoint::restore(&storage, &id, &dest).is_err());
    assert!(!dest.exists());
    std::os::unix::fs::symlink("../../outside", repo.join("link")).unwrap();
    assert!(runloom_checkpoint::capture(
        &temp.path().join("other"),
        work(&repo),
        &["link".into()],
        None
    )
    .is_err());
    assert!(runloom_checkpoint::capture(
        &temp.path().join("other"),
        work(&repo),
        &["../outside".into()],
        None
    )
    .is_err());
}

#[test]
fn checkpoint_lineage_and_native_export_preserve_source() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init(&repo);
    fs::write(repo.join("a"), "v1").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let config = temp.path().join("profile");
    fs::create_dir(&config).unwrap();
    let session = config.join("native-id.jsonl");
    fs::write(&session, b"{\"fixture\":true}\n").unwrap();
    let mut w = work(&repo);
    w.identity.config_root = config.to_str().unwrap().into();
    w.identity.native_id = "native-id".into();
    let storage = temp.path().join("store");
    let first = runloom_checkpoint::capture(&storage, w.clone(), &[], Some(&session)).unwrap();
    fs::write(repo.join("a"), "v2").unwrap();
    let second =
        runloom_checkpoint::capture_with_parent(&storage, w, &[], Some(&session), Some(&first))
            .unwrap();
    assert_eq!(
        runloom_checkpoint::inspect(&storage, &second)
            .unwrap()
            .parent,
        Some(first)
    );
    assert_eq!(
        runloom_checkpoint::list(&storage, "p", "fixture")
            .unwrap()
            .len(),
        2
    );
    let dest = temp.path().join("restored");
    runloom_checkpoint::restore(&storage, &second, &dest).unwrap();
    assert_eq!(
        fs::read(dest.join(".runloom/session.jsonl")).unwrap(),
        fs::read(session).unwrap()
    );
    assert_eq!(fs::read_to_string(dest.join("a")).unwrap(), "v2");
}

#[test]
fn linked_worktree_keeps_index_and_source_independent() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init(&repo);
    fs::write(repo.join("a"), "base").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    fs::write(repo.join("a"), "staged").unwrap();
    git(&repo, &["add", "."]);
    fs::write(repo.join("a"), "working").unwrap();
    let original = git(&repo, &["status", "--porcelain=v1"]);
    let storage = temp.path().join("store");
    let checkpoint = runloom_checkpoint::capture(&storage, work(&repo), &[], None).unwrap();
    let dest = temp.path().join("branch");
    let backing = temp.path().join("backing");
    runloom_checkpoint::restore_worktree(&storage, &checkpoint, &dest, &backing, "runloom/test")
        .unwrap();
    assert!(dest.join(".git").is_file());
    assert_eq!(
        git(&dest, &["branch", "--show-current"]).trim(),
        "runloom/test"
    );
    assert_eq!(git(&dest, &["show", ":a"]), "staged");
    assert_eq!(git(&dest, &["show", "HEAD:a"]), "base");
    assert_eq!(fs::read_to_string(dest.join("a")).unwrap(), "working");
    fs::write(dest.join("a"), "branch change").unwrap();
    git(&dest, &["add", "a"]);
    assert_eq!(fs::read_to_string(repo.join("a")).unwrap(), "working");
    assert_eq!(git(&repo, &["show", ":a"]), "staged");
    assert_eq!(git(&repo, &["status", "--porcelain=v1"]), original);
    assert!(
        runloom_checkpoint::restore_worktree(&storage, &checkpoint, &dest, &backing, "other")
            .is_err()
    );
}

#[test]
fn native_dependencies_are_bundled_and_verified_before_restore() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init(&repo);
    fs::write(repo.join("a"), "base").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let profile = temp.path().join("profile");
    fs::create_dir_all(profile.join("sessions")).unwrap();
    let parent = "00000000-0000-4000-8000-000000000001";
    let child = "00000000-0000-4000-8000-000000000002";
    let parent_file = profile.join(format!("sessions/rollout-{parent}.jsonl"));
    fs::write(
        &parent_file,
        format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{parent}\"}}}}\n"),
    )
    .unwrap();
    let child_file = profile.join(format!("sessions/rollout-{child}.jsonl"));
    fs::write(&child_file, format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{child}\",\"history_base\":{{\"thread_id\":\"{parent}\",\"end_ordinal_exclusive\":1}}}}}}\n")).unwrap();
    let mut w = work(&repo);
    w.identity.provider = "codex".into();
    w.identity.native_id = child.into();
    w.identity.config_root = profile.to_str().unwrap().into();
    let storage = temp.path().join("store");
    let id = runloom_checkpoint::capture(&storage, w.clone(), &[], Some(&child_file)).unwrap();
    let manifest = runloom_checkpoint::inspect(&storage, &id).unwrap();
    assert_eq!(manifest.native_dependencies.len(), 1);
    fs::remove_file(&parent_file).unwrap();
    let dest = temp.path().join("restore");
    runloom_checkpoint::restore(&storage, &id, &dest).unwrap();
    assert!(dest
        .join(format!(".runloom/native-dependencies/{parent}.jsonl"))
        .is_file());
    fs::write(
        storage
            .join("objects")
            .join(&manifest.native_dependencies[parent].hash),
        "corrupt",
    )
    .unwrap();
    let rejected = temp.path().join("rejected");
    assert!(runloom_checkpoint::restore(&storage, &id, &rejected).is_err());
    assert!(!rejected.exists());
    assert!(runloom_checkpoint::capture(&storage, w, &[], Some(&child_file)).is_err());
}
