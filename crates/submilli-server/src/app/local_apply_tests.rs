use super::*;

/// The local apply path leaves no stale `@mcp/<server>` catalog behind: the next run
/// rediscovers against the edited `mcp:` block, and other blueprints keep theirs.
#[tokio::test]
async fn a_local_apply_evicts_the_blueprints_mcp_catalogs_only() {
    let state = AppState::new(ServerConfig {
        blueprints: Some(Arc::new(InMemoryBlueprintStore::default())),
        ..Default::default()
    })
    .await
    .unwrap();
    state
        .apply_local_blueprint("name: demo\n", "v1")
        .await
        .unwrap();
    let mine = mcp_catalog_cache_key("demo", None);
    let theirs = mcp_catalog_cache_key("other", None);
    {
        let mut catalogs = state.inner.mcp_catalogs.lock().unwrap();
        catalogs.insert(mine.clone(), Arc::new(McpCatalog::empty()));
        catalogs.insert(theirs.clone(), Arc::new(McpCatalog::empty()));
    }
    let generation = state.inner.mcp_catalog_generation.load(Ordering::Acquire);

    state
        .apply_local_blueprint("name: demo\ndefault: allow\n", "v2")
        .await
        .unwrap();

    let catalogs = state.inner.mcp_catalogs.lock().unwrap();
    assert!(!catalogs.contains_key(&mine));
    assert!(catalogs.contains_key(&theirs));
    assert!(state.inner.mcp_catalog_generation.load(Ordering::Acquire) > generation);
}

/// A refused apply changes nothing: the cached catalog and the tag stay.
#[tokio::test]
async fn a_refused_local_apply_evicts_nothing() {
    let state = AppState::new(ServerConfig {
        blueprints: Some(Arc::new(InMemoryBlueprintStore::default())),
        ..Default::default()
    })
    .await
    .unwrap();
    state
        .apply_local_blueprint("name: demo\n", "v1")
        .await
        .unwrap();
    let mine = mcp_catalog_cache_key("demo", None);
    state
        .inner
        .mcp_catalogs
        .lock()
        .unwrap()
        .insert(mine.clone(), Arc::new(McpCatalog::empty()));
    assert!(
        state
            .apply_local_blueprint("name: demo\nnot_a_key: 1\n", "v2")
            .await
            .is_err()
    );
    assert!(state.inner.mcp_catalogs.lock().unwrap().contains_key(&mine));
    let found = state.blueprint_for_run("demo").await.unwrap().unwrap();
    assert_eq!(found.version_tag.as_deref(), Some("v1"));
}
