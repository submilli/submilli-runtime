//! A session's application tools as programs see them: the `@tools` package
//! mapped from the catalog the harness declared, by the same mapper the
//! `@mcp/<server>` packages use.

use interpreter::PackageDeclaration;
use submilli_shared::mcp::catalog::{ToolCatalogEntry, build_tools_definitions};

use crate::domain::tools::SessionTools;

/// The `@tools` package for `tools`, and a warning for each tool the mapper
/// could not express (dropped) or could type only as `unknown`.
pub(crate) fn declaration(tools: &SessionTools) -> (PackageDeclaration, Vec<String>) {
    let entries: Vec<ToolCatalogEntry> = tools
        .tools()
        .iter()
        .map(|tool| ToolCatalogEntry {
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.input_schema.clone(),
            output_schema: tool.output_schema.clone(),
        })
        .collect();
    let (declaration, warnings) = build_tools_definitions(&entries);
    let warnings = warnings
        .into_iter()
        .map(|warning| {
            format!(
                "{}: {}",
                interpreter::runtime::TOOLS_PACKAGE,
                warning.message
            )
        })
        .collect();
    (declaration, warnings)
}
