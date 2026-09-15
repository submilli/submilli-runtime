//! Maps an MCP server's `tools/list` output to a `.d.subm`-shaped [`PackageDeclaration`]
//! for the `@mcp/<server>` virtual package.
//!
//! Each tool becomes one exported function. Its `inputSchema` (a JSON Schema
//! object) maps to a single `args` parameter typed by a structural object type —
//! recursively, so nested objects become nested object types, enums become
//! string-literal unions, and arrays carry their element type. Structural types
//! (not named interfaces) so the same value can be built and JSON-validated by the
//! `JSON.parse` machinery codegen already has.
//!
//! A tool is exposed only if its `inputSchema` is representable: scalar `anyOf`/
//! `oneOf` map to unions (a nullable `[{string},{null}]` → `string | null`), but
//! anything the strict subset still can't express (`allOf`, object/array
//! combinators, `$ref`, recursion, open objects) would force an `unknown` argument
//! the script can't construct, so such a tool is dropped with a
//! [`ToolWarning`](crate::mcp::ToolWarning). Its return
//! is typed from `outputSchema` when that is representable; otherwise the tool
//! returns `unknown`.
//!
//! The mapper is pure and network-free: discovery feeds it [`ToolCatalogEntry`]s,
//! keeping the rmcp transport out of the codegen path.

use std::collections::BTreeMap;

use interpreter::{
    DefaultValue, DocComment, ObjectField, PackageDeclaration, Param, Shape, Span, Type, ValueKind,
    ValueSymbol, mangle,
};
use serde_json::{Map, Value};

use crate::mcp::ToolWarning;
use crate::mcp::schema_registry::SchemaPack;

/// One tool from a server's `tools/list`, decoupled from rmcp so the mapper is
/// testable with canned schemas.
pub struct ToolCatalogEntry {
    pub name: String,
    pub description: Option<String>,
    /// The tool's JSON Schema `inputSchema` (normally `{"type":"object",...}`).
    pub input_schema: Value,
    /// The tool's JSON Schema `outputSchema`, when the server publishes one. A
    /// representable output becomes the function's typed return; anything else
    /// falls back to `unknown`.
    pub output_schema: Option<Value>,
}

/// Bound on schema nesting before a sub-schema is treated as unrepresentable.
/// Guards against recursive (`$ref`-cycle) or pathologically deep schemas.
const MAX_DEPTH: u32 = 12;

/// The package name for a server's virtual package, e.g. `@mcp/linear`.
pub fn package_name(server: &str) -> String {
    format!("@mcp/{server}")
}

/// Build the `@mcp/<server>` [`PackageDeclaration`] from a (possibly empty) tool catalog,
/// plus any [`ToolWarning`]s raised while mapping. An empty catalog yields a valid,
/// importable package with no tools — the shape a server-unreachable stub takes.
///
/// A tool is exposed only if its arguments map to fully concrete structural types;
/// one whose argument schema needs `unknown` anywhere is dropped (we can't marshal
/// a value the script can't construct). Its return is typed from `outputSchema`
/// when that is representable, otherwise the tool returns `unknown`.
pub fn build_mcp_definitions(
    server: &str,
    tools: &[ToolCatalogEntry],
    pack: Option<&SchemaPack>,
) -> (PackageDeclaration, Vec<ToolWarning>) {
    let pkg = package_name(server);
    let mut defs = PackageDeclaration::with_package(&pkg);
    // The structured signal that this is an MCP package — codegen routes its tool
    // calls to `submilli:mcp.call` and skips per-tool imports, no name-prefix match.
    defs.mcp_server = Some(server.to_string());
    let mut warnings = Vec::new();
    for tool in tools {
        let Some(params) = args_param(&tool.input_schema) else {
            warnings.push(ToolWarning::dropped(
                server,
                &tool.name,
                "an argument uses a schema that can't be expressed in the type system",
            ));
            continue;
        };
        let resolved = resolve_output(tool, pack);
        let ret = match &resolved {
            ResolvedOutput::Published(ty) | ResolvedOutput::Registry { ty, .. } => ty.clone(),
            ResolvedOutput::Untyped { .. } => {
                warnings.push(ToolWarning::untyped_output(server, &tool.name));
                Type::Unknown
            }
        };
        for param in &params {
            collect_shapes(&param.ty, &mut defs.shapes);
        }
        collect_shapes(&ret, &mut defs.shapes);
        defs.values.insert(
            tool.name.clone(),
            ValueSymbol {
                name: tool.name.clone(),
                mangled_name: mangle::host(&pkg, &tool.name),
                declaration_span: Span::at(interpreter::FileId::MCP),
                kind: ValueKind::Function {
                    generics: Vec::new(),
                    params,
                    ret,
                    type_predicate: None,
                    doc: Some(tool_doc(tool.description.as_deref(), &resolved)),
                },
            },
        );
    }
    (defs, warnings)
}

/// How a tool's return type was resolved: from the server's own `outputSchema`,
/// from a built-in [`SchemaPack`] overlay filling the gap, or not at all
/// (`unknown`). Drives both the typed return and the `response`-doc line.
enum ResolvedOutput {
    /// Server published a representable, structured schema. Always wins.
    Published(Type),
    /// Server published nothing usable; a built-in pack filled the gap. Kept
    /// distinct from `Published` for provenance even though both read as "typed".
    Registry { ty: Type },
    /// No typed return — `published` distinguishes "published but unrepresentable"
    /// from "published nothing" for the doc line.
    Untyped { published: bool },
}

/// Resolve a tool's return type. The server-published `outputSchema` wins when it's
/// representable; otherwise a matching [`SchemaPack`] entry fills the gap; otherwise
/// the tool returns `unknown`.
fn resolve_output(tool: &ToolCatalogEntry, pack: Option<&SchemaPack>) -> ResolvedOutput {
    let published = tool
        .output_schema
        .as_ref()
        .and_then(|s| schema_to_type(s, 0));
    if let Some(ty) = published {
        return ResolvedOutput::Published(ty);
    }
    if let Some(pack) = pack
        && let Some(ty) = pack
            .output_schema(&tool.name)
            .and_then(|s| schema_to_type(s, 0))
    {
        return ResolvedOutput::Registry { ty };
    }
    ResolvedOutput::Untyped {
        published: tool.output_schema.is_some(),
    }
}

/// A tool's parameter list: a single structural-object `args` param when the
/// `inputSchema` declares properties, or empty for a zero-arg tool. `None` means
/// "drop the tool" — an argument property isn't representable in the subset.
///
/// When the schema marks nothing required, the `args` param carries a `{}` default so a
/// zero-arg call type-checks — the common all-optional-filters shape an LLM reaches for.
fn args_param(schema: &Value) -> Option<Vec<Param>> {
    let has_props = schema
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|p| !p.is_empty());
    if !has_props {
        return Some(Vec::new());
    }
    let obj = schema.as_object()?;
    let ty = object_type(obj, 0)?;
    let has_required = obj
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|a| a.iter().any(Value::is_string));
    let param = if has_required {
        Param::new("args", ty)
    } else {
        Param::with_default("args", ty, DefaultValue::EmptyObject)
    };
    Some(vec![param])
}

/// Recursively map a JSON Schema node to a structural [`Type`]. `None` signals the
/// node can't be expressed in the strict subset (combinators, `$ref`, multi-type,
/// open objects, excess depth) — the caller decides whether that drops the tool
/// (input) or falls back to `unknown` (output).
pub fn schema_to_type(schema: &Value, depth: u32) -> Option<Type> {
    if depth > MAX_DEPTH {
        return None;
    }
    let obj = schema.as_object()?;
    // `allOf` (intersection), `$ref` (no resolver here), and `not` have no faithful
    // subset representation — unrepresentable.
    if obj.contains_key("allOf") || obj.contains_key("$ref") || obj.contains_key("not") {
        return None;
    }
    // `anyOf`/`oneOf` of scalar members map to a union (e.g. a nullable field
    // `[{string},{null}]` → `string | null`). Object/array members aren't wired
    // through union codegen for MCP args, so such a combinator stays unrepresentable.
    if let Some(variants) = obj.get("anyOf").or_else(|| obj.get("oneOf")) {
        return scalar_combinator_union(variants.as_array()?, depth);
    }
    if let Some(ty) = string_enum_union(obj) {
        return Some(ty);
    }
    let type_tag = match obj.get("type") {
        Some(Value::String(s)) => s.as_str(),
        // A multi-type (`"type": ["string","null"]`, common for an optional /
        // nullable field) is a union of its primitive members.
        Some(Value::Array(tags)) => return multi_type_union(tags),
        Some(_) => return None,
        None => {
            // No `type` but `properties` present → treat as an object.
            if obj.contains_key("properties") {
                "object"
            } else {
                return None;
            }
        }
    };
    match type_tag {
        "string" => Some(Type::String),
        "number" | "integer" => Some(Type::Number),
        "boolean" => Some(Type::Boolean),
        "null" => Some(Type::Null),
        "array" => {
            let elem = schema_to_type(obj.get("items")?, depth + 1)?;
            Some(Type::Array(Box::new(elem)))
        }
        "object" => object_type(obj, depth),
        _ => None,
    }
}

/// Map an object schema to a structural [`Type::Object`]. `None` when it declares
/// no properties (an open object carries no field information) or any property is
/// unrepresentable.
fn object_type(obj: &Map<String, Value>, depth: u32) -> Option<Type> {
    let props = obj
        .get("properties")
        .and_then(Value::as_object)
        .filter(|p| !p.is_empty())?;
    let required: Vec<&str> = obj
        .get("required")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    let mut fields: BTreeMap<String, ObjectField> = BTreeMap::new();
    for (name, prop_schema) in props {
        let ty = schema_to_type(prop_schema, depth + 1)?;
        let field = if required.contains(&name.as_str()) {
            ObjectField::required(ty)
        } else {
            ObjectField::optional(ty)
        };
        fields.insert(name.clone(), field);
    }
    Some(Type::Object { fields })
}

/// Map an `anyOf`/`oneOf` of scalar members to a union (e.g. a nullable field
/// `[{string},{null}]` → `string | null`). Members are mapped, flattened, and
/// deduped; a lone member collapses to that type. `None` if empty or any member
/// isn't a scalar (string / number / boolean / null / string-literal) — object
/// and array union members aren't wired through codegen for MCP args.
fn scalar_combinator_union(variants: &[Value], depth: u32) -> Option<Type> {
    if variants.is_empty() {
        return None;
    }
    let mut members: Vec<Type> = Vec::new();
    for variant in variants {
        push_scalar_members(schema_to_type(variant, depth + 1)?, &mut members)?;
    }
    match members.len() {
        0 => None,
        1 => members.into_iter().next(),
        _ => Some(Type::Union(members)),
    }
}

/// Append `ty`'s scalar members to `out` (deduped), flattening nested unions.
/// `None` if `ty` (or a nested member) is non-scalar — that makes the whole
/// combinator unrepresentable.
fn push_scalar_members(ty: Type, out: &mut Vec<Type>) -> Option<()> {
    match ty {
        Type::String | Type::Number | Type::Boolean | Type::Null | Type::StringLiteral(_) => {
            if !out.contains(&ty) {
                out.push(ty);
            }
            Some(())
        }
        Type::Union(inner) => {
            for member in inner {
                push_scalar_members(member, out)?;
            }
            Some(())
        }
        _ => None,
    }
}

/// Map a JSON Schema multi-type (`"type": ["string","null"]`) to a union of its
/// primitive members. `None` if any member isn't a self-describing primitive
/// (e.g. `"object"`, which would carry no field information here).
fn multi_type_union(tags: &[Value]) -> Option<Type> {
    let mut members: Vec<Type> = Vec::new();
    for tag in tags {
        let ty = match tag.as_str()? {
            "string" => Type::String,
            "number" | "integer" => Type::Number,
            "boolean" => Type::Boolean,
            "null" => Type::Null,
            _ => return None,
        };
        if !members.contains(&ty) {
            members.push(ty);
        }
    }
    match members.len() {
        0 => None,
        1 => members.into_iter().next(),
        _ => Some(Type::Union(members)),
    }
}

/// A `string` schema with an `enum` of string literals → a union of those
/// literals (a single literal stays `string`, since unions need ≥2 members).
/// Non-string or mixed enums yield `None` (unrepresentable) at the caller.
fn string_enum_union(obj: &Map<String, Value>) -> Option<Type> {
    let variants = obj.get("enum")?.as_array()?;
    if variants.is_empty() {
        return None;
    }
    let literals: Option<Vec<Type>> = variants
        .iter()
        .map(|v| v.as_str().map(|s| Type::StringLiteral(s.to_string())))
        .collect();
    let literals = literals?;
    match literals.len() {
        1 => Some(Type::String),
        _ => Some(Type::Union(literals)),
    }
}

/// Record every structural shape reachable from `ty` (the type itself and its
/// nested objects / arrays / unions) on `defs.shapes`, so codegen emits the Wasm
/// subtype, vtable, and field-dispatch machinery the call site needs to build and
/// validate these values. Downstream dedups by canonical shape, so duplicates are
/// harmless.
fn collect_shapes(ty: &Type, out: &mut Vec<Shape>) {
    if let Some(shape) = Shape::from_type(ty) {
        out.push(shape);
    }
    match ty {
        Type::Object { fields } => {
            for field in fields.values() {
                collect_shapes(&field.ty, out);
            }
        }
        Type::Array(elem) => collect_shapes(elem, out),
        Type::Tuple(elements) => {
            for elem in elements {
                collect_shapes(elem, out);
            }
        }
        Type::Union(members) => {
            for member in members {
                collect_shapes(member, out);
            }
        }
        _ => {}
    }
}

fn tool_doc(description: Option<&str>, output: &ResolvedOutput) -> DocComment {
    let mut lines = Vec::new();
    if let Some(description) = description.filter(|d| !d.trim().is_empty()) {
        lines.push(description.trim().to_string());
    }
    lines.push(response_doc_line(output));
    doc_summary(&lines.join("\n"))
}

fn response_doc_line(output: &ResolvedOutput) -> String {
    // Typed → "use directly, no cast"; `unknown` → "cast after checking the shape". The
    // LLM's most common MCP mistake is casting a typed result, so lead with that. Whether
    // the type came from the server's own schema or our schema pack is irrelevant to the
    // caller, so both typed cases read identically.
    match output {
        ResolvedOutput::Published(_) | ResolvedOutput::Registry { .. } => {
            "Typed — use the result directly (narrow optional `foo?` fields first); no cast needed."
                .to_string()
        }
        ResolvedOutput::Untyped { published: true } => {
            "Returns `unknown` (the server's outputSchema is not representable) — cast to a declared type (`as T`) after checking the shape."
                .to_string()
        }
        ResolvedOutput::Untyped { published: false } => {
            "Returns `unknown`; this MCP server did not publish an outputSchema — cast to a declared type (`as T`) after checking the shape.".to_string()
        }
    }
}

fn doc_summary(summary: &str) -> DocComment {
    DocComment {
        span: Span::at(interpreter::FileId::MCP),
        summary: summary.to_string(),
        params: Vec::new(),
        returns: None,
        capabilities: Vec::new(),
        throws: Vec::new(),
        deprecated: None,
        examples: Vec::new(),
        unknown_tags: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    fn tool(name: &str, schema_json: &str) -> ToolCatalogEntry {
        ToolCatalogEntry {
            name: name.to_string(),
            description: None,
            input_schema: schema(schema_json),
            output_schema: None,
        }
    }

    fn tool_out(name: &str, in_json: &str, out_json: &str) -> ToolCatalogEntry {
        ToolCatalogEntry {
            name: name.to_string(),
            description: None,
            input_schema: schema(in_json),
            output_schema: Some(schema(out_json)),
        }
    }

    /// Just the definitions, for tests that don't care about warnings.
    fn defs_of(server: &str, tools: &[ToolCatalogEntry]) -> PackageDeclaration {
        build_mcp_definitions(server, tools, None).0
    }

    /// The structural fields of a tool's single `args` parameter.
    fn args_fields<'a>(
        defs: &'a PackageDeclaration,
        tool: &str,
    ) -> &'a BTreeMap<String, ObjectField> {
        let ValueKind::Function { params, .. } = &defs.values[tool].kind else {
            panic!("expected function");
        };
        match &params[0].ty {
            Type::Object { fields } => fields,
            other => panic!("expected an object arg, got {other:?}"),
        }
    }

    fn func_ret(defs: &PackageDeclaration, name: &str) -> Type {
        let ValueKind::Function { ret, .. } = &defs.values[name].kind else {
            panic!("expected function");
        };
        ret.clone()
    }

    /// A tool's single `args` parameter.
    fn args_param_of<'a>(defs: &'a PackageDeclaration, tool: &str) -> &'a Param {
        let ValueKind::Function { params, .. } = &defs.values[tool].kind else {
            panic!("expected function");
        };
        &params[0]
    }

    #[test]
    fn package_is_mcp_and_named() {
        let defs = defs_of("linear", &[]);
        assert_eq!(defs.package_name, "@mcp/linear");
        assert_eq!(defs.mcp_server.as_deref(), Some("linear"));
        assert!(defs.values.is_empty());
    }

    #[test]
    fn object_schema_becomes_structural_args_with_required_split() {
        let defs = defs_of(
            "linear",
            &[tool(
                "listIssues",
                r#"{"type":"object",
                    "properties":{"team":{"type":"string"},"status":{"type":"string"}},
                    "required":["team"]}"#,
            )],
        );
        // No outputSchema → unknown return.
        assert_eq!(func_ret(&defs, "listIssues"), Type::Unknown);
        let fields = args_fields(&defs, "listIssues");
        assert_eq!(fields["team"].ty, Type::String);
        assert!(!fields["team"].optional);
        assert_eq!(fields["status"].ty, Type::String);
        assert!(fields["status"].optional);
    }

    #[test]
    fn all_optional_args_param_carries_empty_object_default() {
        // No `required` → `args` is optional (zero-arg call type-checks).
        let defs = defs_of(
            "github",
            &[tool(
                "get_teams",
                r#"{"type":"object","properties":{"user":{"type":"string"}}}"#,
            )],
        );
        let arg = args_param_of(&defs, "get_teams");
        assert_eq!(arg.default, Some(DefaultValue::EmptyObject));
        // The structural type is unchanged — still an object with an optional field.
        let fields = args_fields(&defs, "get_teams");
        assert!(fields["user"].optional);
    }

    #[test]
    fn required_field_keeps_args_param_required() {
        let defs = defs_of(
            "linear",
            &[tool(
                "listIssues",
                r#"{"type":"object","properties":{"team":{"type":"string"}},"required":["team"]}"#,
            )],
        );
        assert_eq!(args_param_of(&defs, "listIssues").default, None);
    }

    #[test]
    fn optionality_is_reflected_in_rendered_signature() {
        let defs = defs_of(
            "github",
            &[
                tool(
                    "get_teams",
                    r#"{"type":"object","properties":{"user":{"type":"string"}}}"#,
                ),
                tool(
                    "listIssues",
                    r#"{"type":"object","properties":{"team":{"type":"string"}},"required":["team"]}"#,
                ),
            ],
        );
        let decls = interpreter::packages::render_declarations(&defs);
        assert!(
            decls.contains("function get_teams(args?: { user?: string }): unknown;"),
            "all-optional tool should render an optional args param:\n{decls}"
        );
        assert!(
            decls.contains("function listIssues(args: { team: string }): unknown;"),
            "tool with a required field should keep a required args param:\n{decls}"
        );
    }

    #[test]
    fn nested_object_becomes_nested_structural_object() {
        let defs = defs_of(
            "x",
            &[tool(
                "search",
                r#"{"type":"object","properties":{
                    "filter":{"type":"object","properties":{"since":{"type":"string"}},"required":["since"]}
                }}"#,
            )],
        );
        let mut inner = BTreeMap::new();
        inner.insert("since".to_string(), ObjectField::required(Type::String));
        assert_eq!(
            args_fields(&defs, "search")["filter"].ty,
            Type::Object { fields: inner }
        );
    }

    #[test]
    fn string_enum_becomes_literal_union() {
        let defs = defs_of(
            "x",
            &[tool(
                "setState",
                r#"{"type":"object","properties":{"state":{"type":"string","enum":["open","closed"]}},"required":["state"]}"#,
            )],
        );
        assert_eq!(
            args_fields(&defs, "setState")["state"].ty,
            Type::Union(vec![
                Type::StringLiteral("open".to_string()),
                Type::StringLiteral("closed".to_string()),
            ])
        );
    }

    #[test]
    fn typed_array_carries_element_type() {
        let defs = defs_of(
            "x",
            &[tool(
                "tagger",
                r#"{"type":"object","properties":{"tags":{"type":"array","items":{"type":"string"}}}}"#,
            )],
        );
        assert_eq!(
            args_fields(&defs, "tagger")["tags"].ty,
            Type::Array(Box::new(Type::String))
        );
    }

    #[test]
    fn integer_maps_to_number() {
        let defs = defs_of(
            "x",
            &[tool(
                "page",
                r#"{"type":"object","properties":{"limit":{"type":"integer"}}}"#,
            )],
        );
        assert_eq!(args_fields(&defs, "page")["limit"].ty, Type::Number);
    }

    #[test]
    fn scalar_combinator_arg_becomes_union() {
        // anyOf/oneOf of scalars (e.g. a nullable field) maps to a union, not a drop
        // — this is the Linear `list_issues` `assignee: string | null` case.
        let defs = defs_of(
            "x",
            &[tool(
                "listIssues",
                r#"{"type":"object","properties":{
                    "assignee":{"anyOf":[{"type":"string"},{"type":"null"}]},
                    "limit":{"oneOf":[{"type":"integer"},{"type":"null"}]}
                }}"#,
            )],
        );
        assert_eq!(
            args_fields(&defs, "listIssues")["assignee"].ty,
            Type::Union(vec![Type::String, Type::Null])
        );
        assert_eq!(
            args_fields(&defs, "listIssues")["limit"].ty,
            Type::Union(vec![Type::Number, Type::Null])
        );
    }

    #[test]
    fn object_combinator_arg_drops_the_tool() {
        // A combinator with object members has no scalar union form → drop.
        let (defs, warnings) = build_mcp_definitions(
            "x",
            &[tool(
                "weird",
                r#"{"type":"object","properties":{"v":{"oneOf":[
                    {"type":"object","properties":{"a":{"type":"string"}}},
                    {"type":"object","properties":{"b":{"type":"string"}}}
                ]}}}"#,
            )],
            None,
        );
        assert!(!defs.values.contains_key("weird"), "tool must be dropped");
        assert!(warnings[0].message.contains("dropped"));
        assert_eq!(warnings[0].tool, "weird");
    }

    #[test]
    fn nullable_multi_type_field_becomes_a_union() {
        // An optional/nullable field (`["string","null"]`) is a union, not a drop.
        let defs = defs_of(
            "x",
            &[tool(
                "t",
                r#"{"type":"object","properties":{"v":{"type":["string","null"]}}}"#,
            )],
        );
        assert_eq!(
            args_fields(&defs, "t")["v"].ty,
            Type::Union(vec![Type::String, Type::Null])
        );
    }

    #[test]
    fn object_multi_type_arg_drops_the_tool() {
        // A multi-type carrying a non-primitive (`object`) has no field info → drop.
        let (defs, warnings) = build_mcp_definitions(
            "x",
            &[tool(
                "t",
                r#"{"type":"object","properties":{"v":{"type":["object","null"]}}}"#,
            )],
            None,
        );
        assert!(!defs.values.contains_key("t"));
        assert!(warnings[0].message.contains("dropped"));
    }

    #[test]
    fn empty_or_absent_schema_is_zero_arg() {
        let defs = defs_of(
            "x",
            &[
                tool("ping", r#"{"type":"object"}"#),
                tool("pong", r#"{"type":"object","properties":{}}"#),
            ],
        );
        for name in ["ping", "pong"] {
            let ValueKind::Function { params, .. } = &defs.values[name].kind else {
                panic!("function");
            };
            assert!(params.is_empty(), "{name} should be zero-arg");
        }
    }

    #[test]
    fn representable_output_schema_becomes_typed_return() {
        let (defs, warnings) = build_mcp_definitions(
            "x",
            &[tool_out(
                "getUser",
                r#"{"type":"object"}"#,
                r#"{"type":"object","properties":{"id":{"type":"string"},"age":{"type":"integer"}},"required":["id","age"]}"#,
            )],
            None,
        );
        let mut fields = BTreeMap::new();
        fields.insert("id".to_string(), ObjectField::required(Type::String));
        fields.insert("age".to_string(), ObjectField::required(Type::Number));
        assert_eq!(func_ret(&defs, "getUser"), Type::Object { fields });
        assert!(warnings.is_empty(), "fully typed tool warns nothing");
    }

    #[test]
    fn typed_return_registers_object_shapes() {
        // Codegen needs every structural shape (here the return object) on
        // `defs.shapes` to emit its subtype/vtable machinery.
        let (defs, _) = build_mcp_definitions(
            "x",
            &[tool_out(
                "getUser",
                r#"{"type":"object"}"#,
                r#"{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}"#,
            )],
            None,
        );
        let mut fields = BTreeMap::new();
        fields.insert("id".to_string(), ObjectField::required(Type::String));
        assert!(
            defs.shapes.contains(&Shape::Object { fields }),
            "return object shape must be registered: {:?}",
            defs.shapes
        );
    }

    #[test]
    fn missing_output_schema_returns_unknown_with_warning() {
        let (defs, warnings) =
            build_mcp_definitions("x", &[tool("act", r#"{"type":"object"}"#)], None);
        assert_eq!(func_ret(&defs, "act"), Type::Unknown);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("unknown"));
    }

    #[test]
    fn unrepresentable_output_schema_returns_unknown() {
        // `$ref` has no resolver here → unrepresentable → unknown fallback.
        let (defs, warnings) = build_mcp_definitions(
            "x",
            &[tool_out(
                "act",
                r#"{"type":"object"}"#,
                r#"{"$ref":"Thing"}"#,
            )],
            None,
        );
        assert_eq!(func_ret(&defs, "act"), Type::Unknown);
        assert!(warnings[0].message.contains("unknown"));
    }

    #[test]
    fn bare_string_output_is_typed_string() {
        let (defs, warnings) = build_mcp_definitions(
            "x",
            &[tool_out(
                "act",
                r#"{"type":"object"}"#,
                r#"{"type":"string"}"#,
            )],
            None,
        );
        assert_eq!(func_ret(&defs, "act"), Type::String);
        assert!(warnings.is_empty());
    }

    #[test]
    fn tool_description_flows_to_doc() {
        let defs = defs_of(
            "x",
            &[ToolCatalogEntry {
                name: "go".to_string(),
                description: Some("Kick it off.".to_string()),
                input_schema: schema(r#"{"type":"object"}"#),
                output_schema: None,
            }],
        );
        let ValueKind::Function { doc, .. } = &defs.values["go"].kind else {
            panic!("function");
        };
        let summary = &doc.as_ref().unwrap().summary;
        assert!(summary.contains("Kick it off."));
        assert!(summary.contains("did not publish an outputSchema"));
    }

    #[test]
    fn missing_output_schema_is_documented() {
        let defs = defs_of("x", &[tool("act", r#"{"type":"object"}"#)]);
        let decls = interpreter::packages::render_declarations(&defs);
        assert!(
            decls.contains(
                "this MCP server did not publish an outputSchema — cast to a declared type (`as T`)"
            ),
            "declarations should explain missing outputSchema and prescribe a cast:\n{decls}"
        );
        assert!(
            decls.contains("function act(): unknown;"),
            "missing outputSchema should use unknown return:\n{decls}"
        );
    }

    #[test]
    fn representable_output_schema_is_documented() {
        let defs = defs_of(
            "x",
            &[tool_out(
                "getUser",
                r#"{"type":"object"}"#,
                r#"{"type":"object","properties":{"id":{"type":"string"},"age":{"type":"integer"}},"required":["id","age"]}"#,
            )],
        );
        let decls = interpreter::packages::render_declarations(&defs);
        assert!(
            decls.contains("Typed — use the result directly"),
            "a published outputSchema should read as typed use-directly:\n{decls}"
        );
        assert!(
            !decls.contains("Published outputSchema shape:"),
            "the doc comment must not repeat the type body:\n{decls}"
        );
        assert!(
            decls.contains("function getUser(): { age: number; id: string };"),
            "existing typed output behavior should remain:\n{decls}"
        );
    }

    #[test]
    fn unrepresentable_output_schema_is_documented() {
        let defs = defs_of(
            "x",
            &[tool_out(
                "act",
                r#"{"type":"object"}"#,
                r#"{"$ref":"Thing"}"#,
            )],
        );
        let decls = interpreter::packages::render_declarations(&defs);
        assert!(
            decls.contains(
                "Returns `unknown` (the server's outputSchema is not representable) — cast to a declared type"
            ),
            "declarations should explain unrepresentable outputSchema and prescribe a cast:\n{decls}"
        );
        assert!(
            decls.contains("function act(): unknown;"),
            "unrepresentable outputSchema should use unknown return:\n{decls}"
        );
    }

    /// A single-tool [`SchemaPack`] for overlay tests.
    fn pack(id: &'static str, tool: &str, schema_json: &str) -> SchemaPack {
        let mut schemas = BTreeMap::new();
        schemas.insert(tool.to_string(), schema(schema_json));
        SchemaPack::for_test(id, schemas)
    }

    #[test]
    fn registry_overlay_types_a_tool_with_no_output_schema() {
        let p = pack(
            "github",
            "act",
            r#"{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}"#,
        );
        let (defs, warnings) =
            build_mcp_definitions("x", &[tool("act", r#"{"type":"object"}"#)], Some(&p));
        let mut fields = BTreeMap::new();
        fields.insert("id".to_string(), ObjectField::required(Type::String));
        assert_eq!(func_ret(&defs, "act"), Type::Object { fields });
        assert!(
            warnings.is_empty(),
            "an overlaid tool must not warn about an untyped output"
        );
    }

    #[test]
    fn published_output_schema_wins_over_registry() {
        // Server publishes a representable schema; the registry must not override it.
        let p = pack(
            "github",
            "act",
            r#"{"type":"object","properties":{"fromPack":{"type":"string"}},"required":["fromPack"]}"#,
        );
        let (defs, _) = build_mcp_definitions(
            "x",
            &[tool_out(
                "act",
                r#"{"type":"object"}"#,
                r#"{"type":"object","properties":{"fromServer":{"type":"string"}},"required":["fromServer"]}"#,
            )],
            Some(&p),
        );
        let mut fields = BTreeMap::new();
        fields.insert(
            "fromServer".to_string(),
            ObjectField::required(Type::String),
        );
        assert_eq!(func_ret(&defs, "act"), Type::Object { fields });
    }

    #[test]
    fn published_bare_string_wins_over_registry_overlay() {
        // A bare `string` output is a gap the registry may fill.
        let p = pack(
            "github",
            "act",
            r#"{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}"#,
        );
        let (defs, warnings) = build_mcp_definitions(
            "x",
            &[tool_out(
                "act",
                r#"{"type":"object"}"#,
                r#"{"type":"string"}"#,
            )],
            Some(&p),
        );
        assert_eq!(func_ret(&defs, "act"), Type::String);
        assert!(warnings.is_empty());
    }

    #[test]
    fn registry_overlay_is_documented_with_its_pack() {
        let p = pack(
            "github",
            "act",
            r#"{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}"#,
        );
        let (defs, _) =
            build_mcp_definitions("x", &[tool("act", r#"{"type":"object"}"#)], Some(&p));
        let decls = interpreter::packages::render_declarations(&defs);
        assert!(
            decls.contains("Typed — use the result directly"),
            "a schema-pack overlay should read as typed use-directly:\n{decls}"
        );
        assert!(
            !decls.contains("schema pack"),
            "the LLM-facing doc line must not explain why the tool is typed:\n{decls}"
        );
        assert!(
            decls.contains("function act(): { id: string };"),
            "overlaid tool should expose the typed return:\n{decls}"
        );
    }

    #[test]
    fn registry_miss_leaves_the_unknown_fallback() {
        // The pack covers a different tool, so this one stays untyped.
        let p = pack(
            "github",
            "other",
            r#"{"type":"object","properties":{"id":{"type":"string"}}}"#,
        );
        let (defs, warnings) =
            build_mcp_definitions("x", &[tool("act", r#"{"type":"object"}"#)], Some(&p));
        assert_eq!(func_ret(&defs, "act"), Type::Unknown);
        assert!(warnings[0].message.contains("unknown"));
    }

    #[test]
    fn vendored_github_pack_types_a_default_github_mcp_tool() {
        // End-to-end over the real vendored data: the GitHub host resolves to the
        // `github` pack, and `get_me` — which a default github-mcp-server advertises
        // with no outputSchema — gets the MinimalUser shape as a typed return.
        let pack = crate::mcp::schema_registry::pack_for_url("https://api.githubcopilot.com/mcp/")
            .expect("the GitHub host should resolve to a schema pack");
        let entry = tool("get_me", r#"{"type":"object","properties":{}}"#);
        let (defs, warnings) = build_mcp_definitions("gh", &[entry], Some(pack));

        let Type::Object { fields } = func_ret(&defs, "get_me") else {
            panic!("get_me should have a structured return");
        };
        assert_eq!(
            fields.get("login"),
            Some(&ObjectField::required(Type::String))
        );
        assert_eq!(fields.get("id"), Some(&ObjectField::optional(Type::Number)));
        assert!(
            warnings.is_empty(),
            "an overlaid tool must not warn about an untyped output"
        );

        let decls = interpreter::packages::render_declarations(&defs);
        assert!(
            decls.contains("Typed — use the result directly"),
            "an overlaid tool should read as typed use-directly:\n{decls}"
        );
    }

    #[test]
    fn deeply_nested_arg_drops_the_tool_without_overflow() {
        // A chain deeper than MAX_DEPTH isn't representable, so the tool is
        // dropped — and the mapper must terminate rather than recurse forever.
        let mut s = String::from(r#"{"type":"string"}"#);
        for _ in 0..40 {
            s = format!(r#"{{"type":"object","properties":{{"n":{s}}}}}"#);
        }
        let (defs, warnings) = build_mcp_definitions("x", &[tool("deep", &s)], None);
        assert!(!defs.values.contains_key("deep"));
        assert!(warnings[0].message.contains("dropped"));
    }
}
