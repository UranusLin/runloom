use runloom_core::{CommandSpec, Error, InputRequest, NewWork, Store};
use std::sync::{Arc, Barrier};
fn setup() -> (tempfile::TempDir, Store, String) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("state.db")).unwrap();
    let w = store
        .create_work(NewWork {
            workspace: "p".into(),
            host: "h".into(),
            provider: "fixture".into(),
            config_root: "/fixture".into(),
            native_id: "n".into(),
            directory: "/tmp".into(),
            title: "test".into(),
        })
        .unwrap();
    (dir, store, w.id)
}
#[test]
fn only_one_owner_and_epoch_fences_old_tokens() {
    let (dir, mut store, work) = setup();
    let gate = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let mut s = Store::open(dir.path().join("state.db")).unwrap();
            let gate = gate.clone();
            let work = work.clone();
            std::thread::spawn(move || {
                gate.wait();
                s.acquire_lease("p", &work, &format!("client-{i}"), 30, None)
            })
        })
        .collect();
    let grants: Vec<_> = handles
        .into_iter()
        .filter_map(|h| h.join().unwrap().ok())
        .collect();
    assert_eq!(grants.len(), 1);
    let first = &grants[0];
    let next = store
        .acquire_lease("p", &work, "second", 30, Some(first.proof.epoch))
        .unwrap();
    assert!(next.proof.epoch > first.proof.epoch);
    assert!(matches!(
        store.renew_lease("p", &work, &first.proof, 30),
        Err(Error::StaleLease)
    ));
    assert!(store
        .acquire_lease("p", &work, "third", 30, Some(first.proof.epoch))
        .is_err());
    store.release_lease("p", &work, &next.proof).unwrap();
    let third = store.acquire_lease("p", &work, "third", 30, None).unwrap();
    assert!(third.proof.epoch > next.proof.epoch);
}
#[test]
fn input_retries_and_takeover_between_prepare_and_delivery() {
    let (_dir, mut store, work) = setup();
    let grant = store.acquire_lease("p", &work, "one", 30, None).unwrap();
    let cmd = CommandSpec {
        executable: "/bin/sh".into(),
        args: vec![],
        directory: "/tmp".into(),
        interactive: true,
        tty: false,
        env: Default::default(),
    };
    assert!(store.admit_execution("p", &work, "r", cmd.clone()).is_err());
    let (e, _) = store
        .admit_guarded("p", &work, "r", cmd, Some(&grant.proof))
        .unwrap();
    store.claim_execution(&e.id).unwrap();
    let mut input = InputRequest {
        workspace: "p".into(),
        work: work.clone(),
        execution: e.id,
        request_id: "input-1".into(),
        proof: grant.proof.clone(),
        bytes: b"test\n".to_vec(),
    };
    assert!(store.prepare_input(&input).unwrap());
    assert!(matches!(
        store.prepare_input(&input),
        Err(Error::UncertainInput)
    ));
    let next = store
        .acquire_lease("p", &work, "two", 30, Some(grant.proof.epoch))
        .unwrap();
    assert!(matches!(
        store.deliver_input(&input, |_| panic!("stale write")),
        Err(Error::StaleLease)
    ));
    input.proof = next.proof;
    input.request_id = "input-2".into();
    assert!(store.prepare_input(&input).unwrap());
    let mut writes = 0;
    store
        .deliver_input(&input, |_| {
            writes += 1;
            Ok(())
        })
        .unwrap();
    assert!(!store.prepare_input(&input).unwrap());
    assert_eq!(writes, 1);
    input.bytes = b"changed".to_vec();
    assert!(matches!(
        store.prepare_input(&input),
        Err(Error::RequestConflict)
    ));
}
#[test]
fn expired_lease_cannot_renew_or_write() {
    let (dir, mut store, work) = setup();
    let grant = store.acquire_lease("p", &work, "one", 30, None).unwrap();
    let db = rusqlite::Connection::open(dir.path().join("state.db")).unwrap();
    db.execute("UPDATE leases SET expires_ms=0", []).unwrap();
    assert!(matches!(
        store.renew_lease("p", &work, &grant.proof, 30),
        Err(Error::StaleLease)
    ));
    assert!(
        store
            .acquire_lease("p", &work, "next", 30, None)
            .unwrap()
            .proof
            .epoch
            > grant.proof.epoch
    );
}

#[test]
fn checkpoint_guard_blocks_other_work_in_same_directory() {
    let (_dir, mut store, work) = setup();
    let grant = store.acquire_lease("p", &work, "one", 30, None).unwrap();
    let other = store
        .create_work(NewWork {
            workspace: "p".into(),
            host: "h".into(),
            provider: "fixture".into(),
            config_root: "/fixture".into(),
            native_id: "other".into(),
            directory: "/tmp".into(),
            title: "other".into(),
        })
        .unwrap();
    let cmd = CommandSpec {
        executable: "/bin/sh".into(),
        args: vec![],
        directory: "/tmp".into(),
        interactive: false,
        tty: false,
        env: Default::default(),
    };
    store.admit_execution("p", &other.id, "run", cmd).unwrap();
    let result: Result<(), _> = store.with_idle_work("p", &work, &grant.proof, |_| {
        panic!("must not snapshot active directory")
    });
    assert!(matches!(result, Err(Error::ActiveExecution)));
}
