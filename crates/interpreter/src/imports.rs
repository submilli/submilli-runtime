//! What a script's imports are: the engine's to decide, whichever front end
//! parsed the script ([`crate::ParsedScript::imports`]).

use std::collections::BTreeSet;

use crate::{PackageDeclaration, runtime};

/// What a script imports from outside its own source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptImports {
    pub stdlib: BTreeSet<String>,
    pub registry_packages: BTreeSet<String>,
    pub mcp_servers: BTreeSet<String>,
    /// The script imports `@tools`, the session's application tools.
    pub tools: bool,
    /// The script imports `@functions`, the harness-supplied function set.
    pub functions: bool,
}

/// What each module a script imports is ([`crate::ParsedScript::imports`]):
/// `stdlib` names the standard-library packages the embedder offers.
pub fn classify_imports(specifiers: &[String], stdlib: &[&PackageDeclaration]) -> ScriptImports {
    let stdlib_names: BTreeSet<&str> = stdlib
        .iter()
        .map(|defs| defs.package_name.as_str())
        .collect();
    let mut imports = ScriptImports::default();
    for module in specifiers {
        if let Some(server) = module.strip_prefix("@mcp/") {
            imports.mcp_servers.insert(server.to_string());
        } else if module == runtime::TOOLS_PACKAGE {
            imports.tools = true;
        } else if module == crate::functions::FUNCTIONS_PACKAGE {
            imports.functions = true;
        } else if stdlib_names.contains(module.as_str())
            || module == crate::stdlib::functions::MODULE_NAME
            || crate::stdlib::OptionalPackage::from_module_name(module).is_some()
        {
            // An optional package counts as stdlib whether or not the embedder
            // enables it: a disabled one is reported as unavailable, never
            // looked up as a registry package.
            imports.stdlib.insert(module.clone());
        } else {
            imports.registry_packages.insert(module.clone());
        }
    }
    imports
}
