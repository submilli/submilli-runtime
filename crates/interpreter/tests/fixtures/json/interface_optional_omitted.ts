// Regression: a named `interface` with optional fields must omit absent
// optionals from `JSON.stringify` (like an inline object type does), not
// materialize them as `null` slots. The optional flag is now threaded through
// the typed field origins into the shape collector and codegen, so the
// per-shape `to_json` skips the absent fields.

interface Opt {
    a?: string;
    b?: string;
    c?: number;
}

function main(): void {
    const partial: Opt = { a: "x" };
    assert(JSON.stringify(partial) === "{\"a\":\"x\"}", "absent optionals omitted, not null");
    assert(partial.b === undefined, "absent optional reads undefined");

    const empty: Opt = {};
    assert(JSON.stringify(empty) === "{}", "all-absent interface serializes empty");

    const full: Opt = { a: "x", b: "y", c: 3 };
    assert(JSON.stringify(full) === "{\"a\":\"x\",\"b\":\"y\",\"c\":3}", "present optionals emitted");
}
