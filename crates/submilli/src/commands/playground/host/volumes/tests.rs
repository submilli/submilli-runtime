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
