use runloom_core::{CommandSpec, Error, NewWork, Store};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
fn identity(root: &std::path::Path, name: &str, native: &str) -> NewWork {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    NewWork {
        workspace: "p".into(),
        host: "h".into(),
        provider: "fixture".into(),
        config_root: root.to_str().unwrap().into(),
        native_id: native.into(),
        directory: dir.to_str().unwrap().into(),
        title: name.into(),
    }
}
fn command(work: &runloom_core::Work) -> CommandSpec {
    CommandSpec {
        executable: "/bin/sh".into(),
        args: vec![],
        directory: work.identity.directory.clone(),
        interactive: false,
        tty: false,
        env: Default::default(),
    }
}
#[test]
fn reservation_fences_overlap_and_native_identity_without_blocking_other_work() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("state.db");
    let mut s = Store::open(&db).unwrap();
    let a = s.create_work(identity(root.path(), "a", "a")).unwrap();
    let nested = s
        .create_work(identity(root.path(), "a/nested", "nested"))
        .unwrap();
    let alias = s
        .create_work(identity(root.path(), "alias", "a"))
        .unwrap_err(); // duplicate session within scope
    assert!(matches!(alias, Error::DuplicateSession));
    let mut alias = identity(root.path(), "alias", "a");
    alias.workspace = "other".into();
    let alias = s.create_work(alias).unwrap();
    let b = s.create_work(identity(root.path(), "b", "b")).unwrap();
    let proof = s
        .acquire_lease("p", &a.id, "owner", 30, None)
        .unwrap()
        .proof;
    let bproof = s
        .acquire_lease("p", &b.id, "other", 30, None)
        .unwrap()
        .proof;
    let (entered, waiting) = mpsc::channel();
    let (finish, release) = mpsc::channel();
    let id = a.id.clone();
    let db2 = db.clone();
    let thread = std::thread::spawn(move || {
        let mut s = Store::open(db2).unwrap();
        s.with_idle_work("p", &id, &proof, |_| {
            entered.send(()).unwrap();
            release.recv().unwrap();
            Ok(())
        })
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let now = Instant::now();
    s.append_event(
        "p",
        &b.id,
        "out",
        "execution.output",
        serde_json::json!({"text":"still running"}),
    )
    .unwrap();
    s.renew_lease("p", &b.id, &bproof, 30).unwrap();
    assert!(now.elapsed() < Duration::from_secs(2));
    for w in [&a, &nested, &alias] {
        assert!(matches!(
            s.admit_execution(&w.identity.workspace, &w.id, "blocked", command(w)),
            Err(Error::WorkspaceReserved(_))
        ));
    }
    s.admit_execution("p", &b.id, "allowed", command(&b))
        .unwrap();
    finish.send(()).unwrap();
    thread.join().unwrap().unwrap();
    s.admit_execution("p", &a.id, "after", command(&a)).unwrap();
}
#[test]
fn ordinary_error_releases_but_panic_and_reopen_keep_reservation() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("db");
    let mut s = Store::open(&db).unwrap();
    let a = s.create_work(identity(root.path(), "a", "a")).unwrap();
    let proof = s
        .acquire_lease("p", &a.id, "owner", 30, None)
        .unwrap()
        .proof;
    assert!(s
        .with_idle_work::<()>("p", &a.id, &proof, |_| Err("fixture error".into()))
        .is_err());
    s.with_idle_work("p", &a.id, &proof, |_| Ok(())).unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.with_idle_work::<()>(
            "p",
            &a.id,
            &proof,
            |_| panic!("fixture crash")
        )))
        .is_err()
    );
    drop(s);
    let mut s = Store::open(&db).unwrap();
    assert!(matches!(
        s.admit_execution("p", &a.id, "never-replay", command(&a)),
        Err(Error::WorkspaceReserved(_))
    ));
    assert!(matches!(
        s.with_idle_work("p", &a.id, &proof, |_| Ok(())),
        Err(Error::WorkspaceReserved(_))
    ));
}
#[test]
fn migrates_v3_to_reservations() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("db");
    let c = rusqlite::Connection::open(&db).unwrap();
    c.execute_batch(include_str!("../src/schema.sql")).unwrap();
    c.execute_batch(include_str!("../src/schema-v2.sql"))
        .unwrap();
    c.execute_batch(include_str!("../src/schema-v3.sql"))
        .unwrap();
    drop(c);
    Store::open(&db).unwrap();
    let c = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        runloom_core::SCHEMA_VERSION
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM checkpoint_reservations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn core_directory_identity_and_legacy_execution_paths_are_fenced() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("db");
    let mut s = Store::open(&db).unwrap();
    let a = s.create_work(identity(root.path(), "a", "a")).unwrap();
    let b = s.create_work(identity(root.path(), "b", "b")).unwrap();
    assert!(matches!(
        s.admit_execution("p", &b.id, "mismatched", command(&a)),
        Err(Error::Invalid(_))
    ));
    let (execution, _) = s
        .admit_execution("p", &b.id, "legacy", command(&b))
        .unwrap();
    // Old core clients could admit a different directory; preserve protection
    // when opening an existing database containing that command.
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE executions SET command_json=?1 WHERE id=?2",
            rusqlite::params![serde_json::to_string(&command(&a)).unwrap(), execution.id],
        )
        .unwrap();
    let proof = s
        .acquire_lease("p", &a.id, "owner", 30, None)
        .unwrap()
        .proof;
    assert!(matches!(
        s.with_idle_work("p", &a.id, &proof, |_| Ok(())),
        Err(Error::ActiveExecution)
    ));
}

#[test]
fn restore_destination_is_reserved_before_it_exists() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("db");
    let mut s = Store::open(&db).unwrap();
    let a = s
        .create_work(identity(root.path(), "source", "source"))
        .unwrap();
    let proof = s
        .acquire_lease("p", &a.id, "owner", 30, None)
        .unwrap()
        .proof;
    let destination = root.path().join("restoring");
    let (entered, waiting) = mpsc::channel();
    let (finish, release) = mpsc::channel();
    let database = db.clone();
    let target = destination.clone();
    let thread = std::thread::spawn(move || {
        let mut store = Store::open(database).unwrap();
        store.with_idle_work_paths("p", &a.id, &proof, &[&target], |_| {
            std::fs::create_dir(&target)?;
            entered.send(()).unwrap();
            release.recv().unwrap();
            std::fs::write(target.join("restored"), b"checkpoint")?;
            Ok(())
        })
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let b = s
        .create_work(identity(root.path(), "restoring", "new-writer"))
        .unwrap();
    assert!(matches!(
        s.admit_execution("p", &b.id, "during-restore", command(&b)),
        Err(Error::WorkspaceReserved(_))
    ));
    finish.send(()).unwrap();
    thread.join().unwrap().unwrap();
    assert_eq!(
        std::fs::read(destination.join("restored")).unwrap(),
        b"checkpoint"
    );
    s.admit_execution("p", &b.id, "after-restore", command(&b))
        .unwrap();
}

#[test]
fn branch_destination_guard_allows_source_continuation_but_blocks_target() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("db");
    let mut s = Store::open(&db).unwrap();
    let a = s
        .create_work(identity(root.path(), "source", "source"))
        .unwrap();
    let (entered, waiting) = mpsc::channel();
    let (finish, release) = mpsc::channel();
    let database = db.clone();
    let target = root.path().join("branch");
    let id = a.id.clone();
    let thread = std::thread::spawn(move || {
        let mut store = Store::open(database).unwrap();
        store.with_reserved_destination("p", &id, &target, || {
            std::fs::create_dir(&target)?;
            entered.send(()).unwrap();
            release.recv().unwrap();
            Ok(())
        })
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let b = s
        .create_work(identity(root.path(), "branch", "branch"))
        .unwrap();
    s.admit_execution("p", &a.id, "source-continues", command(&a))
        .unwrap();
    assert!(matches!(
        s.admit_execution("p", &b.id, "during-branch", command(&b)),
        Err(Error::WorkspaceReserved(_))
    ));
    finish.send(()).unwrap();
    thread.join().unwrap().unwrap();
    s.admit_execution("p", &b.id, "after-branch", command(&b))
        .unwrap();
}
