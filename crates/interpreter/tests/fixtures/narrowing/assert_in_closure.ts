// An assertion inside a closure narrows a captured variable, including
// through a condition held in a `const` outside the closure.
function check(c: boolean): asserts c {
  if (!c) throw new Error("check failed");
}
function assertIsString(x: unknown): asserts x is string {
  if (typeof x !== "string") throw new Error("not a string");
}

function aliased(v: unknown): number {
  const ok = typeof v === "string";
  const f = (): number => {
    check(ok);
    return v.length;
  };
  return f();
}

function direct(v: unknown): number {
  function inner(): number {
    assertIsString(v);
    return v.length;
  }
  return inner();
}

function main(): void {
  assert(aliased("abc") === 3, "aliased condition in a closure");
  assert(direct("ab") === 2, "assertion in a nested function");
  const w: unknown = "abcd";
  const g = (): number => {
    const ok = typeof w === "string";
    check(ok);
    return w.length;
  };
  assert(g() === 4, "aliased condition inside the closure");

  const u: unknown = "abcde";
  const uOk = typeof u === "string";
  const h = (): number => {
    check(uOk);
    return u.length;
  };
  assert(h() === 5, "aliased condition on a const local");
  function nested(): number {
    check(!!uOk);
    return u.length;
  }
  assert(nested() === 5, "negated alias in a nested function");
}
