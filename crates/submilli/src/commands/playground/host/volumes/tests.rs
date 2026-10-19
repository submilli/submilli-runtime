use std::fs;

use submilli_blueprint::VarBindings;

use super::*;

fn blueprint(yaml: &str) -> Blueprint {
    submilli_blueprint::parse(yaml).unwrap()
}

fn state(dir: &Path) -> StateDir {
    let state = StateDir::for_project(dir);
    state.create().unwrap();
    state
}

#[test]
fn a_managed_volume_file_from_before_the_upgrade_is_readable_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(dir.path());
    // Where the embedded server kept the volume.
    let before = state.volume_root().join("managed-local/notes");
    fs::create_dir_all(&before).unwrap();
    fs::write(before.join("todo.txt"), "carry over").unwrap();

    let volumes = Volumes::boot(
        &state,
        VolumeTable::from([(
            "notes".to_owned(),
            VolumeSpec::managed(SizeLimit::Unlimited),
        )]),
    );
    let vfs = volumes
        .build_vfs(
            &blueprint("name: x\nvfs:\n  mode: named\n  volume: notes\n"),
            &VarBindings::new(),
            None,
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(vfs.root().join("todo.txt")).unwrap(),
        "carry over"
    );
}

#[tokio::test]
async fn a_volume_write_past_its_size_limit_is_refused_by_its_shared_quota() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(dir.path());
    let volumes = Volumes::boot(
        &state,
        VolumeTable::from([(
            "notes".to_owned(),
            VolumeSpec::managed(SizeLimit::Bytes(10)),
        )]),
    );
    let blueprint =
        blueprint("name: x\nvfs:\n  mode: named\n  volume: notes\n  access: read_write\n");
    let vfs = volumes
        .build_vfs(&blueprint, &VarBindings::new(), None)
        .unwrap();
    let vfs = volumes.attach_limits(vfs, &blueprint).await.unwrap();
    let quota = vfs.quota().expect("the volume's limit").clone();
    assert_eq!(quota.limit(), 10);
    quota.reserve(8).unwrap();
    // The runtime's own refusal, so a program sees what it sees on the server.
    let _: interpreter::runtime::QuotaExceeded = quota.reserve(8).unwrap_err();
}

#[test]
fn leftover_ephemeral_scratch_is_swept_at_boot() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(dir.path());
    let scratch = state.host_ephemeral_dir().join(format!(
        "{}leftover",
        interpreter::runtime::vfs::TEMPDIR_PREFIX
    ));
    fs::create_dir_all(&scratch).unwrap();
    let kept = state.host_ephemeral_dir().join("not-scratch");
    fs::write(&kept, "").unwrap();
    Volumes::boot(&state, VolumeTable::new());
    assert!(!scratch.exists());
    assert!(kept.exists());
}

#[test]
fn references_must_be_declared_and_within_the_access_ceiling() {
    let dir = tempfile::tempdir().unwrap();
    let volumes = Volumes::boot(
        &state(dir.path()),
        VolumeTable::from([(
            "handbook".to_owned(),
            VolumeSpec::local_path(dir.path().join("handbook")).with_access(Access::ReadOnly),
        )]),
    );
    assert!(volumes.is_declared("handbook"));
    assert_eq!(volumes.check_reference("handbook", None), Ok(()));
    assert_eq!(
        volumes.check_reference("handbook", Some(Access::ReadOnly)),
        Ok(())
    );
    assert!(matches!(
        volumes.check_reference("handbook", Some(Access::ReadWrite)),
        Err(VolumeProblem::AccessExceeds(message)) if message.contains("read_only on this server")
    ));
    assert!(matches!(
        volumes.check_reference("other", None),
        Err(VolumeProblem::Undeclared(message)) if message.contains("declared volumes: handbook")
    ));
}

#[test]
fn a_withdrawn_declaration_is_gone_and_can_be_made_again() {
    let dir = tempfile::tempdir().unwrap();
    let volumes = Volumes::boot(&state(dir.path()), VolumeTable::new());
    assert_eq!(volumes.declare_managed("notes"), Ok(true));
    assert_eq!(volumes.declare_managed("notes"), Ok(false));
    assert!(volumes.is_declared("notes"));
    volumes.withdraw_managed("notes");
    assert!(!volumes.is_declared("notes"));
    assert!(volumes.check_reference("notes", None).is_err());
    assert_eq!(volumes.declare_managed("notes"), Ok(true));
}

#[test]
fn a_managed_name_must_differ_in_more_than_case() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(dir.path());
    let volumes = Volumes::boot(&state, VolumeTable::new());
    assert_eq!(volumes.declare_managed("notes"), Ok(true));
    let conflict = volumes.check_managed("Notes").unwrap_err();
    assert!(
        conflict.contains("differs only in letter case from 'notes'"),
        "{conflict}"
    );
    assert!(volumes.declare_managed("Notes").is_err());
    // A directory left behind by a volume of another case counts too.
    fs::create_dir_all(state.managed_volumes_dir().join("Archive")).unwrap();
    assert!(volumes.check_managed("archive").is_err());
    assert!(volumes.check_managed("bad/name").is_err());
    assert_eq!(volumes.check_managed("notes"), Ok(()));
}

#[test]
fn a_declared_managed_volume_is_mounted_by_a_run() {
    let dir = tempfile::tempdir().unwrap();
    let volumes = Volumes::boot(&state(dir.path()), VolumeTable::new());
    let named = blueprint("name: x\nvfs:\n  mode: named\n  volume: notes\n");
    assert!(matches!(
        volumes.build_vfs(&named, &VarBindings::new(), None),
        Err(VolumeError::UnknownVolume(_))
    ));
    volumes.declare_managed("notes").unwrap();
    volumes
        .build_vfs(&named, &VarBindings::new(), None)
        .unwrap();
}
