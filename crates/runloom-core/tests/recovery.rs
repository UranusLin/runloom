use runloom_core::{Error, NewWork, Store};

#[test]
fn inspection_preserves_unknown_evidence_and_does_not_expose_payloads() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("state.db");
    let mut store = Store::open(&db).unwrap();
    let work = store
        .create_work(NewWork {
            workspace: "fixture".into(),
            host: "host".into(),
            provider: "fixture".into(),
            config_root: temp.path().to_str().unwrap().into(),
            native_id: "native".into(),
            directory: temp.path().to_str().unwrap().into(),
            title: "fixture".into(),
        })
        .unwrap();
    let raw = rusqlite::Connection::open(&db).unwrap();
    raw.execute(
        "INSERT INTO checkpoint_reservations VALUES ('held',?1,'/fixture/destination',1,1)",
        [&work.id],
    )
    .unwrap();
    raw.execute("INSERT INTO executions(id,work_id,request_id,command_json,state) VALUES ('exec',?1,'start','SECRET_COMMAND','running')",[&work.id]).unwrap();
    raw.execute(
        "INSERT INTO input_receipts VALUES ('exec','input',?1,'uncertain')",
        [b"SECRET_PAYLOAD".as_slice()],
    )
    .unwrap();
    raw.execute(
        "INSERT INTO leases VALUES (?1,'owner',1,'SECRET_TOKEN',0)",
        [&work.id],
    )
    .unwrap();
    let report = store.recovery_inspect("fixture", &work.id).unwrap();
    assert_eq!(report.reservations.len(), 1);
    assert_eq!(report.unsettled_executions[0].persisted_state, "running");
    assert_eq!(report.uncertain_inputs.len(), 1);
    assert!(!report.process_liveness_checked);
    assert!(!report.automatic_recovery_available);
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("SECRET"));
    assert!(matches!(
        store.recovery_inspect("wrong", &work.id),
        Err(Error::NotFound)
    ));
    assert_eq!(
        store
            .recovery_inspect("fixture", &work.id)
            .unwrap()
            .reservations
            .len(),
        1
    );
    assert_eq!(
        raw.query_row("SELECT state FROM input_receipts", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "uncertain"
    );
    assert_eq!(
        raw.query_row("SELECT count(*) FROM checkpoint_reservations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn inspection_bounds_results_and_empty_does_not_claim_safe_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("db");
    let mut store = Store::open(&db).unwrap();
    let work = store
        .create_work(NewWork {
            workspace: "p".into(),
            host: "h".into(),
            provider: "f".into(),
            config_root: "/fixture".into(),
            native_id: "n".into(),
            directory: "/fixture".into(),
            title: "fixture".into(),
        })
        .unwrap();
    let empty = store.recovery_inspect("p", &work.id).unwrap();
    assert!(empty.reservations.is_empty());
    assert!(!empty.automatic_recovery_available);
    let raw = rusqlite::Connection::open(db).unwrap();
    for n in 0..105 {
        raw.execute(
            "INSERT INTO checkpoint_reservations VALUES (?1,?2,'/fixture',1,1)",
            rusqlite::params![format!("r{n:03}"), work.id],
        )
        .unwrap();
    }
    let report = store.recovery_inspect("p", &work.id).unwrap();
    assert!(report.truncated);
    assert_eq!(report.reservations.len(), 100);
    assert_eq!(
        raw.query_row("SELECT count(*) FROM checkpoint_reservations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        105
    );
}
