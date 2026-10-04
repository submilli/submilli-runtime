use super::*;
use crate::blueprint::FileBlueprintStore;

fn stored(yaml: &str) -> StoredBlueprint {
    StoredBlueprint::new(submilli_blueprint::parse(yaml).unwrap(), yaml.into())
}

async fn open(
    path: &std::path::Path,
    source: Option<PathBuf>,
) -> (Arc<ServerDatabase>, SqliteBlueprintStore) {
    let database = Arc::new(ServerDatabase::open(path).await.unwrap());
    let store = SqliteBlueprintStore::new(database.clone(), source);
    (database, store)
}

#[tokio::test]
async fn imports_history_and_active_index_then_archives_source() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("blueprints");
    let files = FileBlueprintStore::new(source.clone()).unwrap();
    let original = "# keep this comment\nname: demo\n";
    files.add_yaml(stored(original)).await.unwrap();
    files
        .upsert_yaml(stored("name: demo\n# second"))
        .await
        .unwrap();
    files.add_yaml(stored("name: deleted\n")).await.unwrap();
    files.remove("deleted").await.unwrap();
    std::fs::write(source.join("demo.000007.yaml"), "invalid: [").unwrap();
    let path = directory.path().join("db/server.db");
    let (database, store) = open(&path, Some(source.clone())).await;
    store.migrate().await.unwrap();
    assert_eq!(store.list().await.unwrap(), vec!["demo"]);
    assert_eq!(
        store.get_yaml("demo").await.unwrap().unwrap(),
        "name: demo\n# second"
    );
    let archive = directory.path().join("archive/blueprints");
    assert!(archive.join("index.json").exists());
    assert!(!source.exists());
    assert!(!source.join("demo.000001.yaml").exists());
    assert!(!store.upsert_yaml(stored(original)).await.unwrap());
    let revision: i64 = database
        .read(|connection| {
            Box::pin(async move {
                Ok(
                    sqlx::query_scalar("SELECT current_revision FROM blueprints WHERE name='demo'")
                        .fetch_one(connection)
                        .await?,
                )
            })
        })
        .await
        .unwrap();
    assert_eq!(revision, 8);
    assert!(store.remove("demo").await.unwrap());
    store.add_yaml(stored(original)).await.unwrap();
    database.close().await.unwrap();
    // Archived files are never used as an import source, even when damaged.
    std::fs::write(archive.join("index.json"), "{").unwrap();
    let (database, store) = open(&path, Some(source)).await;
    store.migrate().await.unwrap();
    assert_eq!(store.get_yaml("demo").await.unwrap().unwrap(), original);
    let rows: Vec<(i64, String)> = database.read(|connection| Box::pin(async move {
        Ok(sqlx::query_as("SELECT revision, yaml FROM blueprint_revisions WHERE name='demo' ORDER BY revision").fetch_all(connection).await?)
    })).await.unwrap();
    assert_eq!(
        rows.iter().map(|row| row.0).collect::<Vec<_>>(),
        vec![1, 2, 7, 8, 9]
    );
    assert_eq!(rows.first().unwrap().1, original);
    database.close().await.unwrap();
}

#[tokio::test]
async fn malformed_current_yaml_reserves_name_but_remains_readable_and_replaceable() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("files");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("index.json"), r#"{"demo":1}"#).unwrap();
    std::fs::write(source.join("demo.000001.yaml"), "broken: [").unwrap();
    let (database, store) = open(&directory.path().join("db"), Some(source)).await;
    store.migrate().await.unwrap();
    assert!(store.list().await.unwrap().is_empty());
    assert!(store.get("demo").await.unwrap().is_none());
    assert!(store.unusable_reason("demo").await.unwrap().is_some());
    assert_eq!(store.get_yaml("demo").await.unwrap().unwrap(), "broken: [");
    assert!(matches!(
        store.add_yaml(stored("name: demo")).await,
        Err(StoreError::AlreadyExists)
    ));
    assert!(!store.upsert_yaml(stored("name: demo")).await.unwrap());
    assert!(store.get("demo").await.unwrap().is_some());
    database.close().await.unwrap();
}

#[tokio::test]
async fn failed_import_rolls_back_and_can_retry() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("files");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("index.json"), r#"{"demo":2}"#).unwrap();
    std::fs::write(source.join("demo.000001.yaml"), "name: demo").unwrap();
    let (database, store) = open(&directory.path().join("db"), Some(source.clone())).await;
    assert!(store.migrate().await.is_err());
    let count: i64 = database.read(|connection| Box::pin(async move {
        Ok(sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM blueprint_revisions) + (SELECT COUNT(*) FROM blueprints)").fetch_one(connection).await?)
    })).await.unwrap();
    assert_eq!(count, 0);
    assert!(source.join("demo.000001.yaml").exists());
    assert!(!directory.path().join("archive").exists());
    std::fs::write(source.join("demo.000002.yaml"), "name: demo").unwrap();
    store.migrate().await.unwrap();
    assert_eq!(store.list().await.unwrap(), vec!["demo"]);
    database.close().await.unwrap();
}

#[tokio::test]
async fn independent_instances_serialize_revisions_and_keep_history_after_delete() {
    let directory = tempfile::tempdir().unwrap();
    let (database, store) = open(&directory.path().join("db"), None).await;
    store.migrate().await.unwrap();
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let other = SqliteBlueprintStore::new(database.clone(), None);
        tasks.push(tokio::spawn(async move {
            other.upsert_yaml(stored("name: demo")).await.unwrap()
        }));
    }
    let mut created = 0;
    for task in tasks {
        created += usize::from(task.await.unwrap());
    }
    assert_eq!(created, 1);
    store.remove("demo").await.unwrap();
    assert!(store.get_yaml("demo").await.unwrap().is_none());
    store.add_yaml(stored("name: demo")).await.unwrap();
    let revision: i64 = database
        .read(|connection| {
            Box::pin(async move {
                Ok(
                    sqlx::query_scalar("SELECT current_revision FROM blueprints WHERE name='demo'")
                        .fetch_one(connection)
                        .await?,
                )
            })
        })
        .await
        .unwrap();
    assert_eq!(revision, 9);
    database.close().await.unwrap();
}

#[tokio::test]
async fn import_rejects_duplicate_and_out_of_range_revisions_without_completion() {
    for filename in ["demo.1.yaml", "demo.18446744073709551615.yaml"] {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("files");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("demo.000001.yaml"), "name: demo").unwrap();
        std::fs::write(source.join(filename), "name: demo").unwrap();
        let (database, store) = open(&directory.path().join("db"), Some(source)).await;
        assert!(store.migrate().await.is_err());
        database.close().await.unwrap();
    }
}

#[tokio::test]
async fn no_index_preserves_orphans_without_activating_them() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("files");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("demo.000004.yaml"), "name: demo").unwrap();
    let (database, store) = open(&directory.path().join("db"), Some(source)).await;
    store.migrate().await.unwrap();
    assert!(store.list().await.unwrap().is_empty());
    store.add_yaml(stored("name: demo")).await.unwrap();
    let revision: i64 = database
        .read(|connection| {
            Box::pin(async move {
                Ok(
                    sqlx::query_scalar("SELECT current_revision FROM blueprints WHERE name='demo'")
                        .fetch_one(connection)
                        .await?,
                )
            })
        })
        .await
        .unwrap();
    assert_eq!(revision, 5);
    database.close().await.unwrap();
}

#[tokio::test]
async fn absent_source_is_allowed_but_conflicting_new_source_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let (database, store) = open(
        &directory.path().join("db"),
        Some(directory.path().join("absent")),
    )
    .await;
    store.migrate().await.unwrap();
    store.add_yaml(stored("name: demo")).await.unwrap();
    let source = directory.path().join("new");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("other.000001.yaml"), "name: other").unwrap();
    SqliteBlueprintStore::new(database.clone(), Some(source.clone()))
        .migrate()
        .await
        .unwrap_err();
    assert!(source.join("other.000001.yaml").exists());
    assert_eq!(store.list().await.unwrap(), vec!["demo"]);
    database.close().await.unwrap();
}

#[tokio::test]
async fn archive_failure_after_commit_retries_without_resurrecting_deleted_blueprints() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("blueprints");
    let files = FileBlueprintStore::new(source.clone()).unwrap();
    files.add_yaml(stored("name: demo")).await.unwrap();
    // Block archive directory creation after the import commits.
    std::fs::write(directory.path().join("archive"), "occupied").unwrap();
    let path = directory.path().join("db");
    let (database, store) = open(&path, Some(source.clone())).await;
    assert!(store.migrate().await.is_err());
    assert_eq!(store.list().await.unwrap(), ["demo"]);
    assert!(source.join("index.json").exists());
    store.remove("demo").await.unwrap();
    database.close().await.unwrap();
    std::fs::remove_file(directory.path().join("archive")).unwrap();
    let (database, store) = open(&path, Some(source.clone())).await;
    store.migrate().await.unwrap();
    assert!(store.list().await.unwrap().is_empty());
    assert!(!source.exists());
    assert!(
        directory
            .path()
            .join("archive/blueprints/demo.000001.yaml")
            .exists()
    );
    store.add_yaml(stored("name: demo")).await.unwrap();
    let revision: i64 = database
        .read(|connection| {
            Box::pin(async move {
                Ok(
                    sqlx::query_scalar("SELECT current_revision FROM blueprints WHERE name='demo'")
                        .fetch_one(connection)
                        .await?,
                )
            })
        })
        .await
        .unwrap();
    assert_eq!(revision, 2);
    database.close().await.unwrap();
}

#[tokio::test]
async fn restart_after_commit_archives_the_whole_directory() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("blueprints");
    let files = FileBlueprintStore::new(source.clone()).unwrap();
    files.add_yaml(stored("name: demo")).await.unwrap();
    files
        .upsert_yaml(stored("name: demo\n# new"))
        .await
        .unwrap();
    std::fs::create_dir(source.join("notes")).unwrap();
    std::fs::write(source.join("notes/readme.txt"), "keep this too").unwrap();
    let path = directory.path().join("db");
    let (database, _) = open(&path, Some(source.clone())).await;
    let import_source = source.clone();
    database
        .transaction(move |connection| {
            Box::pin(async move { import_files(connection, Some(import_source)).await })
        })
        .await
        .unwrap();
    assert!(source.join("index.json").exists());
    assert!(!directory.path().join("archive").exists());
    database.close().await.unwrap();
    let (database, store) = open(&path, Some(source.clone())).await;
    store.migrate().await.unwrap();
    assert_eq!(
        store.get_yaml("demo").await.unwrap().unwrap(),
        "name: demo\n# new"
    );
    assert!(!source.exists());
    for file in [
        "demo.000001.yaml",
        "demo.000002.yaml",
        "index.json",
        "notes/readme.txt",
    ] {
        assert!(
            directory
                .path()
                .join("archive/blueprints")
                .join(file)
                .exists()
        );
    }
    database.close().await.unwrap();
}

#[tokio::test]
async fn empty_import_archives_index_and_restart_does_not_read_archive() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("blueprints");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("index.json"), "{}").unwrap();
    let (database, store) = open(&directory.path().join("db"), Some(source.clone())).await;
    store.migrate().await.unwrap();
    assert!(!source.exists());
    std::fs::write(
        directory.path().join("archive/blueprints/index.json"),
        "broken",
    )
    .unwrap();
    store.migrate().await.unwrap();
    assert!(store.list().await.unwrap().is_empty());
    database.close().await.unwrap();
}

#[tokio::test]
async fn archive_collision_preserves_both_source_and_existing_archive() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("blueprints");
    let files = FileBlueprintStore::new(source.clone()).unwrap();
    files.add_yaml(stored("name: demo")).await.unwrap();
    let archive = directory.path().join("archive/blueprints");
    std::fs::create_dir_all(&archive).unwrap();
    std::fs::write(archive.join("demo.000001.yaml"), "keep").unwrap();
    let (database, store) = open(&directory.path().join("db"), Some(source.clone())).await;
    assert!(store.migrate().await.is_err());
    assert_eq!(
        std::fs::read_to_string(archive.join("demo.000001.yaml")).unwrap(),
        "keep"
    );
    assert!(source.join("demo.000001.yaml").exists());
    assert_eq!(store.list().await.unwrap(), ["demo"]);
    database.close().await.unwrap();
}

#[test]
fn archive_move_atomically_refuses_an_existing_directory() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source");
    let destination = directory.path().join("destination");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(source.join("keep"), "imported").unwrap();
    assert!(archive::move_directory(&source, &destination).is_err());
    assert_eq!(
        std::fs::read_to_string(source.join("keep")).unwrap(),
        "imported"
    );
    assert!(std::fs::read_dir(&destination).unwrap().next().is_none());
}

#[tokio::test]
async fn source_containing_the_database_is_not_moved() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("blueprints");
    let path = source.join("db/server.db");
    let (database, store) = open(&path, Some(source.clone())).await;
    assert!(store.migrate().await.is_err());
    assert!(path.exists());
    assert!(!directory.path().join("archive").exists());
    database.close().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn symbolic_link_source_is_rejected_without_moving_its_target() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("actual");
    let source = directory.path().join("blueprints");
    let files = FileBlueprintStore::new(target.clone()).unwrap();
    files.add_yaml(stored("name: demo")).await.unwrap();
    std::os::unix::fs::symlink("actual", &source).unwrap();
    let (database, store) = open(&directory.path().join("db"), Some(source.clone())).await;
    assert!(store.migrate().await.is_err());
    assert!(store.list().await.unwrap().is_empty());
    assert!(source.join("index.json").exists());
    assert!(target.join("index.json").exists());
    assert!(!directory.path().join("archive").exists());
    database.close().await.unwrap();
}
