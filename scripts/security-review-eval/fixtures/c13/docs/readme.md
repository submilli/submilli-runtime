# Record operations

load refreshes a package-private cache of the record at path. It returns no data; acme.refresh authorizes refresh only. current discloses the cached record and requires acme.read for that record. Permissions to refresh and disclose are intentionally independent. Paths are absolute VFS paths.
