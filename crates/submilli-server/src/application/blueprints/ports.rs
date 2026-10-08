pub(crate) trait MCPServer: Send + Sync {
    fn evict(&self, name: &str);
}

pub(crate) trait MCPCatalog: Send + Sync {
    fn evict(&self, name: &str);
}

pub(crate) trait PreparedPackages: Send + Sync {
    fn evict(&self, name: &str);
}
