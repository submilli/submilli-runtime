// test262: test/built-ins/RegExp/15.10.2.5-3-1.js
// expect-error: invalid regex pattern
// Documented divergence: regex literals are validated at compile time
// (docs/regex.md), so the SyntaxError JS throws for {2,1} (max < min) becomes
// a compile diagnostic here instead of a catchable runtime throw.

function main(): void {
  const r = /0{2,1}/;
  assert(r.source === "0{2,1}", "never reached");
}
