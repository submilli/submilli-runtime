// expect-error: unary `-` not defined for `number | null`
// expect-error: cannot read field `toString` on `string | null`
// expect-error: cannot read field `toString` on `number | null`
// expect-error: cannot read field `length` on `string | null`
// A reassignment whose RHS type *equals* the binding's declared type establishes
// no narrowing — but it still destroys the guard's. Reading through the dead
// guard used to compile and return the pre-assignment value: a silent wrong
// answer, not a trap. `v = null` was always rejected, which is why only the
// same-declared-type RHS slipped through.

function give(): number | null {
  return null;
}

let g: string | null = "a";

function local(w: number | null): number {
  let v: number | null = 1;
  if (v !== null) {
    v = w;
    return -v;
  }
  return 0;
}

function global(w: string | null): string {
  if (g !== null) {
    g = w;
    return g.toString();
  }
  return "";
}

function fromCall(): number {
  let v: number | null = 1;
  if (v !== null) {
    v = give();
    return v.toString().length;
  }
  return 0;
}

// The other half of the branch rule: a write in one arm *does* reach the code
// after the join, which both arms flow into. Nested one region deeper than the
// guard, so it also pins that the kill is not limited to the writing frame.
function afterTheJoin(c: boolean): number {
  let s: string | null = "a";
  if (s !== null) {
    if (c) {
      s = null;
    }
    return s.length;
  }
  return 0;
}

function main(): void {
  assert(local(null) === 0, "unreachable");
  assert(global(null) === "", "unreachable");
  assert(fromCall() === 0, "unreachable");
  assert(afterTheJoin(true) === 0, "unreachable");
}
