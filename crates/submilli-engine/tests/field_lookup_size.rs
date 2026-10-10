use submilli_engine::{ModulePath, PackageSourceModule, compile_package};

fn module_size(accesses: usize, write: bool) -> usize {
    let statement = if write {
        "o.a = o.a + 1;\n"
    } else {
        "t = t + o.a;\n"
    };
    let source = format!(
        "export interface Big {{ a: number; b: string; c: boolean; d: number; }}\n\
         export function probe(o: Big): number {{ let t: number = 0; {} return t; }}",
        statement.repeat(accesses),
    );
    let package = compile_package(
        "lookup-size",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: &source,
        }],
        &[],
    )
    .expect("compile property accesses");
    wasmparser::Parser::new(0)
        .parse_all(&package.wasm)
        .find_map(|payload| match payload.expect("valid Wasm") {
            wasmparser::Payload::CodeSectionStart { range, .. } => Some(range.len()),
            _ => None,
        })
        .expect("code section")
}

#[test]
fn field_lookup_has_bounded_marginal_module_size() {
    for (write, limit) in [(false, 1200), (true, 2800)] {
        let marginal = (module_size(101, write) - module_size(1, write)) / 100;
        eprintln!("write={write}: {marginal} bytes per access");
        assert!(
            marginal < limit,
            "field scan must be shared: {marginal} >= {limit}"
        );
    }
}
