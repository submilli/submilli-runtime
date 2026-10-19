use std::fs;
use std::path::Path;

use super::*;
use crate::commands::playground::state::SESSIONS_LEFT_BEHIND;

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn blueprint(yaml: &str) -> Blueprint {
    submilli_blueprint::parse(yaml).unwrap()
}

fn per_session() -> Blueprint {
    blueprint(
        "name: notes\nidle_timeout: 1h\nvfs:\n  mode: per_session\nvariables:\n  \
         user:\n    default: ada\n  team:\n    default: core\n",
    )
}

async fn booted(dir: &Path) -> (StateDir, Sessions, Vec<Session>) {
    let state = StateDir::for_project(dir);
    let booted = Sessions::boot_reporting(&state).await.unwrap();
    (state, booted.sessions, booted.open)
}

#[tokio::test]
async fn a_missing_fixed_variable_takes_the_blueprint_default() {
    let dir = tempfile::tempdir().unwrap();
    let (_state, sessions, _) = booted(dir.path()).await;
    let supplied = BTreeMap::from([("team".to_owned(), "infra".to_owned())]);
    let session = sessions.open(&per_session(), &supplied).await.unwrap();
    let expected = BTreeMap::from([
        ("team".to_owned(), "infra".to_owned()),
        ("user".to_owned(), "ada".to_owned()),
    ]);
    assert_eq!(session.variables, expected);
    let stored = sessions.get(&session.id).await.unwrap().unwrap();
    assert_eq!(stored.variables, expected);
}

#[tokio::test]
async fn an_open_session_its_values_and_last_run_survive_closing_the_host() {
    let dir = tempfile::tempdir().unwrap();
    let (state, sessions, _) = booted(dir.path()).await;
    let session = sessions
        .open(&per_session(), &BTreeMap::new())
        .await
        .unwrap();
    sessions
        .values(&session.id)
        .set(&units("greeting"), &units("\"hello\""))
        .await
        .unwrap();
    sessions.begin_run(&session.id).await.unwrap();
    sessions
        .end_run(&session.id, "run-1", RunEnd::Finished(br#"{"result":1}"#))
        .await
        .unwrap();
    sessions.close_database().await.unwrap();

    // Nothing readable is stored: every value is sealed.
    let raw = fs::read(state.host_database_path()).unwrap();
    assert!(!raw.windows(5).any(|w| w == b"hello".as_slice()));

    let Booted {
        sessions: reopened,
        open,
        notice,
    } = Sessions::boot_reporting(&state).await.unwrap();
    assert_eq!(notice, None);
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].id, session.id);
    assert_eq!(open[0].variables, session.variables);
    assert_eq!(
        reopened
            .values(&session.id)
            .get(&units("greeting"))
            .await
            .unwrap(),
        Some(units("\"hello\""))
    );
    let Some(LastRun::Kept { run_id, json, .. }) = reopened.last_run(&session.id).await.unwrap()
    else {
        panic!("the last run was not kept");
    };
    assert_eq!(run_id, "run-1");
    assert_eq!(json, br#"{"result":1}"#);
    assert!(reopened.session_dir(&session.id).is_dir());
    reopened.close_database().await.unwrap();
}

#[tokio::test]
async fn the_reaper_closes_an_idle_session_but_not_one_with_a_run_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let (_state, sessions, _) = booted(dir.path()).await;
    let idle = sessions
        .open(&per_session(), &BTreeMap::new())
        .await
        .unwrap();
    let busy = sessions
        .open(&per_session(), &BTreeMap::new())
        .await
        .unwrap();
    sessions.begin_run(&busy.id).await.unwrap();
    let later = SystemTime::now() + Duration::from_secs(2 * 3600);
    assert_eq!(sessions.reap(later).await.unwrap(), vec![idle.id.clone()]);
    assert!(sessions.get(&idle.id).await.unwrap().is_none());
    assert!(sessions.get(&busy.id).await.unwrap().is_some());
    // Before its timeout, a session stays open.
    sessions
        .end_run(&busy.id, "run-1", RunEnd::Unknown)
        .await
        .unwrap();
    assert!(sessions.reap(SystemTime::now()).await.unwrap().is_empty());
}

#[tokio::test]
async fn closing_a_session_removes_its_directory_and_values() {
    let dir = tempfile::tempdir().unwrap();
    let (_state, sessions, _) = booted(dir.path()).await;
    let session = sessions
        .open(&per_session(), &BTreeMap::new())
        .await
        .unwrap();
    let values = sessions.values(&session.id);
    values.set(&units("k"), &units("1")).await.unwrap();
    let workspace = sessions.session_dir(&session.id);
    fs::write(workspace.join("notes.txt"), "kept until close").unwrap();

    assert!(sessions.close(&session.id).await.unwrap());
    assert!(!workspace.exists());
    // A run still holding the closed session's values can neither read nor write.
    assert!(matches!(
        values.get(&units("k")).await,
        Err(SessionKvError::SessionClosed { .. })
    ));
    assert!(matches!(
        values.set(&units("k"), &units("2")).await,
        Err(SessionKvError::SessionClosed { .. })
    ));
}

#[tokio::test]
async fn a_last_run_over_the_limit_is_reported_too_large_rather_than_stored() {
    let dir = tempfile::tempdir().unwrap();
    let (_state, sessions, _) = booted(dir.path()).await;
    // The real limit is 64 MiB; a smaller one keeps the test fast.
    let sessions = sessions.with_max_last_run_bytes(16);
    let session = sessions
        .open(&per_session(), &BTreeMap::new())
        .await
        .unwrap();
    let json = br#"{"result":"well over sixteen bytes"}"#;
    sessions.begin_run(&session.id).await.unwrap();
    sessions
        .end_run(&session.id, "run-1", RunEnd::Finished(json))
        .await
        .unwrap();
    assert!(matches!(
        sessions.last_run(&session.id).await.unwrap(),
        Some(LastRun::TooLarge { run_id, bytes, limit: 16, .. })
            if run_id == "run-1" && bytes == json.len() as u64
    ));
    assert_eq!(MAX_LAST_RUN_BYTES, 64 * 1024 * 1024);
}

#[tokio::test]
async fn an_unknown_end_forgets_the_last_run_and_a_run_that_never_started_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    let (_state, sessions, _) = booted(dir.path()).await;
    let session = sessions
        .open(&per_session(), &BTreeMap::new())
        .await
        .unwrap();
    sessions.begin_run(&session.id).await.unwrap();
    sessions
        .end_run(&session.id, "run-1", RunEnd::Finished(b"{}"))
        .await
        .unwrap();
    sessions.begin_run(&session.id).await.unwrap();
    sessions
        .end_run(&session.id, "run-2", RunEnd::NeverStarted)
        .await
        .unwrap();
    assert!(matches!(
        sessions.last_run(&session.id).await.unwrap(),
        Some(LastRun::Kept { run_id, .. }) if run_id == "run-1"
    ));
    sessions.begin_run(&session.id).await.unwrap();
    sessions
        .end_run(&session.id, "run-3", RunEnd::Unknown)
        .await
        .unwrap();
    assert_eq!(sessions.last_run(&session.id).await.unwrap(), None);
}

#[tokio::test]
async fn an_embedded_server_state_directory_starts_empty_with_one_notice() {
    let dir = tempfile::tempdir().unwrap();
    let state = StateDir::for_project(dir.path());
    // What the embedded server left: its database, and the run store beside it.
    fs::create_dir_all(state.root().join("server")).unwrap();
    fs::write(state.root().join("server/server.db"), b"old sessions").unwrap();
    fs::create_dir_all(state.store_dir()).unwrap();
    fs::write(state.store_dir().join("1.json"), b"a run record").unwrap();

    let booted = Sessions::boot_reporting(&state).await.unwrap();
    assert_eq!(booted.notice, Some(SESSIONS_LEFT_BEHIND));
    assert!(booted.open.is_empty());
    booted.sessions.close_database().await.unwrap();
    assert_eq!(
        fs::read(state.store_dir().join("1.json")).unwrap(),
        b"a run record"
    );
    assert_eq!(
        fs::read(state.root().join("server/server.db")).unwrap(),
        b"old sessions"
    );

    let next = Sessions::boot_reporting(&state).await.unwrap();
    assert_eq!(next.notice, None);
    next.sessions.close_database().await.unwrap();
}

#[tokio::test]
async fn a_new_project_starts_with_no_notice() {
    let dir = tempfile::tempdir().unwrap();
    let state = StateDir::for_project(dir.path());
    let booted = Sessions::boot_reporting(&state).await.unwrap();
    assert_eq!(booted.notice, None);
    booted.sessions.close_database().await.unwrap();
}

#[tokio::test]
async fn closing_cancels_admitted_work_and_waits_for_it_to_drain() {
    let dir = tempfile::tempdir().unwrap();
    let (_state, sessions, _) = booted(dir.path()).await;
    let sessions = Arc::new(sessions);
    let session = sessions
        .open(&per_session(), &BTreeMap::new())
        .await
        .unwrap();
    let permit = sessions.admit_resource_work(&session.id).await.unwrap();
    let close = tokio::spawn({
        let sessions = Arc::clone(&sessions);
        let id = session.id.clone();
        async move { sessions.close(&id).await }
    });
    permit.cancel.cancelled().await;
    tokio::task::yield_now().await;
    assert!(!close.is_finished(), "close must wait for work to drain");
    assert!(sessions.session_dir(&session.id).exists());
    drop(permit);
    assert!(close.await.unwrap().unwrap());
    assert!(!sessions.session_dir(&session.id).exists());
    assert!(matches!(
        sessions.admit_resource_work(&session.id).await,
        Err(SessionsError::UnknownSession)
    ));
}
