use runloom_core::{Error, NewWork, Store};
use serde_json::json;
use std::sync::{Arc, Barrier};
fn input() -> NewWork {
    NewWork {
        workspace: "personal".into(),
        host: "mini".into(),
        provider: "fixture-agent".into(),
        config_root: "/fixture/profile".into(),
        native_id: "native-1".into(),
        directory: "/fixture/project".into(),
        title: "Fixture work".into(),
    }
}
#[test]
fn persistence_identity_and_workspace_scope() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    let mut s = Store::open(&path).unwrap();
    let work = s.create_work(input()).unwrap();
    assert!(matches!(
        s.create_work(input()),
        Err(Error::DuplicateSession)
    ));
    for field in ["host", "provider", "config_root", "native_id", "workspace"] {
        let mut i = input();
        match field {
            "host" => i.host = "other".into(),
            "provider" => i.provider = "other".into(),
            "config_root" => i.config_root = "other".into(),
            "native_id" => i.native_id = "other".into(),
            _ => i.workspace = "champ".into(),
        }
        s.create_work(i).unwrap();
    }
    assert!(matches!(
        s.events("champ", &work.id, 0, 100),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        s.append_event("champ", &work.id, "x", "note", json!({})),
        Err(Error::NotFound)
    ));
    drop(s);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.list("personal").unwrap().len(), 5);
    assert_eq!(reopened.list("champ").unwrap().len(), 1);
    assert_eq!(
        reopened.events("personal", &work.id, 0, 100).unwrap()[0].kind,
        "work.registered"
    );
}
#[test]
fn concurrent_sequence_and_idempotency() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    let mut s = Store::open(&path).unwrap();
    let w = s.create_work(input()).unwrap();
    let stores: Vec<_> = (0..8).map(|_| Store::open(&path).unwrap()).collect();
    let gate = Arc::new(Barrier::new(8));
    let threads: Vec<_> = stores
        .into_iter()
        .enumerate()
        .map(|(i, mut db)| {
            let gate = gate.clone();
            let id = w.id.clone();
            std::thread::spawn(move || {
                gate.wait();
                db.append_event(
                    "personal",
                    &id,
                    "same-request",
                    "note",
                    json!({"same":true}),
                )
                .unwrap();
                db.append_event(
                    "personal",
                    &id,
                    &format!("request-{i}"),
                    "note",
                    json!({"i":i}),
                )
                .unwrap();
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let events = s.events("personal", &w.id, 0, 100).unwrap();
    assert_eq!(events.len(), 10);
    assert_eq!(
        events.iter().map(|e| e.sequence).collect::<Vec<_>>(),
        (1..=10).collect::<Vec<_>>()
    );
    assert_eq!(s.events("personal", &w.id, 8, 1).unwrap()[0].sequence, 9);
    assert!(matches!(
        s.append_event(
            "personal",
            &w.id,
            "same-request",
            "note",
            json!({"different":true})
        ),
        Err(Error::RequestConflict)
    ));
    assert_eq!(s.events("personal", &w.id, 0, 100).unwrap().len(), 10);
}
#[test]
fn invalid_input_does_not_write_partial_work() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("state.db")).unwrap();
    let mut i = input();
    i.native_id = "\n".into();
    assert!(s.create_work(i).is_err());
    assert!(s.list("personal").unwrap().is_empty());
    let w = s.create_work(input()).unwrap();
    assert!(s.events("personal", &w.id, -1, 10).is_err());
    assert!(s.events("personal", &w.id, 0, 1001).is_err());
    assert!(s
        .append_event("personal", &w.id, "id", "work.registered", json!({}))
        .is_err());
    assert!(s
        .append_event("personal", &w.id, "id", "note", json!("x".repeat(65537)))
        .is_err());
    assert_eq!(s.events("personal", &w.id, 0, 100).unwrap().len(), 1);
}
#[test]
fn rejects_newer_schema_without_downgrading() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.pragma_update(None, "user_version", 999).unwrap();
    assert!(matches!(Store::open(&path), Err(Error::FutureSchema(999))));
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        999
    );
}
#[test]
fn interrupted_transaction_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    Store::open(&path).unwrap();
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("BEGIN IMMEDIATE; INSERT INTO workspaces(id) VALUES ('uncommitted');")
            .unwrap();
    }
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM workspaces", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    Store::open(&path).unwrap();
}

#[test]
fn migration_preserves_v1_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(include_str!("../src/schema.sql")).unwrap();
    db.execute("INSERT INTO workspaces(id) VALUES ('legacy')", [])
        .unwrap();
    drop(db);
    Store::open(&path).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        i64::from(runloom_core::SCHEMA_VERSION)
    );
    assert_eq!(
        db.query_row("SELECT id FROM workspaces", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "legacy"
    );
}

#[test]
fn execution_admission_is_atomic_and_scoped() {
    use runloom_core::CommandSpec;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    let mut s = Store::open(&path).unwrap();
    let w = s.create_work(input()).unwrap();
    let cmd = CommandSpec {
        executable: "/bin/sh".into(),
        args: vec!["-c".into(), "echo test".into()],
        directory: "/fixture/project".into(),
        interactive: false,
        tty: false,
        env: Default::default(),
    };
    let stores: Vec<_> = (0..8).map(|_| Store::open(&path).unwrap()).collect();
    let gate = Arc::new(Barrier::new(8));
    let threads: Vec<_> = stores
        .into_iter()
        .map(|mut store| {
            let gate = gate.clone();
            let id = w.id.clone();
            let cmd = cmd.clone();
            std::thread::spawn(move || {
                gate.wait();
                store.admit_execution("personal", &id, "same", cmd).unwrap()
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.1).count(), 1);
    let id = &results[0].0.id;
    assert!(results.iter().all(|r| r.0.id == *id));
    assert!(matches!(
        s.admit_execution("personal", &w.id, "other", cmd.clone()),
        Err(Error::ActiveExecution)
    ));
    assert!(matches!(
        s.scoped_execution("champ", &w.id, id),
        Err(Error::NotFound)
    ));
    s.claim_execution(id).unwrap();
    assert!(matches!(s.claim_execution(id), Err(Error::StateConflict)));
    s.finish_execution(id, Some(7), None).unwrap();
    assert!(
        !s.admit_execution("personal", &w.id, "same", cmd.clone())
            .unwrap()
            .1
    );
    let mut changed = cmd.clone();
    changed.args.push("different".into());
    assert!(matches!(
        s.admit_execution("personal", &w.id, "same", changed),
        Err(Error::RequestConflict)
    ));
    assert!(s.finish_execution(id, Some(0), None).is_err());
    assert!(s.admit_execution("personal", &w.id, "next", cmd).unwrap().1);
}

#[test]
fn workspace_labels_cannot_duplicate_native_session_writer() {
    use runloom_core::CommandSpec;
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("state.db")).unwrap();
    let first = store.create_work(input()).unwrap();
    let mut other = input();
    other.workspace = "champ".into();
    let second = store.create_work(other).unwrap();
    let command = CommandSpec {
        executable: "/bin/sh".into(),
        args: vec![],
        directory: "/fixture/project".into(),
        interactive: false,
        tty: false,
        env: Default::default(),
    };
    store
        .admit_execution("personal", &first.id, "first", command.clone())
        .unwrap();
    assert!(matches!(
        store.admit_execution("champ", &second.id, "second", command),
        Err(Error::ActiveExecution)
    ));
}
