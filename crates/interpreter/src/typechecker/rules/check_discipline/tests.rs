use super::super::test_util::run_package;
use super::stability::is_primitive;
use crate::compiler_error::CompilerFailure;
use crate::types::LiteralF64;
use crate::{
    Diagnostic, ExprId, Package, Severity, StmtId, Type, TypedAst, TypedExprKind, TypedStmtKind,
    TypedTypeDecl,
};

const UNNAMED_FUNCTION: &str =
    "`check()` is called in a function value that no exported function or exported constant names";

const IMPORT: &str = "import { check } from \"submilli:security\";\n\
     export interface Input { channelId: string; text: string; tags: string[]; options: Options | null }\n\
     export interface Options { unfurl: boolean; notify: string[] }\n\
     function post(channelId: string, text: string): void {}\n\
     function deliver(input: Input): void {}\n";

fn typed(modules: &[(&str, &str)]) -> (TypedAst, Vec<Diagnostic>) {
    let (ta, _, diags) = run_package("@test/package", modules, &[]);
    let errors: Vec<_> = diags
        .iter()
        .filter(|diag| diag.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    (ta, diags)
}

/// What this rule reports on a package, in the order it reports it.
fn findings(modules: &[(&str, &str)]) -> Vec<Diagnostic> {
    let (_, diags) = typed(modules);
    diags
        .into_iter()
        .filter(|diag| diag.message.contains("check()") && !diag.message.contains("@capability"))
        .collect()
}

fn messages_of(modules: &[(&str, &str)]) -> Vec<String> {
    findings(modules)
        .into_iter()
        .map(|diag| diag.message)
        .collect()
}

/// The findings on a root module that starts with [`IMPORT`].
fn messages(body: &str) -> Vec<String> {
    messages_of(&[("lib", &format!("{IMPORT}{body}"))])
}

fn assert_clean(body: &str) {
    let found = messages(body);
    assert!(found.is_empty(), "{found:#?}");
}

fn named(package: &str, name: &str) -> (crate::MangledName, Package, String) {
    (
        crate::mangle::package_symbol(package, name),
        Package(package.to_string()),
        name.to_string(),
    )
}

#[test]
fn a_value_type_is_primitive() {
    let (mangled, package, name) = named("@test/package", "Level");
    for ty in [
        Type::Number,
        Type::NumberLiteral(LiteralF64(1.0)),
        Type::BigInt,
        Type::String,
        Type::StringLiteral("a".to_string()),
        Type::Boolean,
        Type::BooleanLiteral(true),
        Type::Null,
        Type::Void,
        Type::Never,
        Type::Error,
        Type::NumberEnum {
            mangled: mangled.clone(),
            package: package.clone(),
            name: name.clone(),
            member: None,
        },
        Type::StringEnum {
            mangled,
            package,
            name,
            member: None,
        },
        Type::union(vec![Type::String, Type::Null, Type::Number]),
    ] {
        assert!(is_primitive(&ty), "{ty:?}");
    }
}

#[test]
fn a_type_the_caller_can_change_or_run_code_behind_is_not_primitive() {
    let (mangled, package, name) = named("@test/package", "Input");
    for ty in [
        Type::Uint8Array,
        Type::Unknown,
        Type::Function {
            params: Vec::new(),
            ret: Box::new(Type::String),
            predicate: None,
            has_rest: false,
        },
        Type::Object {
            fields: std::collections::BTreeMap::new(),
            index: None,
        },
        Type::Array(Box::new(Type::String)),
        Type::Tuple(vec![Type::String, Type::Number]),
        Type::TypeVar("T".to_string()),
        Type::GenericParam {
            id: 0,
            name: "T".to_string(),
        },
        Type::interface_ref(package.clone(), &name, mangled.clone(), Vec::new()),
        Type::ClassRef {
            mangled: mangled.clone(),
            package: package.clone(),
            name: name.clone(),
            args: Vec::new(),
        },
        Type::alias_ref(package.clone(), &name, mangled.clone(), Vec::new()),
        Type::union(vec![Type::String, Type::Array(Box::new(Type::String))]),
    ] {
        assert!(!is_primitive(&ty), "{ty:?}");
    }
}

#[test]
fn a_wrapper_is_judged_by_what_it_wraps() {
    let (mangled, package, name) = named("@test/package", "Id");
    let alias = |ty: Type| Type::Alias {
        mangled: mangled.clone(),
        package: package.clone(),
        name: name.clone(),
        args: Vec::new(),
        ty: Box::new(ty),
    };
    let strings = Type::Array(Box::new(Type::String));
    assert!(is_primitive(&alias(Type::String)));
    assert!(!is_primitive(&alias(strings.clone())));
    assert!(!is_primitive(&Type::Readonly(Box::new(strings.clone()))));
    let refined = |ty: Type| Type::Refined {
        original: Box::new(Type::TypeVar("T".to_string())),
        ty: Box::new(ty),
    };
    assert!(is_primitive(&refined(Type::String)));
    assert!(!is_primitive(&refined(strings)));
}

#[test]
fn a_package_that_does_not_import_security_is_skipped() {
    let (ta, diags) = typed(&[(
        "lib",
        "function check(capability: string, context: { id: string }): void {}\n\
         function guard(input: { id: string }): void {\n\
           check(\"x/op\", { id: input.id });\n\
           check(\"x/op\", { id: input.id });\n\
         }\n",
    )]);
    assert!(diags.is_empty(), "{diags:#?}");
    // A corrupt body is never reached.
    let mut corrupt = ta;
    corrupt.functions[0].body = StmtId(u32::MAX);
    let mut found = Vec::new();
    super::run(&corrupt, &mut found).unwrap();
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn a_body_without_a_check_is_not_analysed() {
    assert_clean(
        "export function send(input: Input): Input {\n\
           post(input.channelId, input.text);\n\
           post(input.channelId, input.tags.join(\",\"));\n\
           deliver(input);\n\
           return input;\n\
         }\n\
         export function other(channelId: string): void { check(\"x/other\", { channelId }); }\n",
    );
}

#[test]
fn an_invalid_node_id_is_an_internal_failure() {
    let (original, _) = typed(&[(
        "lib",
        &format!(
            "{IMPORT}export function send(input: Input): string {{\n\
               check(\"x/send\", {{ channelId: input.channelId }});\n\
               return input.text;\n\
             }}\n"
        ),
    )]);
    let send = original
        .functions
        .iter()
        .position(|function| function.name.name == "send")
        .unwrap();

    let mut corrupt_statement = original.clone();
    corrupt_statement.functions[send].body = StmtId(u32::MAX);

    let mut corrupt_expression = original.clone();
    let mut replaced = false;
    for id in corrupt_expression.stmt_ids().unwrap() {
        if let TypedStmtKind::Return(value) = &mut corrupt_expression.try_stmt_mut(id).unwrap().kind
        {
            *value = Some(ExprId(u32::MAX));
            replaced = true;
        }
    }
    assert!(replaced);

    for corrupt in [corrupt_statement, corrupt_expression] {
        let failure = super::run(&corrupt, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(failure, CompilerFailure::Internal { .. }),
            "{failure}"
        );
    }
    super::run(&original, &mut Vec::new()).unwrap();
}

#[test]
fn an_invalid_member_body_or_initializer_is_an_internal_failure() {
    let (original, _) = typed(&[(
        "lib",
        &format!(
            "{IMPORT}export class Client {{\n\
               label: string = \"a\";\n\
               send(channelId: string): void {{ check(\"x/send\", {{ channelId }}); }}\n\
             }}\n"
        ),
    )]);
    let mut corrupt_method = original.clone();
    first_class(&mut corrupt_method).methods[0].body = StmtId(u32::MAX);
    let mut corrupt_initializer = original;
    first_class(&mut corrupt_initializer).fields[0].initializer = Some(ExprId(u32::MAX));
    for corrupt in [corrupt_method, corrupt_initializer] {
        let failure = super::run(&corrupt, &mut Vec::new()).unwrap_err();
        assert!(
            matches!(failure, CompilerFailure::Internal { .. }),
            "{failure}"
        );
    }
}

#[test]
fn a_cycle_of_wrappers_is_an_internal_failure() {
    let (mut corrupt, _) = typed(&[(
        "lib",
        &format!(
            "{IMPORT}export type Send = (channelId: string) => void;\n\
             export const send = ((channelId: string): void => {{\n\
               check(\"x/send\", {{ channelId }});\n\
             }}) as Send;\n"
        ),
    )]);
    super::run(&corrupt, &mut Vec::new()).unwrap();

    let mut wrapped = false;
    for id in corrupt.expr_ids().unwrap() {
        if let TypedExprKind::Cast { value, .. } = &mut corrupt.try_expr_mut(id).unwrap().kind {
            *value = id;
            wrapped = true;
        }
    }
    assert!(wrapped);

    let failure = super::run(&corrupt, &mut Vec::new()).unwrap_err();
    assert!(
        matches!(failure, CompilerFailure::Internal { span: Some(_), .. }),
        "{failure}"
    );
    assert!(failure.to_string().contains("form a cycle"), "{failure}");
}

fn first_class(ta: &mut TypedAst) -> &mut crate::TypedClassDecl {
    ta.types
        .iter_mut()
        .find_map(|declaration| match declaration {
            TypedTypeDecl::Class(class) => Some(class),
            _ => None,
        })
        .expect("a class")
}

#[test]
fn a_check_in_an_exported_function_is_well_placed() {
    assert_clean(
        "export function send(channelId: string): void { check(\"x/send\", { channelId }); }\n",
    );
}

#[test]
fn a_check_in_an_unexported_function_is_reported_once() {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}function guard(channelId: string): void {{\n\
               check(\"x/a\", {{ channelId }});\n\
               check(\"x/b\", {{ channelId }});\n\
             }}\n\
             export function send(channelId: string): void {{ guard(channelId); }}\n"
        ),
    )]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].message,
        "`check()` is called in `guard`, which is not part of the package's public API"
    );
    assert_eq!(
        found[0].notes[0].1,
        "`guard` is not exported from the package root"
    );
    assert_eq!(
        found[0].help,
        [
            "Move the `check()` into each exported function that reaches `guard`, or export `guard` from the package root"
        ]
    );
}

#[test]
fn a_function_another_module_exports_is_public_only_when_the_root_exports_it() {
    let internal = "import { check } from \"submilli:security\";\n\
         export function guard(id: string): void { check(\"x/guard\", { id }); }\n\
         export function send(id: string): void { check(\"x/send\", { id }); }\n";
    let found = messages_of(&[
        (
            "lib",
            "import { guard } from \"./internal\";\n\
             export { send as deliver } from \"./internal\";\n\
             export function run(id: string): void { guard(id); }\n",
        ),
        ("internal", internal),
    ]);
    assert_eq!(
        found,
        ["`check()` is called in `guard`, which is not part of the package's public API"]
    );
}

#[test]
fn a_check_in_a_nested_function_is_reported_for_each_function() {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}export function send(channelId: string): void {{\n\
               const first = (): void => {{\n\
                 check(\"x/a\", {{ channelId }});\n\
                 check(\"x/b\", {{ channelId }});\n\
               }};\n\
               function second(): void {{ check(\"x/c\", {{ channelId }}); }}\n\
               first();\n\
               second();\n\
             }}\n"
        ),
    )]);
    let messages: Vec<_> = found.iter().map(|diag| diag.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "`check()` is called inside a nested function in `send`",
            "`check()` is called inside a nested function in `send`",
        ]
    );
    assert_eq!(found[0].notes[0].1, "the nested function starts here");
    assert_eq!(
        found[1].notes[0].1,
        "the nested function `second` is declared here"
    );
}

#[test]
fn a_check_outside_a_function_is_reported() {
    let found = messages(
        "check(\"x/load\", {});\n\
         export class Client {\n\
           label: string = ((): string => { check(\"x/field\", {}); return \"a\"; })();\n\
         }\n",
    );
    assert_eq!(
        found,
        ["`check()` is called outside a function", UNNAMED_FUNCTION]
    );
}

#[test]
fn members_of_an_exported_class_and_its_ancestors_are_public() {
    assert_clean(
        "class Root { archive(id: string): void { check(\"x/archive\", { id }); } }\n\
         class Base extends Root { rename(id: string): void { check(\"x/rename\", { id }); } }\n\
         export class Client extends Base {\n\
           private token: string;\n\
           constructor(token: string) { super(); check(\"x/create\", {}); this.token = token; }\n\
           send(id: string): void { check(\"x/send\", { id }); }\n\
           get secret(): string { check(\"x/read\", {}); return this.token; }\n\
           set secret(value: string) { check(\"x/write\", {}); this.token = value; }\n\
           static open(id: string): void { check(\"x/open\", { id }); }\n\
         }\n",
    );
}

#[test]
fn a_private_member_and_a_member_of_an_unexported_class_are_internal() {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}export class Client {{\n\
               private guard(id: string): void {{ check(\"x/guard\", {{ id }}); }}\n\
               private get secret(): string {{ check(\"x/read\", {{}}); return \"s\"; }}\n\
             }}\n\
             class Hidden {{\n\
               constructor() {{ check(\"x/create\", {{}}); }}\n\
               send(id: string): void {{ check(\"x/send\", {{ id }}); }}\n\
             }}\n"
        ),
    )]);
    let reported: Vec<_> = found
        .iter()
        .map(|diag| (diag.message.as_str(), diag.notes[0].1.as_str()))
        .collect();
    let message = |label: &str| {
        format!("`check()` is called in `{label}`, which is not part of the package's public API")
    };
    assert_eq!(
        reported,
        [
            (
                message("Client.guard").as_str(),
                "`Client.guard` is private"
            ),
            (
                message("Client.secret").as_str(),
                "`Client.secret` is private"
            ),
            (
                message("new Hidden").as_str(),
                "`Hidden` is not exported from the package root"
            ),
            (
                message("Hidden.send").as_str(),
                "`Hidden` is not exported from the package root"
            ),
        ]
    );
}

#[test]
fn discipline_is_checked_wherever_the_check_is_placed() {
    let found = messages(
        "function guard(input: Input): void {\n\
           check(\"x/guard\", { channelId: input.channelId });\n\
           post(input.channelId, \"a\");\n\
         }\n",
    );
    assert_eq!(
        found,
        [
            "`check()` is called in `guard`, which is not part of the package's public API",
            "`input.channelId` is read more than once in `guard`, which calls `check()`",
        ]
    );
}

#[test]
fn a_second_read_of_a_property_is_reported_once() {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}export function send(input: Input): void {{\n\
               check(\"x/send\", {{ channelId: input.channelId }});\n\
               post(input.channelId, input.text);\n\
               post(input.channelId, \"again\");\n\
             }}\n"
        ),
    )]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].message,
        "`input.channelId` is read more than once in `send`, which calls `check()`"
    );
    let notes: Vec<_> = found[0].notes.iter().map(|note| note.1.as_str()).collect();
    assert_eq!(
        notes,
        [
            "first read here",
            "`check()` is called here",
            "`input.channelId` reaches `check()` here"
        ]
    );
    assert_eq!(
        found[0].help,
        ["Read it once into a `const`; the caller can return a different value on each read"]
    );
}

#[test]
fn a_reported_read_yields_a_stable_value() {
    let found = messages(
        "export function send(input: Input): void {\n\
           if (input.options !== null) {\n\
             check(\"x/send\", {});\n\
             post(\"a\", input.options.unfurl ? \"a\" : \"b\");\n\
             for (const user of input.options.notify) post(user, \"a\");\n\
           }\n\
         }\n",
    );
    assert_eq!(
        found,
        ["`input.options` is read more than once in `send`, which calls `check()`"]
    );
}

#[test]
fn aliases_share_the_reads_of_the_value_they_name() {
    let found = messages(
        "export function send(input: Input | null): void {\n\
           if (input === null) return;\n\
           check(\"x/send\", { reached: \"text\" in input });\n\
           const alias = input;\n\
           const cast = input as Input;\n\
           post(input.channelId, alias!.text);\n\
           post(alias.channelId, cast.text);\n\
         }\n",
    );
    assert_eq!(
        found,
        [
            "`alias.channelId` is read more than once in `send`, which calls `check()`",
            "`cast.text` is read more than once in `send`, which calls `check()`",
        ]
    );
}

#[test]
fn a_narrowed_path_is_read_at_each_use() {
    let found = messages(
        "export function send(input: Input): void {\n\
           if (input.options !== null) {\n\
             check(\"x/send\", {});\n\
             post(\"a\", input.options.unfurl ? \"a\" : \"b\");\n\
           }\n\
         }\n",
    );
    assert_eq!(
        found,
        ["`input.options` is read more than once in `send`, which calls `check()`"]
    );
}

#[test]
fn a_narrowed_root_is_not_a_read() {
    assert_clean(
        "export function send(options: Options | null, payload: unknown): void {\n\
           check(\"x/send\", { notify: options?.notify.length ?? 0, reached: \"text\" in payload });\n\
           if (options !== null) post(\"a\", options.unfurl ? \"a\" : \"b\");\n\
           if (typeof payload === \"string\") { post(payload, payload); }\n\
         }\n",
    );
}

#[test]
fn a_read_in_a_loop_of_a_value_bound_outside_it_is_reported() {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}export function send(input: Input): void {{\n\
               check(\"x/send\", {{ reached: \"text\" in input }});\n\
               let n = 0;\n\
               while (n < 3) {{ post(\"a\", input.text); n = n + 1; }}\n\
               do {{ post(\"a\", input.channelId); n = n + 1; }} while (n < 6);\n\
               for (let i = 0; i < input.tags.length; i++) {{ post(\"a\", \"b\"); }}\n\
             }}\n"
        ),
    )]);
    let messages: Vec<_> = found.iter().map(|diag| diag.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "`input.text` is read inside a loop in `send`, which calls `check()`",
            "`input.channelId` is read inside a loop in `send`, which calls `check()`",
            "`input.tags` is read inside a loop in `send`, which calls `check()`",
        ]
    );
    assert_eq!(
        found[0].notes[0].1,
        "`input` is bound here, outside the loop"
    );
    assert_eq!(found[0].help, ["Read it once before the loop"]);
}

#[test]
fn an_element_is_read_once_in_the_loop_that_binds_it() {
    assert_clean(
        "export function send(routes: Options[], pairs: Map<string, string[]>): void {\n\
           for (const route of routes) {\n\
             check(\"x/route\", {});\n\
             const unfurl = route.unfurl;\n\
             for (const user of route.notify) post(user, unfurl ? \"a\" : \"b\");\n\
           }\n\
           for (const [key, values] of pairs) {\n\
             check(\"x/pair\", {});\n\
             for (const value of values) post(key, value);\n\
           }\n\
         }\n",
    );
}

#[test]
fn a_second_read_of_the_elements_is_reported() {
    let found = messages(
        "export function send(tags: string[], names: string[], ids: string[]): void {\n\
           for (const tag of tags) check(\"x/tag\", { tag });\n\
           for (const tag of tags) post(tag, \"b\");\n\
           post(names[0], names[1]);\n\
           check(\"x/name\", { name: names[0] });\n\
           for (const id of ids) post(id, \"d\");\n\
           check(\"x/id\", { id: ids[0] });\n\
           post(tags.length > 0 ? \"f\" : \"g\", \"h\");\n\
         }\n",
    );
    assert_eq!(
        found,
        [
            "elements of `tags` are read more than once in `send`, which calls `check()`",
            "elements of `names` are read more than once in `send`, which calls `check()`",
            "elements of `ids` are read more than once in `send`, which calls `check()`",
            "`tags.length` is read more than once in `send`, which calls `check()`",
        ]
    );
}

/// The message of the one escape `body` holds, in a function `send` that
/// passes `reached`, which makes the escaped value reach it, to `check()`.
fn escape(params: &str, reached: &str, body: &str) -> String {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}export class Failure extends Error {{}}\n\
             function keep(tags: string[]): void {{}}\n\
             let held: Input | null = null;\n\
             export function send({params}): void {{\n\
               check(\"x/send\", {{ reached: {reached} }});\n\
               {body}\n\
             }}\n"
        ),
    )]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].notes.len(), 2, "{found:#?}");
    let why = &found[0].notes[1].1;
    assert!(
        why.ends_with("reaches `check()` here") || why.ends_with("is examined"),
        "{found:#?}"
    );
    let message = &found[0].message;
    let reason = if message.contains("` is called in ") {
        "; the caller's function runs the caller's code, which can change the other arguments"
    } else {
        "; a value the caller supplies can change after `check()` approves it"
    };
    assert!(found[0].help[0].ends_with(reason), "{found:#?}");
    let escape = message
        .strip_prefix("caller-supplied ")
        .and_then(|rest| rest.strip_suffix(" in `send`, which calls `check()`"));
    escape.unwrap_or(message).to_string()
}

#[test]
fn each_escape_is_named_by_what_is_done() {
    for (params, reached, body, expected) in [
        (
            "input: Input",
            "input.text",
            "deliver(input);",
            "`input` is passed to `deliver`",
        ),
        (
            "input: Input",
            "\"text\" in input",
            "keep(input.tags);",
            "`input.tags` is passed to `keep`",
        ),
        (
            "input: Input",
            "input.text",
            "check(\"x/send\", input);",
            "`input` is passed to `check()` as its context",
        ),
        (
            "tags: string[]",
            "tags.length",
            "check(\"x/send\", { tags });",
            "`tags` is passed to `check()` as its context",
        ),
        (
            "tags: string[]",
            "tags.length",
            "post(\"a\", tags.join(\",\"));",
            "`tags` has `join` called on it",
        ),
        (
            "routes: Map<string, string>",
            "routes.size",
            "const route = routes.get(\"a\");",
            "`routes` has `get` called on it",
        ),
        ("render: () => string", "render()", "", "`render` is called"),
        (
            "input: Input",
            "input.text",
            "const held = { input };",
            "`input` is stored in an object literal",
        ),
        (
            "input: Input",
            "input.text",
            "const held = [input];",
            "`input` is stored in an array literal",
        ),
        (
            "input: Input",
            "input.text",
            "const held: [Input, string] = [input, \"a\"];",
            "`input` is stored in an array literal",
        ),
        (
            "input: Input",
            "input.text",
            "const body: { input: Input | null } = { input: null }; body.input = input;",
            "`input` is stored in the property `input`",
        ),
        (
            "input: Input",
            "input.text",
            "const all: Input[] = []; all[0] = input;",
            "`input` is stored in an element",
        ),
        (
            "input: Input",
            "input.text",
            "let last: Input | null = null; last = input;",
            "`input` is stored in `last`",
        ),
        (
            "input: Input",
            "input.text",
            "held = input;",
            "`input` is stored in `held`",
        ),
        (
            "shape: { text: string }",
            "shape.text",
            "const copy = { ...shape };",
            "`shape` is spread into a literal",
        ),
        (
            "tags: string[]",
            "tags.length",
            "const copy = [...tags];",
            "`tags` is spread into a literal",
        ),
        (
            "failure: Failure",
            "failure.message",
            "throw failure;",
            "`failure` is thrown",
        ),
        (
            "payload: unknown",
            "\"text\" in payload",
            "post(\"a\", `b${payload}`);",
            "`payload` is passed to `String`",
        ),
        (
            "input: Input",
            "input.channelId",
            "input.text = \"a\";",
            "`input` is written to",
        ),
        (
            "tags: string[]",
            "tags.length",
            "tags[0] = \"a\";",
            "`tags` is written to",
        ),
        (
            "input: Input",
            "input.text",
            "const later = (): void => deliver(input);",
            "`input` is captured by a nested function",
        ),
        (
            "tags: string[]",
            "tags.length",
            "function later(): string[] { return tags; }",
            "`tags` is captured by a nested function",
        ),
    ] {
        assert_eq!(escape(params, reached, body), expected, "{params}: {body}");
    }
}

#[test]
fn a_returned_value_is_an_escape() {
    let found = messages(
        "export function send(input: Input): Input {\n\
           check(\"x/send\", { channelId: input.channelId });\n\
           return input;\n\
         }\n\
         export function tags(input: Input): string[] {\n\
           check(\"x/tags\", { reached: \"text\" in input });\n\
           return input.tags;\n\
         }\n",
    );
    assert_eq!(
        found,
        [
            "caller-supplied `input` is returned in `send`, which calls `check()`",
            "caller-supplied `input.tags` is returned in `tags`, which calls `check()`",
        ]
    );
}

#[test]
fn an_escape_is_reported_once_for_each_kind_with_the_sites_it_stands_for() {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}export function send(input: Input, other: Input): Input {{\n\
               check(\"x/send\", {{ channelId: input.channelId, text: other.text }});\n\
               deliver(input);\n\
               deliver(other);\n\
               deliver(input);\n\
               deliver(input);\n\
               return input;\n\
             }}\n"
        ),
    )]);
    let messages: Vec<_> = found.iter().map(|diag| diag.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "caller-supplied `input` is passed to `deliver` in `send`, which calls `check()` (and 2 more)",
            "caller-supplied `other` is passed to `deliver` in `send`, which calls `check()`",
            "caller-supplied `input` is returned in `send`, which calls `check()`",
        ]
    );
    assert_eq!(found[0].notes[0].1, "`input` is a parameter of `send`");
}

#[test]
fn findings_are_ordered_by_position() {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}export function first(input: Input): void {{\n\
               check(\"x/first\", {{ text: input.text }});\n\
               deliver(input);\n\
               post(input.text, input.text);\n\
             }}\n\
             function second(input: Input): void {{\n\
               deliver(input);\n\
               check(\"x/second\", {{ channelId: input.channelId }});\n\
             }}\n"
        ),
    )]);
    let starts: Vec<_> = found.iter().map(|diag| diag.span.start).collect();
    let mut sorted = starts.clone();
    sorted.sort_unstable();
    assert_eq!(starts, sorted, "{found:#?}");
    assert_eq!(found.len(), 4, "{found:#?}");
}

#[test]
fn a_conditional_passes_its_use_to_what_it_yields() {
    let found = messages(
        "export function one(input: Input | null, flag: boolean): void {\n\
           check(\"x/one\", { channelId: input?.channelId ?? \"\" });\n\
           deliver(flag && input !== null ? input : { channelId: \"a\", text: \"b\", tags: [], options: null });\n\
         }\n\
         export function merged(first: Options | null, second: Options, flag: boolean): void {\n\
           check(\"x/merged\", { unfurl: second.unfurl });\n\
           const either = flag ? first : second;\n\
           const other = first ?? second;\n\
         }\n\
         export function same(input: Input, flag: boolean): void {\n\
           check(\"x/same\", { channelId: input.channelId });\n\
           const either = flag ? input : input;\n\
           post(either.text, \"a\");\n\
         }\n",
    );
    assert_eq!(
        found,
        [
            "caller-supplied `input` is passed to `deliver` in `one`, which calls `check()`",
            "a conditional expression yields more than one caller-supplied value in `merged`, which calls `check()`",
            "a conditional expression yields more than one caller-supplied value in `merged`, which calls `check()`",
        ]
    );
}

#[test]
fn every_kind_of_parameter_is_the_callers() {
    let found = messages(
        "export function send<T>(\n\
           generic: T,\n\
           payload: unknown,\n\
           { tags, ...others }: { tags: string[]; extra: string[]; more: string },\n\
           [head]: string[],\n\
           options: Options | null = null,\n\
           ...rest: string[]\n\
         ): void {\n\
           check(\"x/send\", { generic, payload, options, tags, others, rest });\n\
         }\n",
    );
    let passed = |shown: &str| {
        format!(
            "caller-supplied `{shown}` is passed to `check()` as its context in `send`, which calls `check()`"
        )
    };
    assert_eq!(
        found,
        [
            passed("generic"),
            passed("payload"),
            passed("options"),
            passed("tags"),
            passed("others"),
            passed("rest"),
        ]
    );
}

#[test]
fn this_is_the_callers_but_may_be_written_through() {
    let found = messages(
        "export class Client {\n\
           private token: string;\n\
           private settings: Options;\n\
           private count: number = 0;\n\
           constructor(token: string, settings: Options) {\n\
             check(\"x/create\", { unfurl: settings.unfurl });\n\
             this.token = token;\n\
             this.settings = settings;\n\
           }\n\
           send(channelId: string): Client {\n\
             check(\"x/send\", { channelId, reached: \"token\" in this });\n\
             this.count += 1;\n\
             this.token = channelId;\n\
             this.settings.unfurl = true;\n\
             post(this.token, this.token);\n\
             return this;\n\
           }\n\
         }\n",
    );
    assert_eq!(
        found,
        [
            "caller-supplied `settings` is stored in the property `settings` in `new Client`, which calls `check()`",
            "caller-supplied `this.settings` is written to in `Client.send`, which calls `check()`",
            "`this.token` is read more than once in `Client.send`, which calls `check()`",
            "caller-supplied `this` is returned in `Client.send`, which calls `check()`",
        ]
    );
}

#[test]
fn a_parameter_property_is_stored() {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}export class Client {{\n\
               constructor(private token: string, private settings: Options) {{\n\
                 check(\"x/create\", {{ unfurl: settings.unfurl }});\n\
               }}\n\
             }}\n"
        ),
    )]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].message,
        "caller-supplied `settings` is stored in the property `settings` in `new Client`, which calls `check()`"
    );
    assert_eq!(
        found[0].notes[0].1,
        "`settings` is a parameter of `new Client`"
    );
}

#[test]
fn a_global_is_the_callers_when_the_caller_can_reach_or_the_package_replace_it() {
    let found = messages(
        "const TABLE: Options[] = [{ unfurl: true, notify: [] }];\n\
         const LIMIT = 3;\n\
         export const SHARED: Options[] = [];\n\
         let current: Options[] = [];\n\
         export let count = 0;\n\
         export function send(): void {\n\
           for (const row of TABLE) check(\"x/table\", { unfurl: row.unfurl });\n\
           for (const row of TABLE) post(\"a\", row.notify.join(\",\"));\n\
           for (const row of SHARED) check(\"x/shared\", {});\n\
           for (const row of SHARED) post(\"a\", \"c\");\n\
           for (const row of current) check(\"x/current\", {});\n\
           for (const row of current) post(\"a\", \"e\");\n\
           count = count + LIMIT;\n\
           console.log(\"sent\");\n\
         }\n",
    );
    assert_eq!(
        found,
        [
            "elements of `SHARED` are read more than once in `send`, which calls `check()`",
            "`current` is read more than once in `send`, which calls `check()`",
        ]
    );
}

/// Code the caller runs, such as a getter of `input`, can call an exported
/// function that reassigns a module `let` between two reads of it.
fn acting_as(body: &str) -> String {
    format!(
        "let actingAs: string | null = null;\n\
         export function actAs(user: string | null): void {{ actingAs = user; }}\n\
         export function send(input: Input): string {{\n\
           {body}\n\
         }}\n"
    )
}

#[test]
fn a_module_variable_is_read_from_its_binding_at_each_use() {
    let found = messages(&acting_as(
        "if (actingAs !== null) check(\"x/impersonate\", {});\n\
         const text = input.text;\n\
         return (actingAs ?? \"self\") + \": \" + text;",
    ));
    assert_eq!(
        found,
        ["`actingAs` is read more than once in `send`, which calls `check()`"]
    );
    assert_clean(&acting_as(
        "const user = actingAs;\n\
         if (user !== null) check(\"x/impersonate\", {});\n\
         const text = input.text;\n\
         return (user ?? \"self\") + \": \" + text;",
    ));
    let found = messages(&acting_as(
        "for (const tag of input.tags) {\n\
           if (actingAs !== null) check(\"x/impersonate\", { tag });\n\
         }\n\
         return \"sent\";",
    ));
    assert_eq!(
        found,
        ["`actingAs` is read inside a loop in `send`, which calls `check()`"]
    );
}

#[test]
fn a_module_variable_is_the_callers_whatever_its_type() {
    let found = messages(
        "let limit = 3;\n\
         let current: Options = { unfurl: true, notify: [] };\n\
         export class Config { static mode: string = \"open\"; }\n\
         export function send(input: Input): void {\n\
           if (limit > 0) check(\"x/limit\", {});\n\
           limit++;\n\
           check(\"x/unfurl\", { unfurl: current.unfurl });\n\
           post(input.channelId, current.notify.join(\",\"));\n\
           if (Config.mode === \"closed\") check(\"x/mode\", {});\n\
           post(Config.mode, \"a\");\n\
         }\n",
    );
    assert_eq!(
        found,
        [
            "`limit` is read more than once in `send`, which calls `check()`",
            "`current` is read more than once in `send`, which calls `check()`",
            "`Config.mode` is read more than once in `send`, which calls `check()`",
        ]
    );
}

#[test]
fn a_nested_function_reads_a_module_variable_when_it_runs() {
    let found = messages(&acting_as(
        "const user = actingAs;\n\
         if (user !== null) check(\"x/impersonate\", {});\n\
         const later = (): string => actingAs ?? \"self\";\n\
         return later();",
    ));
    assert_eq!(
        found,
        [
            "caller-supplied `actingAs` is captured by a nested function in `send`, which calls `check()`"
        ]
    );
}

#[test]
fn a_static_field_is_named_with_its_class() {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}export class Cfg {{ static count: number = 0; }}\n\
             export function send(input: Input): void {{\n\
               Cfg.count++;\n\
               check(\"x/count\", {{ n: Cfg.count }});\n\
               post(input.channelId, \"a\");\n\
             }}\n"
        ),
    )]);
    let shown: Vec<_> = found
        .iter()
        .map(|diag| {
            (
                diag.message.as_str(),
                diag.notes.last().map(|(_, note)| note.as_str()),
            )
        })
        .collect();
    assert_eq!(
        shown,
        [(
            "`Cfg.count` is read more than once in `send`, which calls `check()`",
            Some(
                "`send` changes the global `Cfg.count` before `check()`, so every caller-supplied value in `send` is examined"
            ),
        )]
    );
}

#[test]
fn a_module_variable_that_does_not_reach_check_may_be_read_again() {
    assert_clean(
        "let calls = 0;\n\
         export function send(input: Input): void {\n\
           const { channelId } = input;\n\
           check(\"x/send\", { channelId });\n\
           calls++;\n\
           post(channelId, \"call \" + String(calls));\n\
         }\n",
    );
}

#[test]
fn a_module_constant_is_read_once_for_all() {
    assert_clean(
        "const LIMIT = 3;\n\
         export class Limits { static readonly MAX: number = 3; }\n\
         const TABLE: Options = { unfurl: true, notify: [] };\n\
         export function send(input: Input): void {\n\
           const { channelId } = input;\n\
           if (LIMIT > 0) check(\"x/limit\", { channelId, unfurl: TABLE.unfurl });\n\
           post(channelId, LIMIT > 1 ? \"a\" : \"b\");\n\
           post(channelId, TABLE.notify.join(\",\"));\n\
           if (Limits.MAX > 0) check(\"x/max\", { channelId });\n\
           post(channelId, Limits.MAX > 1 ? \"a\" : \"b\");\n\
           post(input.text, input.text);\n\
         }\n",
    );
}

#[test]
fn another_packages_variable_is_read_from_its_binding_at_each_use() {
    let (_, state, diags) = run_package(
        "@test/state",
        &[(
            "lib",
            "/** Whom calls act as. */\n\
             export let actingAs: string | null = null;\n\
             /** Chooses whom calls act as.\n\
              * @param user Whom calls act as; `null` for the caller. */\n\
             export function actAs(user: string | null): void { actingAs = user; }\n\
             /** The most calls. */\n\
             export const LIMIT = 3;\n\
             /** The mode. */\n\
             export class Mode {\n\
               /** The current mode. */\n\
               static current: string = \"a\";\n\
               /** The first mode. */\n\
               static readonly FIRST: string = \"a\";\n\
               /** The calls so far. */\n\
               static count: number = 0;\n\
             }\n",
        )],
        &[],
    );
    assert!(diags.is_empty(), "{diags:#?}");
    let (_, _, diags) = run_package(
        "@test/package",
        &[(
            "lib",
            &format!(
                "{IMPORT}import {{ actingAs as who, LIMIT, Mode as M }} from \"@test/state\";\n\
                 export function send(input: Input): string {{\n\
                   if (who !== null) check(\"x/impersonate\", {{}});\n\
                   if (M.current !== M.FIRST) check(\"x/mode\", {{}});\n\
                   const text = input.text;\n\
                   return (who ?? \"self\") + M.current + M.FIRST + text;\n\
                 }}\n\
                 export function bump(input: Input): void {{\n\
                   if (M.count++ > LIMIT) check(\"x/count\", {{}});\n\
                   post(input.text, String(M.count++));\n\
                 }}\n\
                 export function constant(input: Input): void {{\n\
                   if (M.FIRST === \"a\" && LIMIT > 0) check(\"x/first\", {{}});\n\
                   post(input.text, input.text);\n\
                 }}\n"
            ),
        )],
        &[state],
    );
    let found: Vec<_> = diags
        .into_iter()
        .filter(|diag| diag.message.contains("check()") && !diag.message.contains("@capability"))
        .map(|diag| diag.message)
        .collect();
    assert_eq!(
        found,
        [
            "`actingAs` is read more than once in `send`, which calls `check()`",
            "`Mode.current` is read more than once in `send`, which calls `check()`",
            "`Mode.count` is read more than once in `bump`, which calls `check()`",
        ]
    );
}

#[test]
fn a_value_the_package_made_is_stable() {
    assert_clean(
        "function load(channelId: string): Input {\n\
           return { channelId, text: \"a\", tags: [], options: null };\n\
         }\n\
         export function send(channelId: string): Input {\n\
           check(\"x/send\", { channelId });\n\
           const loaded = load(channelId);\n\
           const parsed = JSON.parse(loaded.text) as Input;\n\
           const made: Input = { channelId, text: loaded.text, tags: loaded.tags, options: null };\n\
           const table = new Map<string, Input>();\n\
           table.set(channelId, made);\n\
           post(loaded.channelId, loaded.channelId);\n\
           post(parsed.channelId, parsed.channelId);\n\
           for (const tag of loaded.tags) post(tag, loaded.tags.join(\",\"));\n\
           deliver(loaded);\n\
           try { deliver(made); } catch (error) { post(error.message, error.message); throw error; }\n\
           made.tags.map((tag: string): string => tag + tag).forEach((tag: string): void => post(tag, tag));\n\
           return made;\n\
         }\n",
    );
}

#[test]
fn a_nested_function_that_checks_takes_the_callers_values() {
    let found = messages(
        "export function send(channelId: string): void {\n\
           const inner = (input: Input): void => {\n\
             check(\"x/send\", { channelId: input.channelId });\n\
             post(input.channelId, \"a\");\n\
           };\n\
           const plain = (input: Input): void => {\n\
             post(input.channelId, input.channelId);\n\
           };\n\
         }\n",
    );
    assert_eq!(
        found,
        [
            "`check()` is called inside a nested function in `send`",
            "`input.channelId` is read more than once in `send`, which calls `check()`",
        ]
    );
}

#[test]
fn a_comparison_reads_nothing() {
    assert_clean(
        "export function send(input: Input, other: Input, payload: unknown): void {\n\
           if (input === other || input !== other) check(\"x/send\", {});\n\
           if (input === other) post(\"a\", \"c\");\n\
           if (typeof payload === \"string\" || typeof payload === \"number\") check(\"x/payload\", {});\n\
           if (payload instanceof Error) post(\"a\", \"e\");\n\
           if (!input) post(\"a\", \"f\");\n\
           input;\n\
         }\n",
    );
}

#[test]
fn an_optional_chain_reads_each_step() {
    let found = messages(
        "export function send(input: Input | null, tags: string[] | null): void {\n\
           check(\"x/send\", { unfurl: input?.options?.unfurl ?? false, joined: tags?.join(\",\") ?? \"\" });\n\
           const second = input?.options?.notify;\n\
         }\n",
    );
    assert_eq!(
        found,
        [
            "caller-supplied `tags` has `join` called on it in `send`, which calls `check()`",
            "`input.options` is read more than once in `send`, which calls `check()`",
        ]
    );
}

#[test]
fn a_compound_assignment_reads_its_receiver_once() {
    let found = messages(
        "export interface Counter { inner: { count: number } }\n\
         export function send(counter: Counter): void {\n\
           check(\"x/send\", { reached: \"inner\" in counter });\n\
           counter.inner.count += 1;\n\
         }\n",
    );
    assert_eq!(
        found,
        ["caller-supplied `counter.inner` is written to in `send`, which calls `check()`"]
    );
}

#[test]
fn a_called_function_is_advised_to_be_called_after_the_reads() {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}export function send(to: string, render: () => string): void {{\n\
               const text = render();\n\
               check(\"x/send\", {{ to, text }});\n\
               post(to, text);\n\
             }}\n"
        ),
    )]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].message,
        "caller-supplied `render` is called in `send`, which calls `check()`"
    );
    assert_eq!(
        found[0].help,
        [
            "Read everything `check()` and the request need into `const`s before calling it; \
             the caller's function runs the caller's code, which can change the other arguments"
        ]
    );
}

#[test]
fn an_exported_function_constant_is_a_public_body() {
    let found = messages(
        "export const send = (input: Input): void => {\n\
           check(\"x/send\", { channelId: input.channelId });\n\
           post(input.channelId, \"a\");\n\
         };\n\
         export const tags = (input: Input): string[] => guard(input) ?? input.tags;\n\
         function guard(input: Input): string[] | null { return null; }\n\
         export const clean = function again(channelId: string): void {\n\
           check(\"x/clean\", { channelId });\n\
           const next = again;\n\
         };\n",
    );
    assert_eq!(
        found,
        ["`input.channelId` is read more than once in `send`, which calls `check()`"]
    );
}

#[test]
fn an_expression_bodied_function_constant_is_analysed() {
    let found = messages(
        "function approve(channelId: string): string { return channelId; }\n\
         export const send = (input: Input): string =>\n\
           approve(((): string => {\n\
             const channelId = input.channelId;\n\
             check(\"x/send\", { channelId });\n\
             return channelId;\n\
           })());\n\
         export const list = (input: Input): string[] => {\n\
           check(\"x/list\", { reached: \"text\" in input });\n\
           return input.tags;\n\
         };\n",
    );
    assert_eq!(
        found,
        [
            "caller-supplied `input` is captured by a nested function in `send`, which calls `check()`",
            "`check()` is called inside a nested function in `send`",
            "caller-supplied `input.tags` is returned in `list`, which calls `check()`",
        ]
    );
}

#[test]
fn an_unexported_function_constant_is_not_public() {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}const guard = (channelId: string): void => {{\n\
               check(\"x/send\", {{ channelId }});\n\
             }};\n\
             export function send(channelId: string): void {{ guard(channelId); }}\n"
        ),
    )]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].message,
        "`check()` is called in `guard`, which is not part of the package's public API"
    );
    assert_eq!(
        found[0].notes[0].1,
        "`guard` is not exported from the package root"
    );
}

#[test]
fn a_function_value_no_constant_names_is_reported_as_unnamed() {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}export let send = (channelId: string): void => {{ check(\"x/let\", {{ channelId }}); }};\n\
             export const all = [(channelId: string): void => {{ check(\"x/element\", {{ channelId }}); }}];\n\
             export class Client {{\n\
               static send: (channelId: string) => void = (channelId: string): void => {{ check(\"x/static\", {{ channelId }}); }};\n\
             }}\n"
        ),
    )]);
    let messages: Vec<_> = found.iter().map(|diag| diag.message.as_str()).collect();
    assert_eq!(
        messages,
        [UNNAMED_FUNCTION, UNNAMED_FUNCTION, UNNAMED_FUNCTION]
    );
    for diag in &found {
        assert_eq!(
            diag.help,
            ["Call `check()` directly in the body of an exported function"]
        );
        assert_eq!(diag.notes[0].1, "the function value starts here");
    }
}

#[test]
fn a_wrapped_function_constant_is_analysed() {
    let found = messages(
        "export const cast = ((input: Input): string => {\n\
           check(\"x/cast\", { channelId: input.channelId });\n\
           return input.channelId;\n\
         }) as (input: Input) => string;\n\
         export const asserted = ((input: Input): string => {\n\
           check(\"x/asserted\", { channelId: input.channelId });\n\
           return input.channelId;\n\
         })!;\n\
         export const both = (((input: Input): string => {\n\
           check(\"x/both\", { channelId: input.channelId });\n\
           return input.channelId;\n\
         }) as (input: Input) => string)!;\n",
    );
    assert_eq!(
        found,
        [
            "`input.channelId` is read more than once in `cast`, which calls `check()`",
            "`input.channelId` is read more than once in `asserted`, which calls `check()`",
            "`input.channelId` is read more than once in `both`, which calls `check()`",
        ]
    );
}

#[test]
fn a_conditional_of_function_values_is_not_one_function() {
    let found = messages(
        "const strict: boolean = true;\n\
         export const send = strict\n\
           ? (input: Input): string => { check(\"x/strict\", { channelId: input.channelId }); return input.channelId; }\n\
           : (input: Input): string => { check(\"x/loose\", {}); return input.channelId; };\n",
    );
    assert_eq!(found, [UNNAMED_FUNCTION, UNNAMED_FUNCTION]);
}

#[test]
fn a_value_passed_to_a_function_value_names_no_callee() {
    let found = messages(
        "function pick(): (input: Input) => void { return deliver; }\n\
         export function send(input: Input): void {\n\
           check(\"x/send\", { channelId: input.channelId });\n\
           pick()(input);\n\
         }\n",
    );
    assert_eq!(
        found,
        ["caller-supplied `input` is passed to a function value in `send`, which calls `check()`"]
    );
}

#[test]
fn a_value_that_never_reaches_check_may_be_read_and_passed_on() {
    assert_clean(
        "function forward(options: Options): void {}\n\
         export function send(channelId: string, payload: Options): void {\n\
           check(\"x/send\", { channelId });\n\
           post(channelId, payload.unfurl ? \"a\" : \"b\");\n\
           post(channelId, payload.unfurl ? \"c\" : \"d\");\n\
           for (const user of payload.notify) post(user, \"e\");\n\
           for (const user of payload.notify) post(user, \"f\");\n\
           forward(payload);\n\
         }\n",
    );
}

#[test]
fn a_value_reaches_check_through_what_is_computed_from_it() {
    let read_twice = "`input.channelId` is read more than once in `send`, which calls `check()`";
    for body in [
        "const channelId = input.channelId;\n\
         check(\"x/send\", { channelId });",
        "check(\"x/send\", context(input.channelId, \"a\"));",
        "check(\"x/send\", { target: `channel:${input.channelId}` });",
        "const target = { channelId: input.channelId, text: \"a\" };\n\
         check(\"x/send\", target);",
        "let target = \"\";\n\
         target = input.channelId;\n\
         check(\"x/send\", { target });",
        "const targets: string[] = [];\n\
         targets.push(input.channelId);\n\
         check(\"x/send\", { targets });",
    ] {
        let found = messages(&format!(
            "function context(channelId: string, text: string): {{ channelId: string; text: string }} {{\n\
               return {{ channelId, text }};\n\
             }}\n\
             export function send(input: Input): void {{\n\
               {body}\n\
               post(input.channelId, \"b\");\n\
             }}\n"
        ));
        assert_eq!(found, [read_twice], "{body}");
    }
}

#[test]
fn a_value_that_decides_which_check_runs_reaches_it() {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}export function send(input: Input): void {{\n\
               if (input.text === \"\") {{\n\
                 check(\"x/empty\", {{}});\n\
               }} else {{\n\
                 check(\"x/send\", {{}});\n\
               }}\n\
               post(input.text, \"a\");\n\
             }}\n"
        ),
    )]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].message,
        "`input.text` is read more than once in `send`, which calls `check()`"
    );
    assert_eq!(
        found[0].notes[2].1,
        "`input.text` decides whether `check()` runs here"
    );
    for decider in [
        "input.text === \"\" ? check(\"x/empty\", {}) : check(\"x/send\", {});",
        "for (const tag of input.tags) check(\"x/tag\", {});",
        "const count = input.tags.length;\n\
         for (let n = 0; n < count; n++) check(\"x/tag\", {});",
        "switch (input.text) { case \"a\": check(\"x/a\", {}); break; default: break; }",
    ] {
        let found = messages(&format!(
            "export function send(input: Input): void {{\n\
               {decider}\n\
               deliver(input);\n\
             }}\n"
        ));
        assert_eq!(
            found,
            ["caller-supplied `input` is passed to `deliver` in `send`, which calls `check()`"],
            "{decider}"
        );
    }
}

#[test]
fn a_value_that_holds_a_checked_value_is_subject() {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}export function send(input: Input): void {{\n\
               check(\"x/send\", {{ channelId: input.channelId }});\n\
               deliver(input);\n\
             }}\n"
        ),
    )]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].message,
        "caller-supplied `input` is passed to `deliver` in `send`, which calls `check()`"
    );
    assert_eq!(
        found[0].notes[1].1,
        "`input.channelId` reaches `check()` here"
    );
}

#[test]
fn a_sibling_of_a_checked_value_is_not_subject() {
    assert_clean(
        "function keep(tags: string[]): void {}\n\
         export function send(input: Input): void {\n\
           check(\"x/send\", { channelId: input.channelId });\n\
           keep(input.tags);\n\
           post(input.text, input.text);\n\
           if (input.options !== null) post(\"a\", input.options.unfurl ? \"b\" : \"c\");\n\
         }\n",
    );
}

#[test]
fn an_element_of_a_checked_value_is_subject() {
    let found = messages(
        "export function send(tags: string[], names: string[]): void {\n\
           for (const tag of tags) check(\"x/tag\", {});\n\
           post(tags[0], \"a\");\n\
           check(\"x/names\", { first: names[0] });\n\
           for (const name of names) post(name, \"a\");\n\
         }\n",
    );
    assert_eq!(
        found,
        [
            "elements of `tags` are read more than once in `send`, which calls `check()`",
            "elements of `names` are read more than once in `send`, which calls `check()`",
        ]
    );
}

#[test]
fn the_note_names_the_checked_value_where_it_reaches_check() {
    let source = format!(
        "{IMPORT}export function send(input: Input): void {{\n\
           const alias = input;\n\
           check(\"x/send\", {{ channelId: alias.channelId }});\n\
           post(input.channelId, \"a\");\n\
         }}\n"
    );
    let found = findings(&[("lib", &source)]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].message,
        "`input.channelId` is read more than once in `send`, which calls `check()`"
    );
    let (span, note) = &found[0].notes[2];
    assert_eq!(note, "`input.channelId` reaches `check()` here");
    let start = source.find("{ channelId: alias.channelId }").unwrap();
    let end = start + "{ channelId: alias.channelId }".len();
    assert_eq!(
        (span.start as usize, span.end as usize),
        (start, end),
        "{found:#?}"
    );
}

#[test]
fn a_binding_reassigned_from_its_own_path_settles() {
    let found = messages(
        "export interface Node { value: string; next: Node | null }\n\
         export function send(head: Node): void {\n\
           let node: Node | null = head;\n\
           while (node !== null) {\n\
             check(\"x/send\", { value: node.value });\n\
             node = node.next;\n\
           }\n\
           post(head.value, \"a\");\n\
         }\n",
    );
    assert_eq!(
        found,
        [
            "`node.value` is read inside a loop in `send`, which calls `check()`",
            "`node.next` is read inside a loop in `send`, which calls `check()`",
        ]
    );
}

#[test]
fn a_branch_that_leaves_before_a_check_decides_whether_it_runs() {
    let source = format!(
        "{IMPORT}export interface Order {{ kind: string }}\n\
         function doA(): void {{}}\n\
         function doB(): void {{}}\n\
         export function send(input: Order): void {{\n\
           if (input.kind === \"a\") return;\n\
           check(\"acme.com/b\", {{}});\n\
           if (input.kind === \"a\") doA(); else doB();\n\
         }}\n"
    );
    let found = findings(&[("lib", &source)]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].message,
        "`input.kind` is read more than once in `send`, which calls `check()`"
    );
    let (span, note) = &found[0].notes[2];
    assert_eq!(note, "`input.kind` decides whether `check()` runs here");
    let start = source.find("input.kind === \"a\"").unwrap();
    let end = start + "input.kind === \"a\"".len();
    assert_eq!((span.start as usize, span.end as usize), (start, end));
}

#[test]
fn an_exit_after_the_check_on_an_unchecked_value_decides_nothing() {
    assert_clean(
        "export function send(channelId: string, payload: Input): void {\n\
           check(\"x/send\", { channelId });\n\
           if (payload.text === \"\") throw new Error(\"empty text\");\n\
           post(channelId, payload.text);\n\
         }\n",
    );
}

#[test]
fn a_guard_that_leaves_before_the_check_makes_its_value_subject() {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}function keep(input: Input | null): void {{}}\n\
             export function send(channelId: string, input: Input | null): void {{\n\
               if (input !== null && input.text === \"\") throw new Error(\"empty text\");\n\
               check(\"x/send\", {{ channelId }});\n\
               keep(input);\n\
             }}\n"
        ),
    )]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].message,
        "caller-supplied `input` is passed to `keep` in `send`, which calls `check()`"
    );
    assert_eq!(
        found[0].notes[1].1,
        "`input.text` decides whether `check()` runs here"
    );
}

#[test]
fn comparing_a_caller_value_itself_reaches_nothing() {
    let source = |body: &str| {
        format!(
            "export interface Tree {{ parent: Tree | null; name: string }}\n\
             function keep(tree: Tree | null): void {{}}\n\
             export function send(input: Tree | null, tree: Tree): void {{\n\
               {body}\n\
             }}\n"
        )
    };
    for body in [
        "if (input === null) throw new Error(\"no input\");\n\
         check(\"acme.com/op\", { id: 1 });\n\
         keep(input);",
        "check(\"c\", { present: input !== null });\n\
         keep(input);",
        "if (!input) return;\n\
         check(\"acme.com/op\", { id: typeof tree === \"object\" ? 1 : 2 });\n\
         keep(input);\n\
         keep(tree);",
    ] {
        assert_clean(&source(body));
    }
    let found = messages(&source(
        "if (tree.parent === null) return;\n\
         check(\"acme.com/op\", { id: 1 });\n\
         keep(tree);",
    ));
    assert_eq!(
        found,
        ["caller-supplied `tree` is passed to `keep` in `send`, which calls `check()`"]
    );
}

/// A package function shaped like firecrawl's `search`, with `body` after
/// the scrape options are read once and the query is checked.
fn search(context: &str, body: &str) -> String {
    format!(
        "export interface ScrapeOptions {{ formats: string[]; schema: unknown }}\n\
         export interface SearchOptions {{ query: string; scrapeOptions: ScrapeOptions | null }}\n\
         function scrape(options: ScrapeOptions): void {{}}\n\
         function forward(options: SearchOptions): void {{}}\n\
         export function search(options: SearchOptions): void {{\n\
           const scrapeOptions = options.scrapeOptions;\n\
           check(\"firecrawl.dev/search\", {context});\n\
           if (scrapeOptions !== null) {{\n\
             check(\"firecrawl.dev/search.scrape\", {{}});\n\
             scrape(scrapeOptions);\n\
           }}\n\
           {body}\n\
         }}\n"
    )
}

#[test]
fn a_value_checked_by_its_identity_may_be_handed_on_once_read() {
    assert_clean(&search(
        "{ query: options.query }",
        "const none: string[] = [];\n\
         const body = { scrapeOptions, formats: scrapeOptions?.formats ?? none };\n\
         const schema = JSON.stringify(scrapeOptions?.schema ?? null);\n\
         post(schema, body.formats.join(\",\"));",
    ));
}

#[test]
fn a_second_read_of_a_value_checked_by_its_identity_is_reported() {
    let found = findings(&[(
        "lib",
        &format!(
            "{IMPORT}{}",
            search(
                "{ query: options.query }",
                "const body = { scrapeOptions: options.scrapeOptions };"
            )
        ),
    )]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].message,
        "`options.scrapeOptions` is read more than once in `search`, which calls `check()`"
    );
    assert_eq!(
        found[0].notes[2].1,
        "`options.scrapeOptions` decides whether `check()` runs here"
    );
}

#[test]
fn handing_on_what_holds_a_value_checked_by_its_identity_is_reported() {
    let found = findings(&[(
        "lib",
        &format!("{IMPORT}{}", search("{}", "forward(options);")),
    )]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].message,
        "caller-supplied `options` is passed to `forward` in `search`, which calls `check()`"
    );
    assert_eq!(
        found[0].notes[1].1,
        "`options.scrapeOptions` decides whether `check()` runs here"
    );
}

#[test]
fn a_flag_set_under_a_condition_carries_the_condition() {
    let read_twice =
        "`options.scrapeOptions` is read more than once in `send`, which calls `check()`";
    for decide in [
        "let scrapes = false;\n\
         if (options.scrapeOptions !== null) scrapes = true;",
        "const scrapes = options.scrapeOptions !== null;",
    ] {
        let found = messages(&format!(
            "export interface SearchOptions {{ scrapeOptions: Options | null }}\n\
             function forward(options: Options | null): void {{}}\n\
             export function send(options: SearchOptions): void {{\n\
               {decide}\n\
               if (scrapes) check(\"c\", {{}});\n\
               forward(options.scrapeOptions);\n\
             }}\n"
        ));
        assert_eq!(found, [read_twice], "{decide}");
    }
    assert_clean(
        "export function send(payload: Input): void {\n\
           let long = false;\n\
           if (payload.text.length > 3) long = true;\n\
           post(payload.text, long ? \"long\" : \"short\");\n\
           check(\"c\", { id: 1 });\n\
         }\n",
    );
}

/// The declarations the reproducers of the review share.
const PROBE: &str = "import { check } from \"submilli:security\";\n\
     export interface In {\n\
       id: string; other: string; kind: string; ok: boolean; onlyFirst: boolean;\n\
       items: Item[]; channelIds: string[]; mentions: string[] | null; meta: unknown;\n\
       tags: string[] | null; labels: Map<string, string> | null; settings: Ctx;\n\
       lists: string[][]; byId: Map<string, Ctx>;\n\
     }\n\
     export interface Item { id: string; open: boolean }\n\
     export interface Ctx { id: string }\n\
     export interface Outer { inner: Ctx }\n\
     function send(x: string): void {}\n\
     function sendMentions(x: string[] | null): void {}\n\
     function keep(x: unknown): void {}\n\
     function fill(c: Ctx, id: string): void { c.id = id; }\n\
     function collect(out: string[], items: Item[]): void { for (const it of items) out.push(it.id); }\n\
     const SCRATCH: Ctx = { id: \"\" };\n\
     const CACHE = new Map<string, string>();\n\
     let lastId = \"\";\n\
     function reg(): Map<string, string> { return CACHE; }\n\
     const LOG: string[] = [];\n\
     const CHANNEL_ID = /^C[0-9A-Z]+$/;\n\
     const STATES = new Set<string>([\"open\", \"closed\"]);\n\
     const ALIASES = new Map<string, string>();\n\
     const SPACES = / /g;\n\
     function box(): string[] { return LOG; }\n\
     function byName(a: string, b: string): number { return a < b ? -1 : 1; }\n\
     function toBody(kind: string, id: string): Ctx { return { id: kind + id }; }\n\
     function fetchRaw(id: string): string { return id; }\n\
     function withIds(ids: string[]): Ctx { return { id: ids.join(\",\") }; }\n\
     const OTHER: string[] = [];\n\
     export class Registry { static ids: string[] = []; }\n";

/// The findings on a function `run(input: In)` with `body`, after [`PROBE`].
fn probe(body: &str) -> Vec<String> {
    messages_of(&[(
        "lib",
        &format!("{PROBE}export function run(input: In): void {{\n{body}\n}}\n"),
    )])
}

const ID_TWICE: &str = "`input.id` is read more than once in `run`, which calls `check()`";

/// Asserts that a finding of `body` reports a read of `value` again, twice
/// or in a loop: the flow the reproducer hides is followed.
fn assert_read_again(body: &str, value: &str) {
    let found = probe(body);
    let again = [
        format!("`{value}` is read more than once in `run`, which calls `check()`"),
        format!("`{value}` is read inside a loop in `run`, which calls `check()`"),
    ];
    assert!(
        found.iter().any(|message| again.contains(message)),
        "{body}\n{found:#?}"
    );
}

#[test]
fn a_value_written_through_an_alias_is_examined() {
    for body in [
        "const a: Ctx = { id: \"\" };\n\
         const b = a;\n\
         b.id = input.id;\n\
         check(\"acme.com/probe\", { id: a.id });\n\
         send(input.id);",
        "const outer: Outer = { inner: { id: \"\" } };\n\
         const inner = outer.inner;\n\
         inner.id = input.id;\n\
         check(\"acme.com/probe\", { id: outer.inner.id });\n\
         send(input.id);",
        "const all: string[][] = [[]];\n\
         const first = all[0];\n\
         first.push(input.id);\n\
         check(\"acme.com/probe\", { id: all[0][0] });\n\
         send(input.id);",
    ] {
        assert_eq!(probe(body), [ID_TWICE], "{body}");
    }
}

#[test]
fn a_call_that_may_store_into_an_argument_is_followed_or_examined() {
    assert_eq!(
        probe(
            "const c: Ctx = { id: \"\" };\n\
             fill(c, input.id);\n\
             check(\"acme.com/probe\", { id: c.id });\n\
             send(input.id);"
        ),
        [ID_TWICE]
    );
    assert_read_again(
        "const ids: string[] = [];\n\
         collect(ids, input.items);\n\
         check(\"acme.com/probe\", { id: ids.join(\",\") });\n\
         for (const it of input.items) send(it.id);",
        "input.items",
    );
}

#[test]
fn a_write_into_this_or_a_global_examines_the_whole_body() {
    for body in [
        "SCRATCH.id = input.id;\n\
         check(\"acme.com/probe\", { id: SCRATCH.id });\n\
         send(input.id);",
        "CACHE.set(\"k\", input.id);\n\
         check(\"acme.com/probe\", { id: CACHE.get(\"k\") ?? \"\" });\n\
         send(input.id);",
        "lastId = input.id;\n\
         check(\"acme.com/probe\", { id: lastId });\n\
         send(input.id);",
    ] {
        assert_eq!(probe(body), [ID_TWICE], "{body}");
    }
    let found = messages_of(&[(
        "lib",
        &format!(
            "{PROBE}export class Holder {{\n\
               id: string = \"\";\n\
               run(input: In): void {{\n\
                 this.id = input.id;\n\
                 check(\"acme.com/probe\", {{ id: this.id }});\n\
                 send(input.id);\n\
               }}\n\
             }}\n"
        ),
    )]);
    assert_eq!(
        found,
        ["`input.id` is read more than once in `Holder.run`, which calls `check()`"]
    );
}

#[test]
fn a_callback_that_feeds_check_examines_the_whole_body() {
    for (body, value) in [
        (
            "const ids: string[] = [];\n\
             input.items.forEach((it: Item) => { ids.push(it.id); });\n\
             check(\"acme.com/probe\", { id: ids.join(\",\") });\n\
             for (const it of input.items) send(it.id);",
            "input.items",
        ),
        (
            "const ids: string[] = [];\n\
             input.items.map((it: Item): number => ids.push(it.id));\n\
             check(\"acme.com/probe\", { id: ids[0] });\n\
             for (const it of input.items) send(it.id);",
            "input.items",
        ),
        (
            "const ids: string[] = [];\n\
             const add = (s: string): void => { ids.push(s); };\n\
             add(input.id);\n\
             check(\"acme.com/probe\", { id: ids[0] });\n\
             send(input.id);",
            "input.id",
        ),
    ] {
        assert_read_again(body, value);
    }
}

#[test]
fn a_long_chain_of_conditionals_settles_quickly_and_reports() {
    let mut body = String::new();
    for step in 1..=40 {
        let previous = step - 1;
        body.push_str(&format!(
            "  const a{step} = c ? a{previous}.l : a{previous}.r;\n"
        ));
    }
    let source = format!(
        "import {{ check }} from \"submilli:security\";\n\
         export interface T {{ l: T; r: T; id: string }}\n\
         function keep(t: T): void {{}}\n\
         export function f(a0: T, c: boolean): void {{\n\
         {body}\
           check(\"acme.com/probe\", {{ id: a40.id }});\n\
           keep(a0.l);\n\
         }}\n"
    );
    let started = std::time::Instant::now();
    let found = messages_of(&[("lib", &source)]);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    assert!(
        found.contains(&"`a0.l` is read more than once in `f`, which calls `check()`".to_string()),
        "{found:#?}"
    );
}

#[test]
fn a_loop_exit_decides_whether_a_check_runs() {
    for (body, value) in [
        (
            "for (let i = 0; i < 1; i++) {\n\
               if (input.ok) break;\n\
               check(\"acme.com/probe\", { id: \"x\" });\n\
             }\n\
             if (input.ok) send(\"privileged\");",
            "input.ok",
        ),
        (
            "for (let i = 0; i < 1; i++) {\n\
               if (input.ok) continue;\n\
               check(\"acme.com/probe\", { id: \"x\" });\n\
             }\n\
             if (input.ok) send(\"privileged\");",
            "input.ok",
        ),
        (
            "while (!input.ok) {\n\
               if (input.ok) send(\"privileged\");\n\
               return;\n\
             }\n\
             check(\"acme.com/probe\", { id: \"x\" });",
            "input.ok",
        ),
        (
            "const targets: string[] = [];\n\
             for (const id of input.channelIds) targets.push(id);\n\
             for (const id of targets) {\n\
               check(\"acme.com/messages.post\", { channelId: id });\n\
               if (input.onlyFirst) break;\n\
             }\n\
             for (const id of targets) { send(id); if (input.onlyFirst) break; }",
            "input.onlyFirst",
        ),
        (
            "const targets: string[] = [];\n\
             for (const id of input.channelIds) targets.push(id);\n\
             for (const id of targets) {\n\
               if (input.onlyFirst) continue;\n\
               check(\"acme.com/messages.post\", { channelId: id });\n\
             }\n\
             if (!input.onlyFirst) { for (const id of targets) send(id); }",
            "input.onlyFirst",
        ),
        (
            "let notify = false;\n\
             for (const x of [1]) { if (input.mentions === null) break; notify = true; }\n\
             if (notify) check(\"acme.com/mentions.notify\", {});\n\
             sendMentions(input.mentions);",
            "input.mentions",
        ),
    ] {
        assert_read_again(body, value);
    }
}

#[test]
fn an_array_length_and_its_elements_are_one_value() {
    for body in [
        "check(\"acme.com/probe\", { count: input.channelIds.length });\n\
         for (const id of input.channelIds) send(id);",
        "for (const id of input.channelIds) check(\"acme.com/probe\", { id });\n\
         send(input.channelIds.length > 0 ? \"a\" : \"b\");",
    ] {
        assert_eq!(probe(body).len(), 1, "{body}");
    }
    assert!(
        probe(
            "check(\"acme.com/probe\", { id: input.id });\n\
             send(input.channelIds.length > 0 ? \"a\" : \"b\");\n\
             for (const id of input.channelIds) send(id);"
        )
        .is_empty()
    );
}

#[test]
fn a_decision_survives_try_catch_collections_blocks_and_switch() {
    for (body, value) in [
        (
            "try { if (input.mentions !== null) throw new Error(\"m\"); }\n\
             catch (e) { check(\"acme.com/mentions.notify\", {}); }\n\
             sendMentions(input.mentions);",
            "input.mentions",
        ),
        (
            "let notify = false;\n\
             try { if (input.mentions === null) throw new Error(\"m\"); notify = true; } catch (e) {}\n\
             if (notify) check(\"acme.com/mentions.notify\", {});\n\
             sendMentions(input.mentions);",
            "input.mentions",
        ),
        (
            "let notify = true;\n\
             try { if (input.mentions === null) throw new Error(\"none\"); } catch (e) { notify = false; }\n\
             if (notify) check(\"acme.com/mentions.notify\", {});\n\
             sendMentions(input.mentions);",
            "input.mentions",
        ),
        (
            "const flags = new Map<string, boolean>();\n\
             flags.set(\"n\", input.mentions !== null);\n\
             if (flags.get(\"n\") === true) check(\"acme.com/mentions.notify\", {});\n\
             sendMentions(input.mentions);",
            "input.mentions",
        ),
        (
            "const flags: boolean[] = [];\n\
             flags.push(input.mentions !== null);\n\
             if (flags[0]) check(\"acme.com/mentions.notify\", {});\n\
             sendMentions(input.mentions);",
            "input.mentions",
        ),
        (
            "const state = { notify: false };\n\
             if (input.mentions !== null) state.notify = true;\n\
             if (state.notify) check(\"acme.com/mentions.notify\", {});\n\
             sendMentions(input.mentions);",
            "input.mentions",
        ),
        (
            "const on: boolean[] = [];\n\
             if (input.mentions !== null) { const yes = true; on.push(yes); }\n\
             if (on.length > 0) check(\"acme.com/mentions.notify\", {});\n\
             sendMentions(input.mentions);",
            "input.mentions",
        ),
        (
            "if (input.id.length > 0) {\n\
               if (input.mentions === null) return;\n\
             }\n\
             check(\"acme.com/mentions.notify\", {});\n\
             sendMentions(input.mentions);",
            "input.mentions",
        ),
        (
            "for (const c of [\"a\"]) {\n\
               if (input.mentions === null) return;\n\
             }\n\
             check(\"acme.com/mentions.notify\", {});\n\
             sendMentions(input.mentions);",
            "input.mentions",
        ),
        (
            "let notify = false;\n\
             switch (input.kind) { case \"mention\": notify = true; break; default: break; }\n\
             if (notify) check(\"acme.com/mentions.notify\", {});\n\
             if (input.kind === \"mention\") sendMentions(null);",
            "input.kind",
        ),
    ] {
        assert_read_again(body, value);
    }
}

#[test]
fn typeof_and_instanceof_of_a_property_rely_on_its_identity() {
    let meta_twice = "`input.meta` is read more than once in `run`, which calls `check()`";
    for test in [
        "typeof input.meta === \"object\"",
        "input.meta instanceof Error",
    ] {
        assert_eq!(
            probe(&format!(
                "if ({test}) return;\n\
                 check(\"acme.com/probe\", {{}});\n\
                 keep(input.meta);"
            )),
            [meta_twice],
            "{test}"
        );
        let once = test.replace("input.meta", "meta");
        assert!(
            probe(&format!(
                "const meta = input.meta;\n\
                 if ({once}) return;\n\
                 check(\"acme.com/probe\", {{}});\n\
                 keep(meta);"
            ))
            .is_empty(),
            "{test}"
        );
    }
}

/// Why every caller-supplied value of `run` is examined when its body is
/// `body`: the note on a repeated read of `input.other`, which never reaches
/// `check()`. `None` when the body is simple and that read is free.
fn examined_because(body: &str) -> Option<String> {
    let found = findings(&[(
        "lib",
        &format!(
            "{PROBE}export function run(input: In): void {{\n{body}\n\
             send(input.other);\n\
             send(input.other);\n\
             }}\n"
        ),
    )]);
    let read = found.iter().find(|diag| {
        diag.message == "`input.other` is read more than once in `run`, which calls `check()`"
    })?;
    Some(read.notes[2].1.clone())
}

#[test]
fn a_body_is_simple_while_nothing_that_reaches_check_can_change() {
    for body in [
        "check(\"x/run\", { id: input.id });",
        "const c: Ctx = { id: \"\" };\n\
         c.id = input.id;\n\
         check(\"x/run\", { id: c.id });",
        "const c: Ctx = { id: \"\" };\n\
         fill(c, input.id);\n\
         check(\"x/run\", { id: c.id });",
        "const ids: string[] = [];\n\
         for (const it of input.items) ids.push(it.id);\n\
         check(\"x/run\", { id: ids.join(\",\") });",
        "const ids: string[] = [];\n\
         collect(ids, []);\n\
         check(\"x/run\", { id: ids.join(\",\") });",
        "const ids: string[] = [];\n\
         for (const it of input.items) ids.push(it.id);\n\
         keep({ ids, id: input.id, count: input.items.length });\n\
         check(\"x/run\", { id: ids.join(\",\") });",
    ] {
        assert_eq!(examined_because(body), None, "{body}");
    }
    assert_eq!(
        examined_because(
            "const ids: string[] = [];\n\
             keep({ ids, items: input.items });\n\
             check(\"x/run\", { id: ids.join(\",\") });"
        ),
        Some(
            "`check()` depends on `ids`, which is passed to `keep` with `input.items`, \
             so every caller-supplied value in `run` is examined"
                .to_string()
        )
    );
}

#[test]
fn each_reason_to_examine_a_whole_body_is_named() {
    let examined = ", so every caller-supplied value in `run` is examined";
    for (body, reason) in [
        (
            "check(\"x/run\", { id: lastId });",
            "`check()` depends on `lastId`, which code outside `run` can change",
        ),
        (
            "const a: Ctx = { id: \"\" };\n\
             const b = a;\n\
             b.id = input.id;\n\
             check(\"x/run\", { id: a.id });",
            "`check()` depends on `a`, which `b` is made from",
        ),
        (
            "const ids: string[] = [];\n\
             input.items.forEach((it: Item): void => { ids.push(it.id); });\n\
             check(\"x/run\", { id: ids.join(\",\") });",
            "`check()` depends on `ids`, which a nested function captures",
        ),
        (
            "const ids: string[] = [];\n\
             collect(ids, input.items);\n\
             check(\"x/run\", { id: ids.join(\",\") });",
            "`check()` depends on `ids`, which is passed to `collect` with `input.items`",
        ),
        (
            "let id = \"\";\n\
             const take = (): void => { id = input.id; };\n\
             take();\n\
             check(\"x/run\", { id });",
            "`check()` depends on `id`, which a nested function assigns",
        ),
        (
            "const pick = (s: string): string => s;\n\
             check(\"x/run\", { id: pick(input.id) });",
            "`check()` depends on a call of `pick`, whose effects are not followed",
        ),
        (
            "input.items[0].id = input.id;\n\
             check(\"x/run\", { id: input.id });",
            "`run` changes a value the body does not own before `check()`",
        ),
        (
            "reg().set(\"k\", input.id);\n\
             check(\"x/run\", { id: input.id });",
            "`run` changes the receiver of `set` before `check()`",
        ),
        (
            "lastId = input.id;\n\
             check(\"x/run\", { id: input.id });",
            "`run` changes the global `lastId` before `check()`",
        ),
        (
            "check(\"x/run\", { id: input.id });\n\
             if (input.ok) run(input);",
            "`run` calls itself",
        ),
    ] {
        assert_eq!(
            examined_because(body),
            Some(format!("{reason}{examined}")),
            "{body}"
        );
    }
}

#[test]
fn this_that_reaches_check_examines_the_whole_body() {
    let found = findings(&[(
        "lib",
        &format!(
            "{PROBE}export class Holder {{\n\
               id: string = \"\";\n\
               run(input: In): void {{\n\
                 check(\"x/run\", {{ id: this.id }});\n\
                 send(input.other);\n\
                 send(input.other);\n\
               }}\n\
             }}\n"
        ),
    )]);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].notes[2].1,
        "`check()` depends on `this.id`, which code outside `Holder.run` can change, \
         so every caller-supplied value in `Holder.run` is examined"
    );
}

#[test]
fn what_runs_after_every_check_is_left_out() {
    assert_eq!(
        examined_because(
            "check(\"x/run\", { id: input.id });\n\
             SCRATCH.id = input.id;\n\
             const ids: string[] = [];\n\
             collect(ids, input.items);"
        ),
        None
    );
    for body in [
        "for (const it of input.items) {\n\
           check(\"x/run\", { id: it.id });\n\
           SCRATCH.id = input.id;\n\
         }",
        "check(\"x/run\", { id: input.id });\n\
         const later = (): void => { SCRATCH.id = input.id; };",
        "const approve = (): void => { check(\"x/run\", {}); };\n\
         approve();\n\
         SCRATCH.id = input.id;",
    ] {
        assert!(examined_because(body).is_some(), "{body}");
    }
}

#[test]
fn a_body_too_large_to_follow_is_examined_whole_and_quickly() {
    let mut body = String::new();
    for step in 1..=60 {
        let previous = step - 1;
        body.push_str(&format!(
            "  const a{step} = c ? a{previous}.l : a{previous}.r;\n"
        ));
    }
    let source = format!(
        "import {{ check }} from \"submilli:security\";\n\
         export interface T {{ l: T; r: T; id: string }}\n\
         function send(x: string): void {{}}\n\
         export function f(a0: T, c: boolean, other: T): void {{\n\
         {body}\
           check(\"acme.com/probe\", {{ id: a60.id }});\n\
           send(other.id);\n\
           send(other.id);\n\
         }}\n"
    );
    let started = std::time::Instant::now();
    let found = findings(&[("lib", &source)]);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    let read = found
        .iter()
        .find(|diag| {
            diag.message == "`other.id` is read more than once in `f`, which calls `check()`"
        })
        .unwrap_or_else(|| panic!("{found:#?}"));
    assert_eq!(
        read.notes[2].1,
        "`f` is too large to follow what reaches `check()`, \
         so every caller-supplied value in `f` is examined"
    );
}

#[test]
fn a_set_size_counts_the_elements_an_iteration_reads() {
    let found = messages(
        "export function send(tags: Set<string>, names: Set<string>): void {\n\
           check(\"x/send\", { count: tags.size });\n\
           for (const tag of tags) post(tag, \"a\");\n\
           for (const name of names) post(name, \"b\");\n\
           post(\"c\", names.size > 0 ? \"d\" : \"e\");\n\
         }\n",
    );
    assert_eq!(
        found,
        ["elements of `tags` are read more than once in `send`, which calls `check()`"]
    );
}

#[test]
fn a_break_out_of_a_switch_leaves_nothing() {
    assert_clean(
        "export interface Kind { channelId: string; kind: string }\n\
         export function send(input: Kind): void {\n\
           const channelId = input.channelId;\n\
           let label = \"\";\n\
           switch (input.kind) { case \"a\": label = \"A\"; break; default: label = \"B\"; break; }\n\
           check(\"x/send\", { channelId });\n\
           post(channelId, label + input.kind);\n\
         }\n",
    );
}

#[test]
fn only_a_call_of_the_body_itself_is_recursion() {
    let found = messages_of(&[
        (
            "lib",
            "import { check } from \"submilli:security\";\n\
             import * as pages from \"./pages\";\n\
             export interface In { id: string; other: string }\n\
             function send(x: string): void {}\n\
             export function create(input: In): void {\n\
               const id = input.id;\n\
               check(\"x/create\", { id });\n\
               pages.create(id);\n\
               send(input.other);\n\
               send(input.other);\n\
             }\n\
             export class Pages {\n\
               create(input: In, depth: number): void {\n\
                 check(\"x/pages\", { id: input.id });\n\
                 if (depth > 0) this.create(input, depth - 1);\n\
                 send(input.other);\n\
                 send(input.other);\n\
               }\n\
             }\n",
        ),
        ("pages", "export function create(id: string): void {}\n"),
    ]);
    let other_twice = |label: &str| {
        format!("`input.other` is read more than once in `{label}`, which calls `check()`")
    };
    assert!(!found.contains(&other_twice("create")), "{found:#?}");
    assert!(found.contains(&other_twice("Pages.create")), "{found:#?}");
}

#[test]
fn a_container_moved_to_another_local_is_still_the_bodys_own() {
    let examined = ", so every caller-supplied value in `run` is examined";
    for body in [
        "let cc: string[] | null = null;\n\
         if (input.mentions !== null) {\n\
           const copied: string[] = [];\n\
           for (const entry of input.mentions) copied.push(entry);\n\
           cc = copied;\n\
         }\n\
         check(\"x/run\", { count: cc === null ? 0 : cc.length });",
        "let last: string[] = [];\n\
         for (const it of input.items) {\n\
           const one: string[] = [];\n\
           one.push(it.id);\n\
           last = one;\n\
         }\n\
         check(\"x/run\", { id: last.join(\",\") });",
    ] {
        assert_eq!(examined_because(body), None, "{body}");
    }
    for (body, reason) in [
        (
            "let cc: string[] | null = null;\n\
             if (input.mentions !== null) {\n\
               const copied: string[] = [];\n\
               for (const entry of input.mentions) copied.push(entry);\n\
               cc = copied;\n\
               copied.push(input.id);\n\
             }\n\
             check(\"x/run\", { count: cc === null ? 0 : cc.length });",
            "`check()` depends on `copied`, which `cc` is made from",
        ),
        (
            "const a: string[] = [];\n\
             const b = a;\n\
             a.push(input.id);\n\
             check(\"x/run\", { id: b.join(\",\") });",
            "`check()` depends on `a`, which `b` is made from",
        ),
        (
            "const all: string[] = [];\n\
             let last: string[] = [];\n\
             for (const it of input.items) {\n\
               all.push(it.id);\n\
               last = all;\n\
             }\n\
             check(\"x/run\", { id: last.join(\",\") });",
            "`check()` depends on `all`, which `last` is made from",
        ),
    ] {
        assert_eq!(
            examined_because(body),
            Some(format!("{reason}{examined}")),
            "{body}"
        );
    }
}

#[test]
fn reading_constants_and_fresh_values_keeps_a_body_simple() {
    for body in [
        "if (![\"open\", \"closed\"].includes(input.kind)) throw new Error(\"state\");\n\
         check(\"x/run\", { id: input.id });",
        "if (!new Set<string>([\"open\", \"closed\"]).has(input.kind)) throw new Error(\"state\");\n\
         check(\"x/run\", { id: input.id });",
        "if (!\"open,closed\".split(\",\").includes(input.kind)) throw new Error(\"state\");\n\
         check(\"x/run\", { id: input.id });",
        "if ([\"open\", \"closed\"].indexOf(input.kind) < 0) throw new Error(\"state\");\n\
         check(\"x/run\", { id: input.id });",
        "const id = input.id;\n\
         if (!CHANNEL_ID.test(id)) throw new Error(\"channel\");\n\
         check(\"x/run\", { id });",
        "const kind = input.kind;\n\
         if (!STATES.has(kind)) throw new Error(\"state\");\n\
         check(\"x/run\", { id: kind });",
        "const id = input.id;\n\
         check(\"x/run\", { id: ALIASES.get(id) ?? id });",
        "const id = input.id;\n\
         check(\"x/run\", { id: id.replace(SPACES, \"\") });",
        "const ids: string[] = [];\n\
         for (const it of input.items) ids.push(it.id);\n\
         collect(ids, SCRATCH.id === \"\" ? [] : []);\n\
         keep({ ids, table: STATES });\n\
         check(\"x/run\", { id: ids.join(\",\") });",
    ] {
        assert_eq!(examined_because(body), None, "{body}");
    }
}

#[test]
fn changing_a_constant_or_this_still_examines_the_whole_body() {
    let examined = ", so every caller-supplied value in `run` is examined";
    for (body, reason) in [
        (
            "LOG.push(input.id);\n\
             check(\"x/run\", { id: input.id });",
            "`run` changes the receiver of `push` before `check()`",
        ),
        (
            "SCRATCH.id = input.id;\n\
             check(\"x/run\", { id: SCRATCH.id });",
            "`run` changes a value the body does not own before `check()`",
        ),
        (
            "input.items.push(input.items[0]);\n\
             check(\"x/run\", { id: input.id });",
            "`run` changes the receiver of `push` before `check()`",
        ),
        (
            "const id = input.id;\n\
             const ids = input.channelIds;\n\
             ids.push(id);\n\
             check(\"x/run\", { id });",
            "`run` changes the receiver of `push` before `check()`",
        ),
    ] {
        assert_eq!(
            examined_because(body),
            Some(format!("{reason}{examined}")),
            "{body}"
        );
    }
    assert_eq!(
        examined_because(
            "input.items.reverse();\n\
             check(\"x/run\", { id: input.id });"
        ),
        None
    );
    let found = findings(&[(
        "lib",
        &format!(
            "{PROBE}export class Holder {{\n\
               id: string = \"\";\n\
               run(input: In): void {{\n\
                 this.id = input.id;\n\
                 check(\"x/run\", {{ id: input.id }});\n\
                 send(input.other);\n\
                 send(input.other);\n\
               }}\n\
             }}\n"
        ),
    )]);
    let read = found
        .iter()
        .find(|diag| {
            diag.message
                .starts_with("`input.other` is read more than once")
        })
        .unwrap_or_else(|| panic!("{found:#?}"));
    assert_eq!(
        read.notes[2].1,
        "`Holder.run` changes a value the body does not own before `check()`, \
         so every caller-supplied value in `Holder.run` is examined"
    );
}

#[test]
fn an_optional_call_of_a_function_value_is_a_call_of_it() {
    let found = messages_of(&[(
        "lib",
        &format!(
            "{PROBE}export function run(input: In, cb: ((s: string) => string) | null): void {{\n\
               check(\"x/run\", {{ id: cb?.(input.id) ?? \"\" }});\n\
               send(input.other);\n\
               send(input.other);\n\
             }}\n\
             export function direct(input: In, cb: (s: string) => string): void {{\n\
               check(\"x/run\", {{ id: cb(input.id) }});\n\
               send(input.other);\n\
               send(input.other);\n\
             }}\n"
        ),
    )]);
    let other_twice = |label: &str| {
        format!("`input.other` is read more than once in `{label}`, which calls `check()`")
    };
    assert!(found.contains(&other_twice("run")), "{found:#?}");
    assert!(found.contains(&other_twice("direct")), "{found:#?}");
}

#[test]
fn a_wide_guard_before_many_locals_stays_within_the_budget() {
    let reads: Vec<String> = (0..5000).map(|i| format!("ids[{i}]")).collect();
    let mut body = format!("  if ([{}].length === 0) return;\n", reads.join(", "));
    for local in 0..4000 {
        body.push_str(&format!("  const w{local}: string[] = [];\n"));
    }
    let source = format!(
        "import {{ check }} from \"submilli:security\";\n\
         function send(x: string): void {{}}\n\
         export function run(ids: string[], other: string[]): void {{\n\
         {body}\
           check(\"acme.com/probe\", {{ id: \"c\" }});\n\
           send(other[0]);\n\
           send(other[0]);\n\
         }}\n"
    );
    let started = std::time::Instant::now();
    let found = findings(&[("lib", &source)]);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
    let read = found
        .iter()
        .find(|diag| diag.message.starts_with("elements of `other`"))
        .unwrap_or_else(|| panic!("{found:#?}"));
    assert_eq!(
        read.notes[1].1,
        "`run` is too large to follow what reaches `check()`, \
         so every caller-supplied value in `run` is examined"
    );
}

#[test]
fn methods_that_cannot_change_a_value_keep_a_body_simple() {
    for body in [
        "if (input.tags?.includes(\"urgent\")) throw new Error(\"urgent\");\n\
         check(\"x/run\", { id: input.id });",
        "const team = input.labels?.get(\"team\") ?? \"\";\n\
         check(\"x/run\", { id: team });",
        "const ids = input.channelIds.slice().sort(byName);\n\
         check(\"x/run\", { id: ids.join(\",\") });",
        "const ids = input.channelIds.map((c: string): string => c.trim()).sort(byName);\n\
         check(\"x/run\", { id: ids.join(\",\") });",
        "const ids = [...input.channelIds].sort(byName);\n\
         check(\"x/run\", { id: ids.join(\",\") });",
        "const keys = Object.keys(input.settings).sort(byName);\n\
         check(\"x/run\", { id: keys.join(\",\") });",
        "const bytes = new TextEncoder().encode(input.id);\n\
         check(\"x/run\", { id: String(bytes.subarray(0, 1).length) });",
    ] {
        assert_eq!(examined_because(body), None, "{body}");
    }
    // A method called on a parameter that decides the check is still an
    // escape, but the body stays simple: `input.other` is free.
    let found = findings(&[(
        "lib",
        &format!(
            "{PROBE}export function run(tags: string[] | null, input: In): void {{\n\
               if (tags?.includes(\"urgent\")) throw new Error(\"urgent\");\n\
               check(\"x/run\", {{ id: input.id }});\n\
               send(input.other);\n\
               send(input.other);\n\
             }}\n"
        ),
    )]);
    let examined = found
        .iter()
        .flat_map(|diag| &diag.notes)
        .any(|(_, note)| note.ends_with("is examined"));
    assert!(!examined, "{found:#?}");
    assert!(
        !found
            .iter()
            .any(|diag| diag.message.starts_with("`input.other`")),
        "{found:#?}"
    );
}

#[test]
fn a_change_through_a_value_the_body_does_not_own_examines_the_whole_body() {
    let examined = ", so every caller-supplied value in `run` is examined";
    for (body, reason) in [
        (
            "input.tags?.push(input.id);\n\
             check(\"x/run\", { id: input.id });",
            "`run` changes the receiver of `push` before `check()`",
        ),
        (
            "box().push(input.id);\n\
             check(\"x/run\", { id: input.id });",
            "`run` changes the receiver of `push` before `check()`",
        ),
        (
            "const holder = { l: OTHER };\n\
             holder.l.push(input.id);\n\
             check(\"x/run\", { id: OTHER[0] });",
            "`run` changes the receiver of `push` before `check()`",
        ),
    ] {
        assert_eq!(
            examined_because(body),
            Some(format!("{reason}{examined}")),
            "{body}"
        );
    }
}

#[test]
fn a_static_field_that_reaches_check_is_shared_state() {
    assert_eq!(
        examined_because("check(\"x/run\", { id: Registry.ids.join(\",\") });"),
        Some(
            "`check()` depends on `Registry.ids`, which code outside `run` can change, \
             so every caller-supplied value in `run` is examined"
                .to_string()
        )
    );
}

#[test]
fn comparing_objects_uses_their_contents_and_maps_their_identity() {
    let passed = |shown: &str| {
        format!("caller-supplied `{shown}` is passed to `keep` in `run`, which calls `check()`")
    };
    for (body, expected) in [
        (
            "const s = input.settings;\n\
             if (s !== DEFAULTS) check(\"x/run\", { id: \"c\" });\n\
             keep(s);",
            vec![passed("s")],
        ),
        (
            "const s = input.settings;\n\
             const wanted: Ctx = { id: \"general\" };\n\
             if (s !== wanted) check(\"x/run\", { id: \"c\" });\n\
             keep(s);",
            vec![passed("s")],
        ),
        (
            "if (input.settings === DEFAULTS) check(\"x/run\", { id: \"c\" });\n\
             keep(input.settings);",
            vec![
                "`input.settings` is read more than once in `run`, which calls `check()`"
                    .to_string(),
            ],
        ),
        (
            "const table = input.labels;\n\
             if (table !== ALIASES) check(\"x/run\", { id: \"c\" });\n\
             keep(table);",
            Vec::new(),
        ),
    ] {
        let found = messages_of(&[(
            "lib",
            &format!(
                "{PROBE}const DEFAULTS: Ctx = {{ id: \"\" }};\n\
                 export function run(input: In): void {{\n{body}\n}}\n"
            ),
        )]);
        assert_eq!(found, expected, "{body}");
    }
}

#[test]
fn a_local_made_from_the_callers_object_is_not_the_bodys_own() {
    let examined = ", so every caller-supplied value in `run` is examined";
    for (body, reason) in [
        (
            "const l = input.lists.find((x: string[]): boolean => x.length > 0);\n\
             if (l !== null) l.push(\"x\");\n\
             check(\"x/run\", { id: input.id });",
            "`run` changes the receiver of `push` before `check()`",
        ),
        (
            "const s = input.byId.get(\"k\");\n\
             if (s !== null) s.id = \"x\";\n\
             check(\"x/run\", { id: input.id });",
            "`run` changes a value the body does not own before `check()`",
        ),
    ] {
        assert_eq!(
            examined_because(body),
            Some(format!("{reason}{examined}")),
            "{body}"
        );
    }
    assert_eq!(
        examined_because(
            "const copy: Ctx = { id: input.settings.id };\n\
             copy.id = \"x\";\n\
             check(\"x/run\", { id: input.id });"
        ),
        None
    );
}

#[test]
fn a_value_made_only_from_primitives_is_the_bodys_own() {
    for body in [
        "const body = toBody(input.kind, input.id);\n\
         body.id = \"1\";\n\
         check(\"x/run\", { id: body.id });",
        "for (const it of input.items) {\n\
           const body = toBody(input.kind, it.id);\n\
           body.id = \"1\";\n\
           check(\"x/run\", { id: body.id });\n\
         }",
        "const raw = fetchRaw(input.id);\n\
         const data = JSON.parse(raw) as Ctx;\n\
         data.id = \"seen\";\n\
         check(\"x/run\", { id: data.id });",
    ] {
        assert_eq!(examined_because(body), None, "{body}");
    }
    assert_eq!(
        examined_because(
            "const body = withIds(input.channelIds);\n\
             body.id = \"1\";\n\
             check(\"x/run\", { id: body.id });"
        ),
        Some(
            "`run` changes a value the body does not own before `check()`, \
             so every caller-supplied value in `run` is examined"
                .to_string()
        )
    );
}
