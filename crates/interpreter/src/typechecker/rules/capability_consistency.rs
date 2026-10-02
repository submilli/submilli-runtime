use std::collections::{BTreeMap, BTreeSet};

use super::check_calls::{self, CheckCall, SearchRoot};
use super::declarations::{Member, TypeDeclarations};
use crate::compiler_error::CompilerFailure;
use crate::{
    Diagnostic, DocCapabilityBinding, DocCapabilityBindingKind, DocComment, ExprId, Severity, Span,
    StmtId, Type, TypedAst, TypedExprKind, TypedParam, TypedTypeDecl,
};

pub(super) fn run(
    ta: &TypedAst,
    declarations: &TypeDeclarations<'_>,
    diags: &mut Vec<Diagnostic>,
) -> Result<(), CompilerFailure> {
    for callable in callables(ta) {
        if let Some(doc) = callable.doc {
            validate_doc_parser_diagnostics(doc, diags);
            validate_param_bindings(declarations, doc, callable.params, diags);
        }

        let mut calls = Vec::new();
        check_calls::collect(ta, SearchRoot::Stmt(callable.body), &mut calls)?;
        let checks = calls
            .iter()
            .map(|call| security_check(ta, call))
            .collect::<Result<Vec<_>, _>>()?;
        validate_check_tags(callable.doc, &checks, diags);
    }
    Ok(())
}

/// A body whose `check()` calls its `@capability` tags answer for.
struct Callable<'a> {
    doc: Option<&'a DocComment>,
    params: &'a [TypedParam],
    body: StmtId,
}

/// Every function, static methods among them, then every instance method.
///
/// Constructor and accessor tags do not reach the capability schema, so those
/// bodies are not asked for them.
fn callables(ta: &TypedAst) -> impl Iterator<Item = Callable<'_>> {
    let functions = ta.functions.iter().map(|function| Callable {
        doc: function.doc.as_ref(),
        params: &function.params,
        body: function.body,
    });
    let methods = ta
        .types
        .iter()
        .filter_map(|declaration| match declaration {
            TypedTypeDecl::Class(class) => Some(class),
            TypedTypeDecl::Interface(_)
            | TypedTypeDecl::NumberEnum(_)
            | TypedTypeDecl::StringEnum(_)
            | TypedTypeDecl::Alias(_) => None,
        })
        .flat_map(|class| &class.methods)
        .map(|method| Callable {
            doc: method.doc.as_ref(),
            params: &method.params,
            body: method.body,
        });
    functions.chain(methods)
}

fn validate_doc_parser_diagnostics(doc: &DocComment, diags: &mut Vec<Diagnostic>) {
    for cap in &doc.capabilities {
        for diag in &cap.diagnostics {
            diags.push(warning(diag.span, diag.message.clone()));
        }
    }
}

fn validate_param_bindings(
    declarations: &TypeDeclarations<'_>,
    doc: &DocComment,
    params: &[TypedParam],
    diags: &mut Vec<Diagnostic>,
) {
    let params = params
        .iter()
        .map(|p| (p.name.name.as_str(), p))
        .collect::<BTreeMap<_, _>>();
    for cap in &doc.capabilities {
        for binding in &cap.bindings {
            let DocCapabilityBindingKind::Parameter { param, path, span } = &binding.kind else {
                continue;
            };
            let Some(param_decl) = params.get(param.as_str()) else {
                diags.push(warning(
                    *span,
                    format!("unknown parameter binding `${param}` in `@capability`"),
                ));
                continue;
            };
            if let BindingTarget::Missing(missing) =
                binding_target(declarations, &param_decl.ty, path)
            {
                diags.push(warning(
                    *span,
                    format!("unknown field `{missing}` in `@capability` binding `${param}`"),
                ));
            }
        }
    }
}

/// What a `@capability` binding path reads from a value.
pub(super) enum BindingTarget {
    Found(Type),
    /// The first segment that names no field.
    Missing(String),
    /// The path reaches a type whose declaration is not known.
    Unresolved,
}

/// Walks `path` from a value of type `ty`.
pub(super) fn binding_target(
    declarations: &TypeDeclarations<'_>,
    ty: &Type,
    path: &[String],
) -> BindingTarget {
    if names_url_component(ty, path) {
        return BindingTarget::Found(Type::String);
    }
    let mut current = ty.clone();
    for segment in path {
        match declarations.member(&current, segment) {
            Member::Found(member) => current = member,
            Member::Missing => return BindingTarget::Missing(segment.clone()),
            Member::Unresolved => return BindingTarget::Unresolved,
        }
    }
    BindingTarget::Found(current)
}

/// `$url.host` and `$url.path` name a component of the URL a string holds, not
/// a field. The `http` capabilities bind this way, and a call site's filter
/// comes from parsing the URL.
fn names_url_component(ty: &Type, path: &[String]) -> bool {
    matches!(path, [component] if matches!(component.as_str(), "host" | "path"))
        && matches!(ty.peel(), Type::String | Type::StringLiteral(_))
}

#[derive(Debug)]
struct SecurityCheck {
    span: Span,
    capability: Option<(String, Span)>,
    payload_keys: Option<BTreeMap<String, Span>>,
}

fn validate_check_tags(
    doc: Option<&DocComment>,
    checks: &[SecurityCheck],
    diags: &mut Vec<Diagnostic>,
) {
    let Some(doc) = doc else {
        for check in checks {
            if let Some((capability, span)) = &check.capability {
                diags.push(warning(
                    *span,
                    format!("missing `@capability {capability}` for `check()` call"),
                ));
            } else {
                diags.push(warning(
                    check.span,
                    "dynamic capability string in `check()`; use a string literal".to_string(),
                ));
            }
        }
        return;
    };

    let mut checks_by_capability: BTreeMap<&str, Vec<&SecurityCheck>> = BTreeMap::new();
    for check in checks {
        match &check.capability {
            Some((capability, _)) => {
                checks_by_capability
                    .entry(capability.as_str())
                    .or_default()
                    .push(check);
            }
            None => diags.push(warning(
                check.span,
                "dynamic capability string in `check()`; use a string literal".to_string(),
            )),
        }
    }

    for (capability, matching_checks) in &checks_by_capability {
        let tagged = doc
            .capabilities
            .iter()
            .any(|tag| tag.capability == *capability);
        let Some(first) = matching_checks.first().filter(|_| !tagged) else {
            continue;
        };
        let span = first
            .capability
            .as_ref()
            .map_or(first.span, |(_, span)| *span);
        diags.push(warning(
            span,
            format!("missing `@capability {capability}` for `check()` call"),
        ));
    }

    for tag in &doc.capabilities {
        let Some(matching_checks) = checks_by_capability.get(tag.capability.as_str()) else {
            diags.push(warning(
                tag.capability_span,
                format!(
                    "extra `@capability {}` has no matching `check()` call",
                    tag.capability
                ),
            ));
            continue;
        };
        for check in matching_checks {
            let Some(payload_keys) = &check.payload_keys else {
                continue;
            };
            validate_payload_keys(&tag.bindings, payload_keys, diags);
        }
    }
}

fn validate_payload_keys(
    bindings: &[DocCapabilityBinding],
    payload_keys: &BTreeMap<String, Span>,
    diags: &mut Vec<Diagnostic>,
) {
    let binding_keys = bindings
        .iter()
        .map(|binding| binding.field.as_str())
        .collect::<BTreeSet<_>>();
    for (key, span) in payload_keys {
        if !binding_keys.contains(key.as_str()) {
            diags.push(warning(
                *span,
                format!("payload key `{key}` missing from `@capability` binding"),
            ));
        }
    }
    for binding in bindings {
        if !payload_keys.contains_key(binding.field.as_str()) {
            diags.push(warning(
                binding.field_span,
                format!(
                    "`@capability` binding key `{}` is missing from `check()` payload",
                    binding.field
                ),
            ));
        }
    }
}

fn security_check(ta: &TypedAst, call: &CheckCall) -> Result<SecurityCheck, CompilerFailure> {
    Ok(SecurityCheck {
        span: call.span,
        capability: call
            .args
            .first()
            .map(|id| literal_capability(ta, *id))
            .transpose()?
            .flatten(),
        payload_keys: call
            .args
            .get(1)
            .map(|id| literal_payload_keys(ta, *id))
            .transpose()?
            .flatten(),
    })
}

fn literal_capability(
    ta: &TypedAst,
    id: ExprId,
) -> Result<Option<(String, Span)>, CompilerFailure> {
    let expr = ta.try_expr(id).map_err(crate::typechecker::arena_failure)?;
    Ok(match &expr.kind {
        TypedExprKind::String(value) => Some((value.clone(), expr.span)),
        _ => None,
    })
}

fn literal_payload_keys(
    ta: &TypedAst,
    id: ExprId,
) -> Result<Option<BTreeMap<String, Span>>, CompilerFailure> {
    let expr = ta.try_expr(id).map_err(crate::typechecker::arena_failure)?;
    Ok(match &expr.kind {
        TypedExprKind::ObjectLiteral { fields, .. } => Some(
            fields
                .iter()
                .map(|field| (field.name.name.clone(), field.name.span))
                .collect(),
        ),
        _ => None,
    })
}

fn warning(span: Span, message: String) -> Diagnostic {
    Diagnostic {
        severity: Severity::Warning,
        span,
        message,
        help: vec![],
        notes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::{infer_script, run_package};
    use crate::{Diagnostic, PackageDeclaration};

    fn diagnostics(source: &str) -> Vec<Diagnostic> {
        let (ta, mut diags) = infer_script(source);
        diags.extend(crate::check(&ta).unwrap());
        diags
    }

    fn messages(source: &str) -> Vec<String> {
        diagnostics(source)
            .into_iter()
            .map(|diag| diag.message)
            .collect()
    }

    fn package_messages(
        modules: &[(&str, &str)],
        dependencies: &[PackageDeclaration],
    ) -> Vec<String> {
        let (_, _, diags) = run_package("@test/package", modules, dependencies);
        diags.into_iter().map(|diag| diag.message).collect()
    }

    fn capability_messages(messages: Vec<String>) -> Vec<String> {
        messages
            .into_iter()
            .filter(|m| m.contains("@capability") || m.contains("capability string"))
            .collect()
    }

    #[test]
    fn missing_capability_tag_warns() {
        let messages = messages(
            "import { check } from \"submilli:security\";\n\
             function f(): void { check(\"x/op\", { a: 1 }); }\n\
             function main(): void { }\n",
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("missing `@capability x/op`")),
            "{messages:?}"
        );
    }

    #[test]
    fn extra_capability_tag_warns() {
        let messages = messages(
            "import { check } from \"submilli:security\";\n\
             /** @capability x/op { a } */\n\
             function f(a: number): void { }\n\
             function main(): void { }\n",
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("extra `@capability x/op`")),
            "{messages:?}"
        );
    }

    #[test]
    fn payload_key_missing_from_binding_warns() {
        let messages = messages(
            "import { check } from \"submilli:security\";\n\
             /** @capability x/op { a } */\n\
             function f(a: number, b: number): void { check(\"x/op\", { a, b }); }\n\
             function main(): void { }\n",
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("payload key `b` missing")),
            "{messages:?}"
        );
    }

    #[test]
    fn binding_key_missing_from_payload_warns() {
        let messages = messages(
            "import { check } from \"submilli:security\";\n\
             /** @capability x/op { a, c } */\n\
             function f(a: number): void { check(\"x/op\", { a }); }\n\
             function main(): void { }\n",
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("binding key `c` is missing")),
            "{messages:?}"
        );
    }

    #[test]
    fn unknown_parameter_binding_warns() {
        let messages = messages(
            "import { check } from \"submilli:security\";\n\
             /** @capability x/op { foo: $bar } */\n\
             function f(): void { check(\"x/op\", { foo: 1 }); }\n\
             function main(): void { }\n",
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("unknown parameter binding `$bar`")),
            "{messages:?}"
        );
    }

    #[test]
    fn dynamic_capability_string_warns() {
        let messages = messages(
            "import { check } from \"submilli:security\";\n\
             function f(): void { let cap = \"x/op\"; check(cap, { a: 1 }); }\n\
             function main(): void { }\n",
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("dynamic capability string")),
            "{messages:?}"
        );
    }

    #[test]
    fn documented_multiple_checks_do_not_warn() {
        let messages = messages(
            "import { check } from \"submilli:security\";\n\
             /**\n\
              * @param a A.\n\
              * @param b B.\n\
              * @capability x/a { a }\n\
              * @capability x/b { b }\n\
              */\n\
             function f(a: number, b: number): void {\n\
               check(\"x/a\", { a });\n\
               check(\"x/b\", { b });\n\
             }\n\
             function main(): void { }\n",
        );
        assert!(
            !messages
                .iter()
                .any(|m| m.contains("@capability") || m.contains("capability string")),
            "{messages:?}"
        );
    }

    #[test]
    fn a_path_through_an_inline_object_names_its_missing_field() {
        let messages = capability_messages(messages(
            "import { check } from \"submilli:security\";\n\
             /** @capability x/op { owner: $input.team.idd } */\n\
             function f(input: { team: { id: string } }): void {\n\
               check(\"x/op\", { owner: input.team.id });\n\
             }\n\
             function main(): void { }\n",
        ));
        assert_eq!(
            messages,
            ["unknown field `idd` in `@capability` binding `$input`"]
        );
    }

    #[test]
    fn a_path_through_an_alias_a_nullable_and_an_interface_resolves() {
        let messages = capability_messages(messages(
            "import { check } from \"submilli:security\";\n\
             interface Team { id: string }\n\
             interface Input { team: Team | null; label?: string }\n\
             type Aliased = { team: Team };\n\
             /**\n\
              * @capability x/interface { owner: $input.team.id, label: $input.label }\n\
              * @capability x/alias { owner: $aliased.team.id }\n\
              * @capability x/nullable { owner: $nullable.team.id }\n\
              */\n\
             function f(input: Input, aliased: Aliased, nullable: Input | null): void {\n\
               check(\"x/interface\", { owner: \"a\", label: \"b\" });\n\
               check(\"x/alias\", { owner: \"a\" });\n\
               check(\"x/nullable\", { owner: \"a\" });\n\
             }\n\
             function main(): void { }\n",
        ));
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn a_url_component_of_a_string_parameter_is_not_a_field() {
        let messages = capability_messages(messages(
            "import { check } from \"submilli:security\";\n\
             type Address = string;\n\
             /** @capability x/op { host: $url.host, path: $address.path } */\n\
             function f(url: string, address: Address): void {\n\
               check(\"x/op\", { host: \"h\", path: \"p\" });\n\
             }\n\
             function main(): void { }\n",
        ));
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn only_host_and_path_are_components_of_a_string() {
        let messages = capability_messages(messages(
            "import { check } from \"submilli:security\";\n\
             /** @capability x/op { port: $url.port, nested: $url.host.name, count: $n.host } */\n\
             function f(url: string, n: number): void {\n\
               check(\"x/op\", { port: 1, nested: \"n\", count: 2 });\n\
             }\n\
             function main(): void { }\n",
        ));
        assert_eq!(
            messages,
            [
                "unknown field `port` in `@capability` binding `$url`",
                "unknown field `host` in `@capability` binding `$url`",
                "unknown field `host` in `@capability` binding `$n`",
            ]
        );
    }

    #[test]
    fn a_documented_method_answers_for_its_checks() {
        let messages = capability_messages(messages(
            "import { check } from \"submilli:security\";\n\
             class Client {\n\
               /** @capability x/tagged { id } */\n\
               tagged(id: string): void { check(\"x/tagged\", { id }); }\n\
               /** @capability x/extra { id } */\n\
               extra(id: string): void { }\n\
               /** Reads. */\n\
               untagged(id: string): void { check(\"x/untagged\", { id }); }\n\
               /** @capability x/payload { id: $input.idd } */\n\
               payload(input: { id: string }): void { check(\"x/payload\", { other: input.id }); }\n\
             }\n\
             function main(): void { }\n",
        ));
        assert_eq!(
            messages,
            [
                "extra `@capability x/extra` has no matching `check()` call",
                "missing `@capability x/untagged` for `check()` call",
                "unknown field `idd` in `@capability` binding `$input`",
                "payload key `other` missing from `@capability` binding",
                "`@capability` binding key `id` is missing from `check()` payload",
            ]
        );
    }

    #[test]
    fn a_constructor_or_an_accessor_is_not_asked_for_tags() {
        let messages = capability_messages(messages(
            "import { check } from \"submilli:security\";\n\
             class Client {\n\
               private token: string;\n\
               constructor(token: string) { check(\"x/create\", {}); this.token = token; }\n\
               get secret(): string { check(\"x/read\", {}); return this.token; }\n\
             }\n\
             function main(): void { }\n",
        ));
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn an_undocumented_method_is_asked_for_tags() {
        let messages = capability_messages(messages(
            "import { check } from \"submilli:security\";\n\
             class Client {\n\
               helper(): void { check(\"x/helper\", {}); }\n\
             }\n\
             function main(): void { }\n",
        ));
        assert_eq!(
            messages,
            ["missing `@capability x/helper` for `check()` call"]
        );
    }

    #[test]
    fn a_static_method_is_a_function() {
        let messages = capability_messages(messages(
            "import { check } from \"submilli:security\";\n\
             class Client {\n\
               static open(id: string): void { check(\"x/open\", { id }); }\n\
             }\n\
             function main(): void { }\n",
        ));
        assert_eq!(
            messages,
            ["missing `@capability x/open` for `check()` call"]
        );
    }

    const PACKAGE_INPUT: &str = "/** What an operation acts on. */\n\
         export interface Input {\n\
           /** Owning team. */\n\
           teamId: string;\n\
         }\n";

    #[test]
    fn a_package_reports_each_inconsistency() {
        let messages = capability_messages(package_messages(
            &[(
                "lib",
                "import { check } from \"submilli:security\";\n\
                 /** Missing. */\n\
                 export function missing(id: string): void { check(\"x/missing\", { id }); }\n\
                 /**\n\
                  * Extra.\n\
                  * @capability x/extra { id }\n\
                  */\n\
                 export function extra(id: string): void { }\n\
                 /**\n\
                  * Payload.\n\
                  * @capability x/payload { id, other: $nobody }\n\
                  */\n\
                 export function payload(id: string): void { check(\"x/payload\", { id, more: 1 }); }\n\
                 /** Dynamic. */\n\
                 export function dynamic(name: string): void { check(name, {}); }\n",
            )],
            &[],
        ));
        assert_eq!(
            messages,
            [
                "missing `@capability x/missing` for `check()` call",
                "extra `@capability x/extra` has no matching `check()` call",
                "unknown parameter binding `$nobody` in `@capability`",
                "payload key `more` missing from `@capability` binding",
                "`@capability` binding key `other` is missing from `check()` payload",
                "dynamic capability string in `check()`; use a string literal",
            ]
        );
    }

    #[test]
    fn a_package_path_resolves_through_an_interface_of_another_module() {
        let lib = "import { check } from \"submilli:security\";\n\
             import { Input } from \"./types\";\n\
             export { Input } from \"./types\";\n\
             /**\n\
              * Reads.\n\
              * @capability x/read { owner: $input.teamId, typo: $input.teamIdd }\n\
              */\n\
             export function read(input: Input | null): void {\n\
               check(\"x/read\", { owner: \"a\", typo: \"b\" });\n\
             }\n";
        let messages = capability_messages(package_messages(
            &[("lib", lib), ("types", PACKAGE_INPUT)],
            &[],
        ));
        assert_eq!(
            messages,
            ["unknown field `teamIdd` in `@capability` binding `$input`"]
        );
    }

    #[test]
    fn a_package_path_resolves_through_an_inherited_property() {
        let messages = capability_messages(package_messages(
            &[(
                "lib",
                "import { check } from \"submilli:security\";\n\
                 /** Base. */\n\
                 export interface Base {\n\
                   /** Owning team. */\n\
                   teamId: string;\n\
                 }\n\
                 /** Input. */\n\
                 export interface Input extends Base {\n\
                   /** Title. */\n\
                   title: string;\n\
                 }\n\
                 /**\n\
                  * Reads.\n\
                  * @capability x/read { owner: $input.teamId, typo: $input.absent }\n\
                  */\n\
                 export function read(input: Input): void {\n\
                   check(\"x/read\", { owner: \"a\", typo: \"b\" });\n\
                 }\n",
            )],
            &[],
        ));
        assert_eq!(
            messages,
            ["unknown field `absent` in `@capability` binding `$input`"]
        );
    }

    #[test]
    fn a_package_path_resolves_through_an_interface_of_a_dependency() {
        let (_, dependency, diags) = run_package("@test/types", &[("lib", PACKAGE_INPUT)], &[]);
        assert!(diags.is_empty(), "{diags:?}");
        let messages = capability_messages(package_messages(
            &[(
                "lib",
                "import { check } from \"submilli:security\";\n\
                 import { Input } from \"@test/types\";\n\
                 /**\n\
                  * Reads.\n\
                  * @capability x/read { owner: $input.teamId, typo: $input.teamIdd }\n\
                  */\n\
                 export function read(input: Input): void {\n\
                   check(\"x/read\", { owner: \"a\", typo: \"b\" });\n\
                 }\n",
            )],
            &[dependency],
        ));
        assert_eq!(
            messages,
            ["unknown field `teamIdd` in `@capability` binding `$input`"]
        );
    }
}
