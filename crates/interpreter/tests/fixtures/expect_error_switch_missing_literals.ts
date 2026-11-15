// expect-error: missing case(s) "y", "z" for discriminant `.kind`
// expect-error: missing case(s) 1e+21
// expect-error-count: 2
type Variant = { a: 1; kind: "x" } | { a: 2; kind: "y" } | { a: 3; kind: "z" };
function tags(v: Variant): void {
    switch (v.kind) { case "x": break; }
}
function numbers(v: 1 | 1e21): void {
    switch (v) { case 1: break; }
}
function main(): void {}
