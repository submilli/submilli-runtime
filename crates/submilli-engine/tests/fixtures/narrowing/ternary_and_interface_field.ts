// Regression: `&&`-narrowing flowing into a ternary then-branch, where the
// narrowed path is an interface field. Codegen previously panicked because the
// then-branch's synthesized narrowing source couldn't reconstruct the field
// access on the interface-typed (and nullable) receiver, falling back to a
// stale binding that wasn't in codegen scope.

interface Opt {
    name?: string;
}

function pickName(o: Opt | null, fallback: string): string {
    return o !== null && o.name !== undefined ? o.name : fallback;
}

function main(): void {
    assert(pickName({ name: "x" }, "d") === "x", "present field returned through the ternary");
    assert(pickName({}, "d") === "d", "absent optional field falls back");
    assert(pickName(null, "d") === "d", "null receiver falls back");
}
