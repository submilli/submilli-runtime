//! Embedded test applications supply their blueprint dependency explicitly.

pub fn config() -> submilli_server::ServerConfig {
    submilli_server::ServerConfig {
        blueprints: Some(std::sync::Arc::new(
            submilli_server::blueprint::InMemoryBlueprintStore::default(),
        )),
        ..Default::default()
    }
}
