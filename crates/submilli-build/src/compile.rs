//! Compiling a package through the store's [`FrontEnd`].

use std::collections::BTreeMap;

use interpreter::compile::{
    CompiledPackage, compile_package_request, package_modules, package_sources,
};
use interpreter::compiler_error::CompileError;
use interpreter::{
    FrontEnd, ModulePath, PackageDeclaration, PackageRequest, PackageSourceModule, PackageSurface,
    Stdlib,
};

/// Compile a package whose entry module is `lib`, against the core stdlib,
/// with `config` overriding its `config` module's values.
pub(crate) fn compile_package(
    front_end: &dyn FrontEnd,
    name: &str,
    modules: &[PackageSourceModule<'_>],
    dependencies: &[&PackageDeclaration],
    transitive: &[&PackageDeclaration],
    config: &BTreeMap<String, String>,
    surface: PackageSurface,
) -> Result<CompiledPackage, CompileError> {
    let stdlib = Stdlib::core().package_declarations();
    let stdlib: Vec<&PackageDeclaration> = stdlib.iter().collect();
    let sources = package_sources(modules)?;
    let modules = package_modules(&sources);
    let runtime = interpreter::runtime_declarations();
    compile_package_request(
        front_end,
        &PackageRequest {
            name,
            root_module: &ModulePath::from("lib"),
            modules: &modules,
            sources: &sources,
            prelude: runtime.prelude,
            host: runtime.host,
            stdlib: &stdlib,
            dependencies,
            transitive,
            config: Some(config),
            surface,
        },
    )
}
