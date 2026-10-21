// Equality with an enum member narrows a value that may hold anything, or
// a number for a numeric member, to the member type, as tsc does.
enum NumberEnum { A, B, C }
enum StringEnum { A = "A", B = "B", C = "C" }
function f(u: unknown): string {
  if (u === NumberEnum.A) { const a: NumberEnum.A = u; return "NA" + String(a); }
  if (u === StringEnum.B) { const b: StringEnum.B = u; return b; }
  switch (u) {
    case StringEnum.A: { const a: StringEnum.A = u; return a; }
    case StringEnum.C: { const c: StringEnum.C = u; return c; }
  }
  if (u !== NumberEnum.B && u !== NumberEnum.C) { return "other"; }
  const bc: NumberEnum.B | NumberEnum.C = u;
  return "N" + String(bc);
}
function g(n: number): number {
  if (n === NumberEnum.B) { const b: NumberEnum.B = n; return b; }
  return -1;
}
function whole(u: unknown, e: StringEnum): string {
  if (u === e) {
    const s: StringEnum = u;
    return s;
  }
  return "no";
}
function main(): void {
  assert(f(0) === "NA0");
  assert(f("B") === "B");
  assert(f("A") === "A");
  assert(f("C") === "C");
  assert(f(1) === "N1");
  assert(f(2) === "N2");
  assert(f("x") === "other");
  assert(g(1) === 1);
  assert(whole("B", StringEnum.B) === "B");
  assert(whole("x", StringEnum.B) === "no");
  assert(g(5) === -1);
}
