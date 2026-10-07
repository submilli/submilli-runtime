// A `case` label can be any expression, compared with `===` at run time. The
// discriminant runs once, labels run in order until one matches, and a label of
// literal type narrows like a literal label.
const zero: 0 = 0;
const one: 1 = 1;
const two: 2 = 2;
let oneOrTwo: 1 | 2 = 2;
function exhaustive(x: 0 | 1 | 2): string {
  switch (x) {
    case zero: { const a: 0 = x; return `zero${a}`; }
    case one: { const b: 1 = x; return `one${b}`; }
    case two: return "two";
  }
}
function unionLabel(x: 0 | 1 | 2): string {
  switch (x) {
    case zero: return "zero";
    case oneOrTwo: { const c: 1 | 2 = x; return `label${c}`; }
    default: { const d: 1 | 2 = x; return `default${d}`; }
  }
}
let runs: string[] = [];
function track(name: string, value: number): number { runs.push(name); return value; }
function ordered(x: number): string {
  switch (track("disc", x)) {
    case track("a", 1): return "a";
    case track("b", 2): case track("c", 3): return "bc";
    default: return "none";
  }
}
function inClosure(base: number): (n: number) => string {
  return (n: number): string => {
    switch (n) { case base: return "base"; case base + 1: return "next"; default: return "far"; }
  };
}
function strings(x: string, prefix: string): string {
  switch (x) { case prefix + "c": return "c"; case `${prefix}d`: return "d"; default: return "?"; }
}
function main(): void {
  assert(exhaustive(0) === "zero0" && exhaustive(1) === "one1" && exhaustive(2) === "two", "literal-typed labels");
  assert(unionLabel(0) === "zero" && unionLabel(2) === "label2" && unionLabel(1) === "default1", "a union-typed label narrows its clause only");
  assert(ordered(3) === "bc", "a later label matches");
  assert(runs.join(",") === "disc,a,b,c", "the discriminant runs once and labels run in order");
  runs = [];
  assert(ordered(1) === "a" && runs.join(",") === "disc,a", "labels after a match don't run");
  const near = inClosure(5);
  assert(near(5) === "base" && near(6) === "next" && near(7) === "far", "labels read captured values");
  assert(strings("abc", "ab") === "c" && strings("abd", "ab") === "d" && strings("x", "ab") === "?", "string labels");
}
