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
async fn imports_history_and_active_index_once_and_keeps_source() {
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
    store.initialize().await.unwrap();
    assert_eq!(store.list().await.unwrap(), vec!["demo"]);
    assert_eq!(
        store.get_yaml("demo").await.unwrap().unwrap(),
        "name: demo\n# second"
    );
    assert!(source.join("index.json").exists());
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
    // Retained files must not become authoritative on restart, even when damaged.
    std::fs::write(source.join("index.json"), "{").unwrap();
    let (database, store) = open(&path, Some(source)).await;
    store.initialize().await.unwrap();
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
    store.initialize().await.unwrap();
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
    assert!(store.initialize().await.is_err());
    let count: i64 = database.read(|connection| Box::pin(async move {
        Ok(sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM blueprint_revisions) + (SELECT COUNT(*) FROM store_imports)").fetch_one(connection).await?)
    })).await.unwrap();
    assert_eq!(count, 0);
    std::fs::write(source.join("demo.000002.yaml"), "name: demo").unwrap();
    store.initialize().await.unwrap();
    assert_eq!(store.list().await.unwrap(), vec!["demo"]);
    database.close().await.unwrap();
}

#[tokio::test]
async fn independent_instances_serialize_revisions_and_keep_history_after_delete() {
    let directory = tempfile::tempdir().unwrap();
    let (database, store) = open(&directory.path().join("db"), None).await;
    store.initialize().await.unwrap();
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
        assert!(store.initialize().await.is_err());
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
    store.initialize().await.unwrap();
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
async fn absent_source_and_changed_source_do_not_reimport() {
    let directory = tempfile::tempdir().unwrap();
    let (database, store) = open(
        &directory.path().join("db"),
        Some(directory.path().join("absent")),
    )
    .await;
    store.initialize().await.unwrap();
    store.add_yaml(stored("name: demo")).await.unwrap();
    let source = directory.path().join("new");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("index.json"), "{").unwrap();
    SqliteBlueprintStore::new(database.clone(), Some(source))
        .initialize()
        .await
        .unwrap();
    assert_eq!(store.list().await.unwrap(), vec!["demo"]);
    database.close().await.unwrap();
}
