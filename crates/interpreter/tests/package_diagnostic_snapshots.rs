//! Rendered diagnostics of the rules that run on a package.
//!
//! A package is inferred against the standard library, which a script in
//! `diagnostic_snapshots.rs` is not, so it can import `submilli:security`.

use std::collections::BTreeMap;

use interpreter::{
    Asi, FileId, ModulePath, PackageDeclaration, Sources, Token, TokenKind, diagnostics,
    infer_package, lower_patterns, parse,
};

const PRELUDE: &str = "import { check } from \"submilli:security\";

/** What a message is sent with. */
export interface Input {
  /** Conversation to post in. */
  channelId: string;
  /** Message text. */
  text: string;
}

function post(channelId: string, text: string): void {}
";

/// Every diagnostic of the single-module package `PRELUDE` and `body` make up.
fn render_package(body: &str) -> String {
    let source = format!("{PRELUDE}{body}");
    let mut sources = Sources::new();
    let file = sources.add("lib.ts".to_string(), &source).unwrap();
    let ast = lower_patterns(parse_source(&source, file)).unwrap();
    let (_, _, diags) = infer_package(
        "@test/package",
        ModulePath::from("lib"),
        vec![(ModulePath::from("lib"), file, &ast)],
        &sources,
        runtime_declarations(),
        BTreeMap::new(),
    );
    let mut out = String::new();
    for diag in &diags {
        out.push_str(&diagnostics::render(diag, &sources));
        out.push_str("---\n");
    }
    out
}

fn parse_source(source: &str, file: FileId) -> interpreter::Ast {
    let mut asi = Asi::new(source, file);
    let mut tokens: Vec<Token> = Vec::new();
    loop {
        let token = asi.next_token();
        let is_eof = matches!(token.kind, TokenKind::Eof);
        tokens.push(token);
        if is_eof {
            break;
        }
    }
    let lex_diags = asi.into_diagnostics();
    assert!(lex_diags.is_empty(), "{lex_diags:?}");
    let (ast, parse_diags) = parse(source, tokens, file);
    assert!(parse_diags.is_empty(), "{parse_diags:?}");
    ast
}

fn runtime_declarations() -> BTreeMap<String, PackageDeclaration> {
    let (prelude, host, _) = interpreter::runtime::prelude::cached_runtime_package_declarations();
    prelude
        .iter()
        .chain(host)
        .cloned()
        .chain(interpreter::stdlib::stdlib_package_declarations())
        .map(|declaration| (declaration.package_name.clone(), declaration))
        .collect()
}

#[test]
fn check_in_a_function_the_package_does_not_export() {
    insta::assert_snapshot!(render_package(
        "
/**
 * Approves the send.
 * @param channelId Conversation to post in.
 * @capability test.com/send { channelId: string }
 */
function guard(channelId: string): void {
  check(\"test.com/send\", { channelId: channelId });
}

/**
 * Sends the message.
 * @param channelId Conversation to post in.
 * @param text Message text.
 */
export function send(channelId: string, text: string): void {
  guard(channelId);
  post(channelId, text);
}
"
    ));
}

#[test]
fn property_read_again_after_the_check() {
    insta::assert_snapshot!(render_package(
        "
/**
 * Sends the message.
 * @param input Message and where to send it.
 * @capability test.com/send { channelId: string }
 */
export function send(input: Input): void {
  check(\"test.com/send\", { channelId: input.channelId });
  post(input.channelId, \"hello\");
}
"
    ));
}

#[test]
fn caller_value_passed_on_after_the_check() {
    insta::assert_snapshot!(render_package(
        "
function deliver(input: Input): void {}

/**
 * Sends the message.
 * @param input Message and where to send it.
 * @capability test.com/send { channelId: string }
 */
export function send(input: Input): void {
  const channelId = input.channelId;
  check(\"test.com/send\", { channelId: channelId });
  deliver(input);
  deliver(input);
}
"
    ));
}
