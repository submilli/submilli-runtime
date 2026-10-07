use super::*;
use crate::authority::tests::{compile_output, route};

fn compile_body(body: &str) -> crate::CompiledPackage {
    let source = format!(
        r#"
        import {{ check }} from "submilli:security";
        import {{ get }} from "submilli:http";
        export function fetch(flag: boolean): void {{ {body} }}
    "#
    );
    compile_output(&[("lib", &source)], &[])
}

fn statuses(compiled: &crate::CompiledPackage) -> Vec<AuthorityGuardStatus> {
    route(&compiled.authority_map, "#fetch")
        .effects
        .iter()
        .filter(|effect| effect.effect.capability.as_deref() == Some("http.get"))
        .map(|effect| effect.guard.status)
        .collect()
}

#[test]
fn effects_before_checks_and_unchecked_branches_are_reported() {
    let before = compile_body(r#"get("https://example.com"); check("fetch", {});"#);
    assert_eq!(statuses(&before), [AuthorityGuardStatus::Unguarded]);
    let branch = compile_body(r#"if (flag) { check("fetch", {}); } get("https://example.com");"#);
    assert_eq!(statuses(&branch), [AuthorityGuardStatus::Unguarded]);
    assert!(
        branch
            .warnings
            .iter()
            .any(|warning| warning.message.contains("successful direct semantic check"))
    );
}

#[test]
fn denial_does_not_grant_authority_but_terminated_catches_do_not_merge() {
    let swallowed =
        compile_body(r#"try { check("fetch", {}); } catch {} get("https://example.com");"#);
    assert_eq!(statuses(&swallowed), [AuthorityGuardStatus::Unguarded]);
    assert!(
        route(&swallowed.authority_map, "#fetch").effects[0]
            .guard
            .path
            .iter()
            .any(|step| step.description.contains("denied"))
    );
    for catch in ["return;", "throw e;"] {
        let compiled = compile_body(&format!(
            r#"try {{ check("fetch", {{}}); }} catch (e) {{ {catch} }} get("https://example.com");"#
        ));
        assert_eq!(
            statuses(&compiled),
            [AuthorityGuardStatus::Checked],
            "{catch}: {:#?}",
            compiled.warnings
        );
    }
}

#[test]
fn independent_branch_checks_and_aggregate_effects_are_preserved() {
    let compiled = compile_body(
        r#"
        if (flag) { check("fetch.a", {}); } else { check("fetch.b", {}); }
        get("https://example.com/a"); get("https://example.com/b");
    "#,
    );
    assert_eq!(
        statuses(&compiled),
        [AuthorityGuardStatus::Checked, AuthorityGuardStatus::Checked]
    );
    for effect in &route(&compiled.authority_map, "#fetch").effects {
        assert!(effect.guard.capabilities.is_empty());
        assert_eq!(effect.guard.checks.len(), 2);
    }
}

#[test]
fn finally_runs_on_normal_and_denied_paths_and_can_override_returns() {
    let unsafe_finally =
        compile_body(r#"try { check("fetch", {}); } finally { get("https://example.com"); }"#);
    assert!(statuses(&unsafe_finally).contains(&AuthorityGuardStatus::Unguarded));
    let safe_finally = compile_body(
        r#"try { check("fetch", {}); } catch { return; } finally { check("cleanup", {}); get("https://example.com"); }"#,
    );
    assert!(
        statuses(&safe_finally)
            .iter()
            .all(|status| *status == AuthorityGuardStatus::Checked)
    );
    let override_return = compile_body(
        r#"try { if (flag) { return; } } finally { check("fetch", {}); } get("https://example.com");"#,
    );
    assert_eq!(statuses(&override_return), [AuthorityGuardStatus::Checked]);
}

#[test]
fn helper_call_sites_keep_checked_and_unchecked_invocations_separate() {
    let compiled = compile_output(
        &[(
            "lib",
            r#"
        import { check } from "submilli:security";
        import { get } from "submilli:http";
        function helper(): void { get("https://example.com"); }
        export function fetch(flag: boolean): void {
            if (flag) { check("fetch", {}); helper(); } else { helper(); }
        }
    "#,
        )],
        &[],
    );
    let found = statuses(&compiled);
    assert_eq!(found.len(), 2);
    assert!(found.contains(&AuthorityGuardStatus::Checked));
    assert!(found.contains(&AuthorityGuardStatus::Unguarded));
    assert!(
        route(&compiled.authority_map, "#fetch")
            .effects
            .iter()
            .all(|effect| effect.witness.len() == 1)
    );
}

#[test]
fn helper_checks_do_not_authorize_the_public_route() {
    let compiled = compile_output(
        &[(
            "lib",
            r#"
        import { check } from "submilli:security";
        import { get } from "submilli:http";
        function helper(): void { check("fetch", {}); get("https://example.com"); }
        export function fetch(): void { helper(); }
    "#,
        )],
        &[],
    );
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Unguarded]);
}

#[test]
fn loops_require_current_item_or_aggregate_checks() {
    for (body, expected) in [
        (
            r#"check("fetch", { first: urls[0] }); for (const url of urls) { get(url); }"#,
            AuthorityGuardStatus::Unproven,
        ),
        (
            r#"for (const url of urls) { check("fetch", { url }); get(url); }"#,
            AuthorityGuardStatus::Checked,
        ),
        (
            r#"check("fetch", { urls }); for (const url of urls) { get(url); }"#,
            AuthorityGuardStatus::Checked,
        ),
        (
            r#"for (const url of urls) { if (flag) { check("fetch", { url }); } get(url); }"#,
            AuthorityGuardStatus::Unguarded,
        ),
    ] {
        let source = format!(
            r#"
            import {{ check }} from "submilli:security";
            import {{ get }} from "submilli:http";
            export function fetch(urls: string[], flag: boolean): void {{ {body} }}
        "#
        );
        let compiled = compile_output(&[("lib", &source)], &[]);
        assert_eq!(
            statuses(&compiled),
            [expected],
            "{body}: {:#?}",
            compiled.warnings
        );
    }
}

#[test]
fn zero_iteration_loops_do_not_establish_a_post_loop_guard() {
    let compiled =
        compile_body(r#"while (flag) { check("fetch", {}); break; } get("https://example.com");"#);
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Unguarded]);
    let compiled =
        compile_body(r#"do { check("fetch", {}); } while (flag); get("https://example.com");"#);
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Checked]);
}

#[test]
fn lookup_result_used_for_authorization_is_reported_distinctly() {
    let compiled = compile_body(
        r#"const owner = get("https://example.com/owner"); check("fetch", { owner }); get("https://example.com/data");"#,
    );
    assert_eq!(
        statuses(&compiled),
        [AuthorityGuardStatus::Lookup, AuthorityGuardStatus::Checked]
    );
    assert!(
        compiled
            .warnings
            .iter()
            .any(|warning| warning.message.contains("pre-check authorization lookup"))
    );
}

#[test]
fn recursive_summaries_do_not_invent_normal_completion() {
    let compiled = compile_output(
        &[(
            "lib",
            r#"
        import { check } from "submilli:security";
        import { get } from "submilli:http";
        function endless(): void { endless(); }
        export function fetch(): void { endless(); get("https://example.com"); }
    "#,
        )],
        &[],
    );
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Unreachable]);
    let compiled = compile_output(
        &[(
            "lib",
            r#"
        import { check } from "submilli:security";
        import { get } from "submilli:http";
        function recurse(flag: boolean): void { if (flag) { return; } recurse(true); get("https://example.com"); }
        export function fetch(): void { check("fetch", {}); recurse(false); }
    "#,
        )],
        &[],
    );
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Checked]);
}

#[test]
fn reassignment_invalidates_item_facts() {
    let compiled = compile_body(
        r#"let url = "https://example.com/a"; check("fetch", { url }); url = "https://example.com/b"; get(url);"#,
    );
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Unproven]);
    let effect = &route(&compiled.authority_map, "#fetch").effects[0];
    assert_eq!(effect.guard.capabilities, ["fetch"]);
    assert_eq!(effect.guard.checks.len(), 1);
    assert!(
        compiled
            .warnings
            .iter()
            .any(|warning| warning.message.contains("authority coverage gap"))
    );
}

#[test]
fn authority_guard_fields_are_backward_compatible() {
    let compiled = compile_body(r#"check("fetch", {}); get("https://example.com");"#);
    let effect = &route(&compiled.authority_map, "#fetch").effects[0];
    let mut value = serde_json::to_value(effect).unwrap();
    value.as_object_mut().unwrap().remove("guard");
    let old: AuthorityRouteEffect = serde_json::from_value(value).unwrap();
    assert_eq!(old.guard.status, AuthorityGuardStatus::Unproven);
    assert_eq!(
        serde_json::to_string(&compiled.authority_map).unwrap(),
        serde_json::to_string(
            &compile_body(r#"check("fetch", {}); get("https://example.com");"#).authority_map
        )
        .unwrap()
    );
}

#[test]
fn helper_loops_require_aggregate_scope_from_the_public_check() {
    for (payload, expected) in [
        ("{ first: urls[0] }", AuthorityGuardStatus::Unproven),
        ("{ urls }", AuthorityGuardStatus::Checked),
    ] {
        let source = format!(
            r#"
            import {{ check }} from "submilli:security";
            import {{ get }} from "submilli:http";
            function send(url: string): void {{ get(url); }}
            function batch(urls: string[]): void {{ for (const url of urls) {{ send(url); }} }}
            export function fetch(urls: string[]): void {{ check("fetch", {payload}); batch(urls); }}
        "#
        );
        let compiled = compile_output(&[("lib", &source)], &[]);
        assert_eq!(
            statuses(&compiled),
            [expected],
            "{payload}: {:#?}",
            compiled.warnings
        );
    }
}

#[test]
fn switch_and_for_continue_paths_do_not_bypass_guards() {
    let compiled = compile_body(
        r#"
        switch (flag) { case true: check("fetch", {}); break; default: return; }
        get("https://example.com");
    "#,
    );
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Checked]);
    let compiled = compile_body(
        r#"
        for (let i = 0; i < 2; i++) {
            if (flag) { continue; }
            check("fetch", {}); get("https://example.com");
        }
        get("https://example.com/after");
    "#,
    );
    assert!(statuses(&compiled).contains(&AuthorityGuardStatus::Checked));
    assert!(statuses(&compiled).contains(&AuthorityGuardStatus::Unguarded));
}

#[test]
fn evaluation_of_check_arguments_happens_before_authority_is_added() {
    let compiled = compile_body(r#"check("fetch", { response: get("https://example.com") });"#);
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Lookup]);
    let source = r#"
        import { check } from "submilli:security";
        import { get } from "submilli:http";
        function approve(): boolean { check("fetch", {}); return true; }
        export function fetch(flag: boolean): void { flag && approve(); get("https://example.com"); }
    "#;
    let compiled = compile_output(&[("lib", source)], &[]);
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Unguarded]);
}

#[test]
fn stable_item_aliases_and_route_wide_checks_survive_unrelated_mutations() {
    let compiled = compile_output(
        &[(
            "lib",
            r#"
        import { check } from "submilli:security";
        import { get } from "submilli:http";
        export function fetch(urls: string[]): void {
            for (const url of urls) { const snapshot = url; check("fetch", { snapshot }); get(url); }
        }
    "#,
        )],
        &[],
    );
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Checked]);
    let compiled = compile_body(
        r#"let url = "https://example.com"; check("route", {}); check("item", { url }); url = "https://example.com/other"; get(url);"#,
    );
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Checked]);
}

#[test]
fn dynamic_call_targets_remain_unresolved_with_independent_guard_ordering() {
    for (body, expected) in [
        (
            r#"check("fetch", {}); callback();"#,
            AuthorityGuardStatus::Checked,
        ),
        (
            r#"callback(); check("fetch", {});"#,
            AuthorityGuardStatus::Unproven,
        ),
        (
            r#"try { check("fetch", {}); } catch {} callback();"#,
            AuthorityGuardStatus::Unproven,
        ),
    ] {
        let source = format!(
            r#"
            import {{ check }} from "submilli:security";
            export function fetch(callback: () => void): void {{ {body} }}
        "#
        );
        let compiled = compile_output(&[("lib", &source)], &[]);
        let effects = &route(&compiled.authority_map, "#fetch").effects;
        assert!(!effects.is_empty());
        assert!(effects.iter().all(|effect| effect.effect.unresolved));
        assert!(
            effects.iter().all(|effect| effect.guard.status == expected),
            "{body}"
        );
        assert_eq!(
            !compiled
                .warnings
                .iter()
                .any(|warning| warning.message.starts_with("public route")),
            expected == AuthorityGuardStatus::Checked,
            "{body}: {:#?}",
            compiled.warnings
        );
    }
}

#[test]
fn nesting_exhaustion_reports_a_typed_limit_instead_of_recursing_unboundedly() {
    let mut ta = TypedAst::new();
    let (sources, file) = Sources::single("lib.ts", "").unwrap();
    let span = Span::at(file);
    let mut body = ta
        .try_push_stmt(crate::TypedStmt {
            kind: TypedStmtKind::Block(Vec::new()),
            span,
        })
        .unwrap();
    for _ in 0..100 {
        body = ta
            .try_push_stmt(crate::TypedStmt {
                kind: TypedStmtKind::Block(vec![body]),
                span,
            })
            .unwrap();
    }
    let declaration = PackageDeclaration::with_package("@test/guard-depth");
    let mut builder = Builder::new(
        &declaration,
        &ta,
        &sources,
        std::iter::empty(),
        AnalysisLimits::default(),
    )
    .unwrap();
    builder
        .push_node(
            "depth".into(),
            AuthorityCallableKind::Function,
            "depth".into(),
            span,
            AuthorityExposure::Private,
            vec![body],
            Vec::new(),
        )
        .unwrap();
    let Err(error) = analyse(&builder) else {
        panic!("expected nesting limit")
    };
    assert!(matches!(error, CompilerFailure::Limit { span: Some(actual), .. } if actual == span));
}

#[test]
fn catchable_arithmetic_and_index_writes_reach_unchecked_catches() {
    for operation in ["1n / divisor", "1n % divisor", "1n ** divisor"] {
        let source = format!(
            r#"
            import {{ check }} from "submilli:security";
            import {{ get }} from "submilli:http";
            export function fetch(divisor: bigint): void {{
                try {{ const result = {operation}; }} catch {{ get("https://example.com"); }}
                check("fetch", {{}});
            }}
        "#
        );
        let compiled = compile_output(&[("lib", &source)], &[]);
        assert_eq!(
            statuses(&compiled),
            [AuthorityGuardStatus::Unguarded],
            "{operation}"
        );
    }
    let compiled = compile_output(
        &[(
            "lib",
            r#"
        import { check } from "submilli:security";
        import { get } from "submilli:http";
        export function fetch(urls: string[]): void {
            try { urls[0] = "new"; } catch { get("https://example.com"); }
            check("fetch", {});
        }
    "#,
        )],
        &[],
    );
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Unguarded]);
}

#[test]
fn helper_mutation_order_is_preserved_at_effects() {
    for (body, expected) in [
        (
            r#"urls[0] = "https://example.com/new"; get(urls[0]);"#,
            AuthorityGuardStatus::Unproven,
        ),
        (
            r#"get(urls[0]); urls[0] = "https://example.com/new";"#,
            AuthorityGuardStatus::Checked,
        ),
    ] {
        let source = format!(
            r#"
            import {{ check }} from "submilli:security";
            import {{ get }} from "submilli:http";
            function helper(urls: string[]): void {{ {body} }}
            export function fetch(urls: string[]): void {{ check("fetch", {{ urls }}); helper(urls); }}
        "#
        );
        let compiled = compile_output(&[("lib", &source)], &[]);
        assert_eq!(
            statuses(&compiled),
            [expected],
            "{body}: {:#?}",
            compiled.warnings
        );
    }
}

#[test]
fn shadowed_iteration_bindings_cannot_authorize_each_other() {
    let compiled = compile_output(
        &[(
            "lib",
            r#"
        import { check } from "submilli:security";
        import { get } from "submilli:http";
        export function fetch(urls: string[]): void {
            for (const url of urls) {
                { const url = "https://example.com/fixed"; check("fetch", { url }); }
                get(url);
            }
        }
    "#,
        )],
        &[],
    );
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Unproven]);
    assert!(
        compiled
            .warnings
            .iter()
            .any(|warning| warning.message.contains("authority coverage gap"))
    );
}

#[test]
fn check_payload_user_dispatch_is_an_explicit_coverage_gap() {
    let compiled = compile_output(
        &[(
            "lib",
            r#"
        import { check } from "submilli:security";
        import { get } from "submilli:http";
        class Payload { toJson(): string { get("https://example.com/leak"); return "{}"; } }
        export function fetch(): void { const payload = new Payload(); check("fetch", payload); get("https://example.com/after"); }
    "#,
        )],
        &[],
    );
    assert!(
        route(&compiled.authority_map, "#fetch")
            .effects
            .iter()
            .any(
                |effect| effect.guard.status == AuthorityGuardStatus::Unproven
                    && effect
                        .effect
                        .reason
                        .as_ref()
                        .is_some_and(|reason| reason.contains("toJson"))
            )
    );
    assert!(
        compiled
            .warnings
            .iter()
            .any(|warning| warning.message.contains("authority coverage gap"))
    );
}

#[test]
fn catch_local_shadow_does_not_authorize_an_outer_iteration_item() {
    let compiled = compile_output(
        &[(
            "lib",
            r#"
        import { check } from "submilli:security";
        import { get } from "submilli:http";
        export function fetch(urls: string[]): void {
            for (const url of urls) {
                try { const ignored = 1n / 0n; return; }
                catch (url) { check("fetch", { message: url.message }); }
                get(url);
            }
        }
    "#,
        )],
        &[],
    );
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Unproven]);
}

#[test]
fn opaque_payload_dispatch_invalidates_previous_collection_checks() {
    let compiled = compile_output(
        &[(
            "lib",
            r#"
        import { check } from "submilli:security";
        import { get } from "submilli:http";
        class Payload {
            urls: string[];
            constructor(urls: string[]) { this.urls = urls; }
            toJson(): string { this.urls[0] = "https://example.com/changed"; return "{}"; }
        }
        export function fetch(urls: string[]): void {
            const payload = new Payload(urls);
            check("batch", { urls });
            check("payload", payload);
            for (const url of urls) { get(url); }
        }
    "#,
        )],
        &[],
    );
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Unproven]);
    assert!(
        compiled
            .warnings
            .iter()
            .any(|warning| warning.message.contains("authority coverage gap"))
    );
}

#[test]
fn narrowed_shadowed_bindings_cannot_authorize_each_other() {
    let compiled = compile_output(
        &[(
            "lib",
            r#"
        import { check } from "submilli:security";
        import { get } from "submilli:http";
        export function fetch(urls: (string | null)[]): void {
            for (const url of urls) {
                try { const ignored = 1n / 0n; return; }
                catch (url) {
                    if (url instanceof RangeError) { check("fetch", { message: url.message }); }
                    else { return; }
                }
                if (url !== null) { get(url); }
            }
        }
    "#,
        )],
        &[],
    );
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Unproven]);
}

#[test]
fn computed_method_payload_does_not_authorize_a_collection() {
    for value in ["urls.at(0)", "urls?.at(0)"] {
        let source = format!(
            r#"
            import {{ check }} from "submilli:security";
            import {{ get }} from "submilli:http";
            export function fetch(urls: string[]): void {{
                check("fetch", {{ first: {value} }});
                for (const url of urls) {{ get(url); }}
            }}
        "#
        );
        let compiled = compile_output(&[("lib", &source)], &[]);
        assert_eq!(
            statuses(&compiled),
            [AuthorityGuardStatus::Unproven],
            "{value}"
        );
    }
}

#[test]
fn computed_iteration_inputs_have_unproven_applicability() {
    for input in ["url.toLowerCase()", "url.slice(0)"] {
        let source = format!(
            r#"
            import {{ check }} from "submilli:security";
            import {{ get }} from "submilli:http";
            export function fetch(urls: string[]): void {{
                check("fetch", {{ first: urls.at(0) }});
                for (const url of urls) {{ get({input}); }}
            }}
        "#
        );
        let compiled = compile_output(&[("lib", &source)], &[]);
        assert_eq!(
            statuses(&compiled),
            [AuthorityGuardStatus::Unproven],
            "{input}"
        );
    }
}

#[test]
fn computed_call_inputs_propagate_uncertainty_to_transitive_effects() {
    for body in [
        "check(\"fetch\", { first: urls.at(0) }); for (const url of urls) { send(url.toLowerCase()); }",
        "check(\"fetch\", { first: urls.at(0) }); batch(urls);",
    ] {
        let source = format!(
            r#"
            import {{ check }} from "submilli:security";
            import {{ get }} from "submilli:http";
            function send(url: string): void {{ get(url); }}
            function batch(urls: string[]): void {{ for (const url of urls) {{ send(url.toLowerCase()); }} }}
            export function fetch(urls: string[]): void {{ {body} }}
        "#
        );
        let compiled = compile_output(&[("lib", &source)], &[]);
        assert_eq!(
            statuses(&compiled),
            [AuthorityGuardStatus::Unproven],
            "{body}"
        );
    }
}

#[test]
fn narrowed_index_payload_does_not_authorize_the_whole_collection() {
    let compiled = compile_output(
        &[(
            "lib",
            r#"
        import { check } from "submilli:security";
        import { get } from "submilli:http";
        export function fetch(urls: (string | null)[]): void {
            if (urls[0] !== null) { check("fetch", { first: urls[0] }); }
            else { return; }
            for (const url of urls) { if (url !== null) { get(url); } }
        }
    "#,
        )],
        &[],
    );
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Unproven]);
}

#[test]
fn computed_loop_conditions_and_updates_remain_unproven() {
    for body in [
        "for (let i = 0; i < urls.length; send(urls.at(i)!)) { i++; }",
        "while (send(urls.at(0)!)) { break; }",
        "do { break; } while (send(urls.at(0)!));",
    ] {
        let source = format!(
            r#"
            import {{ check }} from "submilli:security";
            import {{ get }} from "submilli:http";
            function send(url: string): boolean {{ get(url); return true; }}
            export function fetch(urls: string[]): void {{
                check("fetch", {{ first: urls.at(0) }}); {body}
            }}
        "#
        );
        let compiled = compile_output(&[("lib", &source)], &[]);
        let expected = if body.starts_with("do") {
            AuthorityGuardStatus::Unreachable
        } else {
            AuthorityGuardStatus::Unproven
        };
        assert_eq!(statuses(&compiled), [expected], "{body}");
    }
}

#[test]
fn quickstart_ledger_access_has_proven_guard_ordering() {
    let compiled = compile_output(
        &[(
            "lib",
            include_str!("../../../../../examples/quickstart/package/src/lib.ts"),
        )],
        &[],
    );
    let effects = &route(&compiled.authority_map, "#listCharges").effects;
    assert!(!effects.is_empty());
    assert!(
        effects
            .iter()
            .all(|effect| effect.guard.status == AuthorityGuardStatus::Checked)
    );
    assert!(effects.iter().any(|effect| effect.effect.unresolved));
    assert!(compiled.warnings.is_empty(), "{:#?}", compiled.warnings);
}

#[test]
fn unknown_accessor_dispatch_preserves_separate_guard_evidence() {
    for (body, expected) in [
        (
            "check(\"read\", {}); return value.property;",
            AuthorityGuardStatus::Checked,
        ),
        (
            "const result = value.property; check(\"read\", {}); return result;",
            AuthorityGuardStatus::Unproven,
        ),
        (
            "try { check(\"read\", {}); } catch {} return value.property;",
            AuthorityGuardStatus::Unproven,
        ),
    ] {
        let source = format!(
            r#"
            import {{ check }} from "submilli:security";
            export interface Shaped {{ readonly property: string; }}
            export function read(value: Shaped): string {{ {body} }}
        "#
        );
        let compiled = compile_output(&[("lib", &source)], &[]);
        let effects = &route(&compiled.authority_map, "#read").effects;
        let accessor = effects
            .iter()
            .find(|effect| {
                effect.effect.reason.as_deref()
                    == Some("structural property may invoke an accessor")
            })
            .expect("accessor uncertainty remains visible");
        assert!(accessor.effect.unresolved);
        assert_eq!(accessor.guard.status, expected, "{body}");
        assert_eq!(
            !compiled
                .warnings
                .iter()
                .any(|warning| warning.message.starts_with("public route")),
            expected == AuthorityGuardStatus::Checked,
            "{body}: {:#?}",
            compiled.warnings
        );
    }
}

#[test]
fn opaque_dispatch_keeps_ordering_but_invalidates_collection_scopes() {
    for body in [
        "callback(urls);",
        "invoke(urls, callback);",
        "try { callback(urls); } catch {}",
    ] {
        let source = format!(
            r#"
            import {{ check }} from "submilli:security";
            import {{ get }} from "submilli:http";
            function invoke(urls: string[], callback: (urls: string[]) => void): void {{ callback(urls); }}
            export function fetch(urls: string[], callback: (urls: string[]) => void): void {{
                check("batch", {{ urls }});
                {body}
                for (const url of urls) {{ get(url); }}
            }}
        "#
        );
        let compiled = compile_output(&[("lib", &source)], &[]);
        assert_eq!(
            statuses(&compiled),
            [AuthorityGuardStatus::Unproven],
            "{body}"
        );
        let effects = &route(&compiled.authority_map, "#fetch").effects;
        assert!(
            effects
                .iter()
                .filter(|effect| effect.effect.unresolved)
                .all(|effect| effect.guard.status == AuthorityGuardStatus::Checked)
        );
    }
}

#[test]
fn opaque_dispatch_does_not_revoke_successful_check_ordering() {
    let compiled = compile_output(
        &[(
            "lib",
            r#"
        import { check } from "submilli:security";
        import { get } from "submilli:http";
        export function fetch(url: string, callback: () => void): void {
            check("fetch", {}); callback(); get(url);
        }
    "#,
        )],
        &[],
    );
    assert_eq!(statuses(&compiled), [AuthorityGuardStatus::Checked]);
}
