# submilli-build

Builds, installs and reads [Submilli](https://github.com/submilli/submilli-runtime)
packages. It parses the `submilli.toml` manifest, builds packages into the
artifacts `submilli-engine` loads, installs them into a package store, and reads
a package's dependency closure back from the store with
`PackageStore::load_closure`.

The artifact format is versioned with the engine, so this crate is released in
lockstep with `submilli-engine` and depends on it at exactly the same version.

Licensed under Apache-2.0.
