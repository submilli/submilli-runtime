// A signed number is a literal `case` label, as in TypeScript. `-0` matches `0`,
// as `===` does.
function f(x: number): string {
  switch (x) {
    case -1: return "neg";
    case +2: return "pos";
    case -0: return "zero";
    case (-3): return "paren";
    default: return "other";
  }
}
function main(): void {
  assert(f(-1) === "neg", "case -1");
  assert(f(2) === "pos", "case +2");
  assert(f(0) === "zero" && f(-0) === "zero", "case -0 matches both zeros");
  assert(f(-3) === "paren", "case (-3)");
  assert(f(1) === "other", "default");
}
